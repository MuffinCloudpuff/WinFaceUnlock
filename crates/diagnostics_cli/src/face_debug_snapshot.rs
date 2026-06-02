use std::{
    fmt,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use face_engine::{
    FaceEngineError, FaceModelProvider, OpenCvFaceModelConfig, OpenCvFaceModelProvider,
};
use serde_json::json;
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoError, VideoFrameProvider,
};

#[derive(Clone, Debug)]
pub struct FaceDebugSnapshotConfig {
    pub output_dir: PathBuf,
    pub scenario: String,
    pub start_delay_seconds: u32,
    pub camera_id: Option<CameraId>,
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
    pub frames: u32,
    pub frame_delay_ms: u32,
    pub model_config: OpenCvFaceModelConfig,
    pub save_aligned_faces: bool,
}

#[derive(Debug)]
pub enum FaceDebugSnapshotError {
    IoFailed,
    Video(VideoError),
    Face(FaceEngineError),
}

impl fmt::Display for FaceDebugSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoFailed => write!(formatter, "I/O operation failed"),
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::Face(error) => write!(formatter, "face engine error: {error:?}"),
        }
    }
}

impl From<VideoError> for FaceDebugSnapshotError {
    fn from(value: VideoError) -> Self {
        Self::Video(value)
    }
}

impl From<FaceEngineError> for FaceDebugSnapshotError {
    fn from(value: FaceEngineError) -> Self {
        Self::Face(value)
    }
}

#[derive(Clone, Debug, Default)]
struct SnapshotSummary {
    requested_frame_count: u32,
    captured_frame_count: u32,
    no_face_frame_count: u32,
    single_face_frame_count: u32,
    multiple_face_frame_count: u32,
    embedding_extraction_succeeded_count: u32,
    embedding_extraction_failed_count: u32,
    total_detection_elapsed_ms: u128,
    max_detection_elapsed_ms: u128,
}

impl SnapshotSummary {
    fn detection_observed(&mut self, elapsed_ms: u128) {
        self.total_detection_elapsed_ms =
            self.total_detection_elapsed_ms.saturating_add(elapsed_ms);
        self.max_detection_elapsed_ms = self.max_detection_elapsed_ms.max(elapsed_ms);
    }

    fn average_detection_elapsed_ms(&self) -> Option<f64> {
        if self.captured_frame_count == 0 {
            return None;
        }
        Some(self.total_detection_elapsed_ms as f64 / f64::from(self.captured_frame_count))
    }
}

pub fn run_face_debug_snapshot(
    config: FaceDebugSnapshotConfig,
) -> Result<(), FaceDebugSnapshotError> {
    let frames_dir = config.output_dir.join("annotated_frames");
    let aligned_faces_dir = config.output_dir.join("aligned_faces");
    fs::create_dir_all(&frames_dir).map_err(|_| FaceDebugSnapshotError::IoFailed)?;
    if config.save_aligned_faces {
        fs::create_dir_all(&aligned_faces_dir).map_err(|_| FaceDebugSnapshotError::IoFailed)?;
    }

    let metrics_path = config.output_dir.join("frame_metrics.jsonl");
    let summary_path = config.output_dir.join("summary.json");
    let mut metrics_file =
        File::create(&metrics_path).map_err(|_| FaceDebugSnapshotError::IoFailed)?;

    let mut camera_provider = OpenCvCameraProvider::new(OpenCvCameraProviderConfig {
        max_camera_index: config.max_camera_index,
        requested_frame_width: config.requested_frame_width,
        requested_frame_height: config.requested_frame_height,
    });
    let sources = camera_provider.list_sources()?;
    let camera_id = selected_camera_id(config.camera_id, &sources)?;
    camera_provider.open(&camera_id)?;

    let mut model_provider = OpenCvFaceModelProvider::new(config.model_config);
    model_provider.load_models()?;

    print_scenario_prompt(&config.scenario, config.start_delay_seconds);

    let mut summary = SnapshotSummary {
        requested_frame_count: config.frames,
        ..SnapshotSummary::default()
    };

    for frame_index in 0..config.frames {
        let frame = camera_provider.read_frame()?;
        summary.captured_frame_count = summary.captured_frame_count.saturating_add(1);

        let detection_started_at = Instant::now();
        let faces = model_provider.detect(&frame)?;
        let detection_elapsed_ms = detection_started_at.elapsed().as_millis();
        summary.detection_observed(detection_elapsed_ms);

        match faces.len() {
            0 => summary.no_face_frame_count = summary.no_face_frame_count.saturating_add(1),
            1 => {
                summary.single_face_frame_count = summary.single_face_frame_count.saturating_add(1)
            }
            _ => {
                summary.multiple_face_frame_count =
                    summary.multiple_face_frame_count.saturating_add(1)
            }
        }

        let base_name = format!("{frame_index:05}");
        let annotated_frame_path = frames_dir.join(format!("{base_name}.jpg"));
        OpenCvFaceModelProvider::write_detection_debug_frame(
            &frame,
            &faces,
            &annotated_frame_path,
        )?;

        let mut embedding_dimensions = None;
        let mut aligned_face_path = None;
        let embedding_extraction_succeeded =
            if let Some(face) = faces.first().filter(|_| faces.len() == 1) {
                match model_provider.extract(&frame, face) {
                    Ok(embedding) => {
                        embedding_dimensions = Some(embedding.values.len());
                        summary.embedding_extraction_succeeded_count = summary
                            .embedding_extraction_succeeded_count
                            .saturating_add(1);

                        if config.save_aligned_faces {
                            let path = aligned_faces_dir.join(format!("{base_name}.jpg"));
                            model_provider.write_aligned_face(&frame, face, &path)?;
                            aligned_face_path = Some(path);
                        }
                        true
                    }
                    Err(_) => {
                        summary.embedding_extraction_failed_count =
                            summary.embedding_extraction_failed_count.saturating_add(1);
                        false
                    }
                }
            } else {
                false
            };

        let metric = json!({
            "frame_index": frame_index,
            "frame_width": frame.width,
            "frame_height": frame.height,
            "detected_face_count": faces.len(),
            "detection_elapsed_ms": detection_elapsed_ms,
            "embedding_extraction_succeeded": embedding_extraction_succeeded,
            "embedding_dimensions": embedding_dimensions,
            "annotated_frame_path": annotated_frame_path,
            "aligned_face_path": aligned_face_path,
            "faces": faces,
        });
        writeln!(
            metrics_file,
            "{}",
            serde_json::to_string(&metric).map_err(|_| FaceDebugSnapshotError::IoFailed)?
        )
        .map_err(|_| FaceDebugSnapshotError::IoFailed)?;

        if config.frame_delay_ms > 0 {
            thread::sleep(Duration::from_millis(u64::from(config.frame_delay_ms)));
        }
    }

    camera_provider.close();
    model_provider.unload_models();

    let summary_json = json!({
        "requested_frame_count": summary.requested_frame_count,
        "scenario": config.scenario,
        "captured_frame_count": summary.captured_frame_count,
        "no_face_frame_count": summary.no_face_frame_count,
        "single_face_frame_count": summary.single_face_frame_count,
        "multiple_face_frame_count": summary.multiple_face_frame_count,
        "embedding_extraction_succeeded_count": summary.embedding_extraction_succeeded_count,
        "embedding_extraction_failed_count": summary.embedding_extraction_failed_count,
        "average_detection_elapsed_ms": summary.average_detection_elapsed_ms(),
        "max_detection_elapsed_ms": summary.max_detection_elapsed_ms,
        "metrics_path": metrics_path,
        "annotated_frames_dir": frames_dir,
        "aligned_faces_dir": if config.save_aligned_faces {
            Some(aligned_faces_dir)
        } else {
            None
        },
    });
    fs::write(
        &summary_path,
        serde_json::to_vec_pretty(&summary_json).map_err(|_| FaceDebugSnapshotError::IoFailed)?,
    )
    .map_err(|_| FaceDebugSnapshotError::IoFailed)?;

    println!("face_debug_snapshot_completed: true");
    println!("scenario: {}", config.scenario);
    println!("camera_id: {}", camera_id.0);
    println!("captured_frame_count: {}", summary.captured_frame_count);
    println!(
        "single_face_frame_count: {}",
        summary.single_face_frame_count
    );
    println!(
        "multiple_face_frame_count: {}",
        summary.multiple_face_frame_count
    );
    println!("no_face_frame_count: {}", summary.no_face_frame_count);
    println!("metrics_path: {}", metrics_path.display());
    println!("summary_path: {}", summary_path.display());

    Ok(())
}

