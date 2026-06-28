use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use opencv::{
    core::{AlgorithmHint, Mat, Rect, Scalar, Vector},
    imgcodecs, imgproc,
    prelude::MatTraitConst,
};
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, PixelFormat, VideoFrame,
    VideoFrameProvider,
};

use crate::{
    camera_frame_recovery::{
        TransientFrameFailureDecision, TransientFrameFailureKind, TransientFrameFailureTolerance,
        validate_frame_for_camera_stream,
    },
    camera_runtime::{CameraLeaseKind, try_acquire_camera_lease},
    presence_monitor::{PresenceMonitorError, PresenceObservationSource},
    presence_person_detector::{
        PersonDetection, PersonDetector, PersonDetectorConfig, PresenceDetector,
        PresencePersonDetectorError,
    },
    presence_policy::PresenceObservation,
    service_log::write_service_event_detail,
};

pub struct PersonCameraPresenceObservationConfig {
    pub camera_id: CameraId,
    pub camera_config: OpenCvCameraProviderConfig,
    pub detector_config: PersonDetectorConfig,
    pub debug_output_dir: Option<PathBuf>,
}

pub struct PersonCameraPresenceObservationSource {
    camera_id: CameraId,
    camera_config: OpenCvCameraProviderConfig,
    camera_provider: OpenCvCameraProvider,
    detector: PersonDetector,
    debug_recorder: Option<PersonPresenceDebugRecorder>,
}

impl PersonCameraPresenceObservationSource {
    pub fn new(
        config: PersonCameraPresenceObservationConfig,
    ) -> Result<Self, PresenceMonitorError> {
        write_service_event_detail(
            "PresencePersonSource.CreateStarted",
            format!(
                "camera_id={} detector_config={:?} debug_output_dir={}",
                config.camera_id.0,
                config.detector_config,
                config
                    .debug_output_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<none>".to_owned())
            ),
        );
        let mut detector = PersonDetector::new(config.detector_config);
        write_service_event_detail("PresencePersonSource.LoadModelStarted", "");
        detector.load_model().map_err(|error| {
            write_service_event_detail(
                "PresencePersonSource.LoadModelFailed",
                format!("error={error:?} detail={error}"),
            );
            PresenceMonitorError::ObservationFailed
        })?;
        write_service_event_detail("PresencePersonSource.LoadModelSucceeded", "");
        let debug_recorder = config
            .debug_output_dir
            .map(PersonPresenceDebugRecorder::new)
            .transpose()
            .map_err(|error| {
                write_service_event_detail(
                    "PresencePersonSource.DebugRecorderFailed",
                    format!("error={error}"),
                );
                PresenceMonitorError::ObservationFailed
            })?;
        write_service_event_detail("PresencePersonSource.CreateSucceeded", "");

        Ok(Self {
            camera_id: config.camera_id,
            camera_config: config.camera_config.clone(),
            camera_provider: OpenCvCameraProvider::new(config.camera_config),
            detector,
            debug_recorder,
        })
    }
}

impl PresenceObservationSource for PersonCameraPresenceObservationSource {
    fn next_observation(&mut self) -> Result<Option<PresenceObservation>, PresenceMonitorError> {
        self.sample_once().map(Some)
    }
}

impl PersonCameraPresenceObservationSource {
    fn sample_once(&mut self) -> Result<PresenceObservation, PresenceMonitorError> {
        write_service_event_detail(
            "PresencePersonSource.SampleStarted",
            format!("camera_id={}", self.camera_id.0),
        );
        let mut frame_failure_tolerance =
            TransientFrameFailureTolerance::default_for_camera_stream();
        let _camera_lease = match try_acquire_camera_lease(CameraLeaseKind::PresenceLock) {
            Ok(lease) => lease,
            Err(reason) => {
                write_service_event_detail(
                    "PresencePersonSource.LeaseDenied",
                    format!("reason={reason:?}"),
                );
                return Ok(PresenceObservation::CameraUnavailable);
            }
        };
        write_service_event_detail("PresencePersonSource.LeaseAcquired", "");
        self.camera_provider = OpenCvCameraProvider::new(self.camera_config.clone());
        write_service_event_detail(
            "PresencePersonSource.OpenCameraStarted",
            format!("camera_id={}", self.camera_id.0),
        );
        if let Err(error) = self.camera_provider.open(&self.camera_id) {
            write_service_event_detail(
                "PresencePersonSource.OpenCameraFailed",
                format!("camera_id={} error={error:?}", self.camera_id.0),
            );
            self.camera_provider.close();
            write_service_event_detail("PresencePersonSource.SampleReleased", "");
            return Ok(PresenceObservation::CameraUnavailable);
        }
        write_service_event_detail(
            "PresencePersonSource.OpenCameraSucceeded",
            format!("camera_id={}", self.camera_id.0),
        );
        let observation = (|| {
            let (frame, detections) = loop {
                let frame = match self.camera_provider.read_frame() {
                    Ok(frame) => match validate_frame_for_camera_stream(&frame) {
                        Ok(()) => frame,
                        Err(kind) => {
                            match Self::record_transient_frame_failure(
                                &mut frame_failure_tolerance,
                                "ValidateFrame",
                                kind,
                            ) {
                                TransientFrameFailureOutcome::RetryNextFrame => continue,
                                TransientFrameFailureOutcome::CameraUnavailable => {
                                    return Ok(PresenceObservation::CameraUnavailable);
                                }
                            }
                        }
                    },
                    Err(error) => {
                        let Some(kind) = TransientFrameFailureKind::from_video_error(error.clone())
                        else {
                            write_service_event_detail(
                                "PresencePersonSource.ReadFrameFailed",
                                format!("error={error:?}"),
                            );
                            return Ok(PresenceObservation::CameraUnavailable);
                        };
                        match Self::record_transient_frame_failure(
                            &mut frame_failure_tolerance,
                            "ReadFrame",
                            kind,
                        ) {
                            TransientFrameFailureOutcome::RetryNextFrame => continue,
                            TransientFrameFailureOutcome::CameraUnavailable => {
                                return Ok(PresenceObservation::CameraUnavailable);
                            }
                        }
                    }
                };
                match self.detector.detect_persons(&frame) {
                    Ok(detections) => {
                        frame_failure_tolerance.record_valid_frame();
                        break (frame, detections);
                    }
                    Err(PresencePersonDetectorError::InvalidFrame) => {
                        write_service_event_detail(
                            "PresencePersonSource.DetectInvalidFrame",
                            format!(
                                "frame_width={} frame_height={} frame_format={:?} frame_data_len={}",
                                frame.width,
                                frame.height,
                                frame.format,
                                frame.data.len()
                            ),
                        );
                        match Self::record_transient_frame_failure(
                            &mut frame_failure_tolerance,
                            "DetectPersons",
                            TransientFrameFailureKind::InvalidFrame,
                        ) {
                            TransientFrameFailureOutcome::RetryNextFrame => continue,
                            TransientFrameFailureOutcome::CameraUnavailable => {
                                return Ok(PresenceObservation::CameraUnavailable);
                            }
                        }
                    }
                    Err(error) => {
                        write_service_event_detail(
                            "PresencePersonSource.DetectFailed",
                            format!("error={error:?} detail={error}"),
                        );
                        return Err(PresenceMonitorError::ObservationFailed);
                    }
                }
            };
            if let Some(debug_recorder) = &mut self.debug_recorder {
                debug_recorder.record(&frame, &detections);
            }
            let Some(primary_person) = largest_person_detection(&detections) else {
                return Ok(PresenceObservation::PersonAbsent);
            };

            let bbox_center_x_ratio = (primary_person.bbox.x as f32
                + primary_person.bbox.width as f32 / 2.0)
                / frame.width as f32;
            let bbox_area_ratio = (primary_person.bbox.width as f32
                * primary_person.bbox.height as f32)
                / (frame.width as f32 * frame.height as f32);

            Ok(PresenceObservation::PersonPresent {
                confidence: primary_person.confidence,
                bbox_center_x_ratio,
                bbox_area_ratio,
            })
        })();
        write_service_event_detail(
            "PresencePersonSource.SampleCompleted",
            format!("observation={observation:?}"),
        );
        self.camera_provider.close();
        write_service_event_detail("PresencePersonSource.SampleReleased", "");
        observation
    }
}

enum TransientFrameFailureOutcome {
    RetryNextFrame,
    CameraUnavailable,
}

impl PersonCameraPresenceObservationSource {
    fn record_transient_frame_failure(
        frame_failure_tolerance: &mut TransientFrameFailureTolerance,
        stage: &'static str,
        kind: TransientFrameFailureKind,
    ) -> TransientFrameFailureOutcome {
        match frame_failure_tolerance.record_transient_failure(kind) {
            TransientFrameFailureDecision::RetryNextFrame {
                consecutive_failures,
                max_consecutive_failures,
            } => {
                write_service_event_detail(
                    "PresencePersonSource.TransientFrameSkipped",
                    format!(
                        "stage={stage} reason={kind:?} consecutive_failures={consecutive_failures} max_consecutive_failures={max_consecutive_failures}"
                    ),
                );
                TransientFrameFailureOutcome::RetryNextFrame
            }
            TransientFrameFailureDecision::Escalate {
                consecutive_failures,
                max_consecutive_failures,
            } => {
                write_service_event_detail(
                    "PresencePersonSource.TransientFrameEscalated",
                    format!(
                        "stage={stage} reason={kind:?} consecutive_failures={consecutive_failures} max_consecutive_failures={max_consecutive_failures}"
                    ),
                );
                TransientFrameFailureOutcome::CameraUnavailable
            }
        }
    }
}

impl Drop for PersonCameraPresenceObservationSource {
    fn drop(&mut self) {
        self.detector.unload_model();
    }
}