fn print_scenario_prompt(scenario: &str, start_delay_seconds: u32) {
    println!("scenario: {scenario}");
    println!("prompt: {}", scenario_prompt(scenario));
    for remaining in (1..=start_delay_seconds).rev() {
        println!("capture_starts_in_seconds: {remaining}");
        thread::sleep(Duration::from_secs(1));
    }
}

fn scenario_prompt(scenario: &str) -> &'static str {
    match scenario {
        "front" | "frontal" => "请正脸看摄像头并保持不动",
        "yaw-left" | "yaw-left-15" | "yaw-left-30" | "yaw-left-45" => "请向左转头并保持当前角度",
        "yaw-right" | "yaw-right-15" | "yaw-right-30" | "yaw-right-45" => {
            "请向右转头并保持当前角度"
        }
        "pitch-up" | "pitch-up-15" | "pitch-up-30" => "请稍微抬头并保持不动",
        "pitch-down" | "pitch-down-15" | "pitch-down-30" => "请稍微低头并保持不动",
        "backlight" => "请保持背光场景，正脸看摄像头",
        "low-light" => "请保持弱光场景，正脸看摄像头",
        "glasses-reflection" => "请保持眼镜反光场景，正脸看摄像头",
        "usb-low-res" => "请使用低像素 USB 摄像头，正脸看摄像头",
        _ => "请保持当前姿态，系统将采集调试帧",
    }
}

fn selected_camera_id(
    requested_camera_id: Option<CameraId>,
    sources: &[video_provider::CameraInfo],
) -> Result<CameraId, FaceDebugSnapshotError> {
    if let Some(camera_id) = requested_camera_id {
        return Ok(camera_id);
    }

    sources
        .first()
        .map(|source| source.id.clone())
        .ok_or(FaceDebugSnapshotError::Video(VideoError::CameraNotFound))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_camera_prefers_explicit_camera_id() -> Result<(), FaceDebugSnapshotError> {
        let camera_id = selected_camera_id(Some(CameraId("opencv-index:7".to_owned())), &[])?;

        assert_eq!(camera_id, CameraId("opencv-index:7".to_owned()));
        Ok(())
    }

    #[test]
    fn selected_camera_falls_back_to_first_source() -> Result<(), FaceDebugSnapshotError> {
        let camera_id = selected_camera_id(
            None,
            &[video_provider::CameraInfo {
                id: CameraId("opencv-index:1".to_owned()),
                display_name: "Camera 1".to_owned(),
            }],
        )?;

        assert_eq!(camera_id, CameraId("opencv-index:1".to_owned()));
        Ok(())
    }
}