fn largest_person_detection(detections: &[PersonDetection]) -> Option<&PersonDetection> {
    detections.iter().max_by(|left, right| {
        person_bbox_area(left)
            .partial_cmp(&person_bbox_area(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn person_bbox_area(detection: &PersonDetection) -> f32 {
    detection.bbox.width as f32 * detection.bbox.height as f32
}

const DEBUG_FRAME_RETENTION_COUNT: u64 = 240;

struct PersonPresenceDebugRecorder {
    output_dir: PathBuf,
    frame_index: u64,
}

impl PersonPresenceDebugRecorder {
    fn new(output_dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&output_dir)?;
        let metadata_path = output_dir.join("presence_person_observations.jsonl");
        if metadata_path.exists() {
            fs::remove_file(metadata_path)?;
        }
        Ok(Self {
            output_dir,
            frame_index: 0,
        })
    }

    fn record(&mut self, frame: &VideoFrame, detections: &[PersonDetection]) {
        self.frame_index = self.frame_index.saturating_add(1);
        let outcome = if detections.is_empty() { "miss" } else { "hit" };
        let slot = self.frame_index % DEBUG_FRAME_RETENTION_COUNT;
        let image_path = self
            .output_dir
            .join(format!("frame-{slot:04}-{outcome}.jpg"));

        let _ = save_debug_frame(&image_path, frame, detections);
        let primary = largest_person_detection(detections);
        let metadata = serde_json::json!({
            "frame_index": self.frame_index,
            "slot": slot,
            "outcome": outcome,
            "captured_at_unix_ms": current_time_unix_ms(),
            "image_path": image_path.display().to_string(),
            "frame_width": frame.width,
            "frame_height": frame.height,
            "detection_count": detections.len(),
            "primary_detection": primary.map(|detection| serde_json::json!({
                "confidence": detection.confidence,
                "bbox": {
                    "x": detection.bbox.x,
                    "y": detection.bbox.y,
                    "width": detection.bbox.width,
                    "height": detection.bbox.height,
                },
                "normalized_bbox": {
                    "x_min": detection.normalized_bbox.x_min,
                    "y_min": detection.normalized_bbox.y_min,
                    "x_max": detection.normalized_bbox.x_max,
                    "y_max": detection.normalized_bbox.y_max,
                }
            })),
        });

        if let Ok(mut metadata_file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.output_dir.join("presence_person_observations.jsonl"))
        {
            let _ = writeln!(metadata_file, "{metadata}");
        }
    }
}

fn save_debug_frame(
    image_path: &Path,
    frame: &VideoFrame,
    detections: &[PersonDetection],
) -> opencv::Result<bool> {
    let mut mat = video_frame_to_mat_for_debug(frame)?;
    for detection in detections {
        imgproc::rectangle(
            &mut mat,
            Rect::new(
                detection.bbox.x as i32,
                detection.bbox.y as i32,
                detection.bbox.width as i32,
                detection.bbox.height as i32,
            ),
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            2,
            imgproc::LINE_8,
            0,
        )?;
        imgproc::put_text(
            &mut mat,
            &format!("{:.2}", detection.confidence),
            opencv::core::Point::new(detection.bbox.x as i32, detection.bbox.y as i32 - 6),
            imgproc::FONT_HERSHEY_SIMPLEX,
            0.6,
            Scalar::new(0.0, 255.0, 0.0, 0.0),
            1,
            imgproc::LINE_8,
            false,
        )?;
    }

    imgcodecs::imwrite(
        &image_path.display().to_string(),
        &mat,
        &Vector::<i32>::new(),
    )
}

fn video_frame_to_mat_for_debug(frame: &VideoFrame) -> opencv::Result<Mat> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat = Mat::from_slice(&frame.data)?;
    let mat = mat.reshape(channels, frame.height as i32)?;
    let mut mat = mat.try_clone()?;

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )?;
        mat = bgr;
    }

    Ok(mat)
}

fn current_time_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    millis.min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use crate::presence_person_detector::{
        NormalizedPersonBoundingBox, PersonBoundingBox, PersonDetection,
    };

    use super::*;

    #[test]
    fn largest_person_detection_prefers_largest_bbox_area() {
        let smaller = PersonDetection {
            confidence: 0.90,
            bbox: PersonBoundingBox {
                x: 10,
                y: 10,
                width: 40,
                height: 50,
            },
            normalized_bbox: NormalizedPersonBoundingBox {
                x_min: 0.1,
                y_min: 0.1,
                x_max: 0.2,
                y_max: 0.2,
            },
        };
        let larger = PersonDetection {
            confidence: 0.70,
            bbox: PersonBoundingBox {
                x: 10,
                y: 10,
                width: 80,
                height: 60,
            },
            normalized_bbox: NormalizedPersonBoundingBox {
                x_min: 0.1,
                y_min: 0.1,
                x_max: 0.4,
                y_max: 0.4,
            },
        };

        assert_eq!(
            largest_person_detection(&[smaller, larger]).map(|detection| detection.confidence),
            Some(0.70)
        );
    }
}
