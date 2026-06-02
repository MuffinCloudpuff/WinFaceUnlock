use std::{
    fmt,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use face_engine::{
    DetectedFace, FaceEngineError, FaceModelProvider, OpenCvFaceModelConfig,
    OpenCvFaceModelProvider,
};
use face_liveness::{
    LivenessDecision, LivenessPolicyDecision, LivenessProviderError, LivenessResult,
    LivenessWindowPolicy, MiniFasNetLivenessProvider, MiniFasNetLivenessProviderConfig,
    ScreenReplayDebugFrameError, ScreenReplayLivenessProvider, ScreenReplayLivenessProviderConfig,
    ScreenReplayProviderSummary, write_screen_replay_debug_frame,
};
use serde_json::json;
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoError, VideoFrameProvider,
};

#[derive(Clone, Debug)]
pub struct LivenessScreenDebugConfig {
    pub output_dir: PathBuf,
    pub camera_id: Option<CameraId>,
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
    pub frames: u32,
    pub frame_delay_ms: u32,
    pub model_config: OpenCvFaceModelConfig,
    pub screen_replay_geometry_provider_config: Option<ScreenReplayLivenessProviderConfig>,
    pub minifasnet_provider_config: Option<MiniFasNetLivenessProviderConfig>,
    pub save_debug_images: bool,
    pub save_minifasnet_crops: bool,
}

#[derive(Debug)]
pub enum LivenessScreenDebugError {
    IoFailed,
    Video(VideoError),
    Face(FaceEngineError),
    Liveness(LivenessProviderError),
    DebugFrame(ScreenReplayDebugFrameError),
}

impl fmt::Display for LivenessScreenDebugError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoFailed => write!(formatter, "I/O operation failed"),
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::Face(error) => write!(formatter, "face engine error: {error:?}"),
            Self::Liveness(error) => write!(formatter, "liveness provider error: {error:?}"),
            Self::DebugFrame(error) => write!(formatter, "debug frame error: {error:?}"),
        }
    }
}

impl From<VideoError> for LivenessScreenDebugError {
    fn from(value: VideoError) -> Self {
        Self::Video(value)
    }
}

impl From<FaceEngineError> for LivenessScreenDebugError {
    fn from(value: FaceEngineError) -> Self {
        Self::Face(value)
    }
}

impl From<LivenessProviderError> for LivenessScreenDebugError {
    fn from(value: LivenessProviderError) -> Self {
        Self::Liveness(value)
    }
}

impl From<ScreenReplayDebugFrameError> for LivenessScreenDebugError {
    fn from(value: ScreenReplayDebugFrameError) -> Self {
        Self::DebugFrame(value)
    }
}

#[derive(Clone, Debug, Default)]
struct LivenessScreenDebugSummary {
    requested_frame_count: u32,
    captured_frame_count: u32,
    no_face_frame_count: u32,
    single_face_frame_count: u32,
    multiple_face_frame_count: u32,
    screen_rectangle_detected_frame_count: u32,
    face_inside_screen_candidate_frame_count: u32,
    spoof_rejected_frame_count: u32,
    inconclusive_frame_count: u32,
    minifasnet_evaluated_frame_count: u32,
    minifasnet_live_accepted_frame_count: u32,
    minifasnet_spoof_rejected_frame_count: u32,
    minifasnet_model_spoof_score_frame_count: u32,
    minifasnet_inconclusive_frame_count: u32,
    auth_window_spoof_rejected: bool,
    total_elapsed_ms: u128,
    max_elapsed_ms: u128,
}

impl LivenessScreenDebugSummary {
    fn record_elapsed(&mut self, elapsed_ms: u128) {
        self.total_elapsed_ms = self.total_elapsed_ms.saturating_add(elapsed_ms);
        self.max_elapsed_ms = self.max_elapsed_ms.max(elapsed_ms);
    }

    fn average_elapsed_ms(&self) -> Option<f64> {
        if self.captured_frame_count == 0 {
            return None;
        }
        Some(self.total_elapsed_ms as f64 / f64::from(self.captured_frame_count))
    }
}

pub fn run_liveness_screen_debug(
    config: LivenessScreenDebugConfig,
) -> Result<(), LivenessScreenDebugError> {
    fs::create_dir_all(&config.output_dir).map_err(|_| LivenessScreenDebugError::IoFailed)?;
    let frames_dir = config.output_dir.join("annotated_frames");
    if config.save_debug_images {
        fs::create_dir_all(&frames_dir).map_err(|_| LivenessScreenDebugError::IoFailed)?;
    }
    let minifasnet_crops_dir = config.output_dir.join("minifasnet_crops");
    let minifasnet_source_crops_dir = minifasnet_crops_dir.join("source");
    let minifasnet_model_inputs_dir = minifasnet_crops_dir.join("model_input_80x80");
    if config.save_minifasnet_crops {
        fs::create_dir_all(&minifasnet_source_crops_dir)
            .map_err(|_| LivenessScreenDebugError::IoFailed)?;
        fs::create_dir_all(&minifasnet_model_inputs_dir)
            .map_err(|_| LivenessScreenDebugError::IoFailed)?;
    }

    let metrics_path = config.output_dir.join("liveness_metrics.jsonl");
    let summary_path = config.output_dir.join("liveness_summary.json");
    let mut metrics_file =
        File::create(&metrics_path).map_err(|_| LivenessScreenDebugError::IoFailed)?;

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
    let screen_replay_geometry_provider = config
        .screen_replay_geometry_provider_config
        .clone()
        .map(ScreenReplayLivenessProvider::new);
    let mut minifasnet_provider = if let Some(provider_config) = config.minifasnet_provider_config {
        let mut provider = MiniFasNetLivenessProvider::new(provider_config);
        provider.load_model()?;
        Some(provider)
    } else {
        None
    };

    let mut summary = LivenessScreenDebugSummary {
        requested_frame_count: config.frames,
        ..LivenessScreenDebugSummary::default()
    };
    let mut frame_liveness_results = Vec::new();

    for frame_index in 0..config.frames {
        let frame = camera_provider.read_frame()?;
        summary.captured_frame_count = summary.captured_frame_count.saturating_add(1);

        let started_at = Instant::now();
        let faces = model_provider.detect(&frame)?;
        record_face_count(&mut summary, faces.len());
        let selected_face = select_largest_face(&faces);
        let (screen_replay_geometry_result, provider_summary) =
            if let Some(provider) = screen_replay_geometry_provider.as_ref() {
                let (result, summary) = provider.evaluate(&frame, selected_face)?;
                (Some(result), summary)
            } else {
                (None, ScreenReplayProviderSummary::default())
            };
        let minifasnet_result = if let Some(provider) = minifasnet_provider.as_mut() {
            let result = provider.evaluate(&frame, selected_face)?;
            summary.minifasnet_evaluated_frame_count =
                summary.minifasnet_evaluated_frame_count.saturating_add(1);
            if mini_fasnet_result_has_spoof_score(&result) {
                summary.minifasnet_model_spoof_score_frame_count = summary
                    .minifasnet_model_spoof_score_frame_count
                    .saturating_add(1);
            }
            match result.liveness_decision {
                LivenessDecision::SpoofRejected => {
                    summary.minifasnet_spoof_rejected_frame_count = summary
                        .minifasnet_spoof_rejected_frame_count
                        .saturating_add(1);
                }
                LivenessDecision::LiveAccepted => {
                    summary.minifasnet_live_accepted_frame_count = summary
                        .minifasnet_live_accepted_frame_count
                        .saturating_add(1);
                }
                LivenessDecision::Inconclusive => {
                    summary.minifasnet_inconclusive_frame_count = summary
                        .minifasnet_inconclusive_frame_count
                        .saturating_add(1);
                }
                LivenessDecision::ProviderUnavailable => {}
            }
            Some(result)
        } else {
            None
        };
        let primary_liveness_result = minifasnet_result.clone().unwrap_or(LivenessResult {
            liveness_decision: LivenessDecision::ProviderUnavailable,
            liveness_score: None,
            evidence: Vec::new(),
        });
        frame_liveness_results.push(primary_liveness_result.clone());
        let minifasnet_debug_crop_paths = if config.save_minifasnet_crops {
            if let (Some(provider), Some(face)) = (minifasnet_provider.as_ref(), selected_face) {
                let source_crop_path =
                    minifasnet_source_crops_dir.join(format!("{frame_index:05}_source.jpg"));
                let model_input_path =
                    minifasnet_model_inputs_dir.join(format!("{frame_index:05}_80x80.jpg"));
                provider.write_debug_crops(&frame, face, &source_crop_path, &model_input_path)?;
                Some(json!({
                    "source_crop_path": source_crop_path,
                    "model_input_path": model_input_path,
                }))
            } else {
                None
            }
        } else {
            None
        };
        let elapsed_ms = started_at.elapsed().as_millis();
        summary.record_elapsed(elapsed_ms);

        if let Some(observation) = &provider_summary.best_observation {
            summary.screen_rectangle_detected_frame_count = summary
                .screen_rectangle_detected_frame_count
                .saturating_add(1);
            if observation.face_inside_rectangle {
                summary.face_inside_screen_candidate_frame_count = summary
                    .face_inside_screen_candidate_frame_count
                    .saturating_add(1);
            }
        }
        match primary_liveness_result.liveness_decision {
            LivenessDecision::SpoofRejected => {
                summary.spoof_rejected_frame_count =
                    summary.spoof_rejected_frame_count.saturating_add(1);
            }
            LivenessDecision::Inconclusive => {
                summary.inconclusive_frame_count =
                    summary.inconclusive_frame_count.saturating_add(1);
            }
            LivenessDecision::LiveAccepted | LivenessDecision::ProviderUnavailable => {}
        }

        let debug_frame_path = if config.save_debug_images {
            let path = frames_dir.join(format!("{frame_index:05}.jpg"));
            write_screen_replay_debug_frame(&frame, &faces, &provider_summary, &path)?;
            Some(path)
        } else {
            None
        };

        let metric = json!({
            "frame_index": frame_index,
            "frame_width": frame.width,
            "frame_height": frame.height,
            "detected_face_count": faces.len(),
            "selected_face": selected_face,
            "screen_like_rectangle_count": provider_summary.candidate_rectangle_count,
            "best_screen_observation": provider_summary.best_observation,
            "liveness_decision": primary_liveness_result.liveness_decision,
            "liveness_score": primary_liveness_result.liveness_score,
            "evidence": primary_liveness_result.evidence,
            "screen_replay_geometry_result": screen_replay_geometry_result,
            "minifasnet_liveness_result": minifasnet_result,
            "minifasnet_debug_crop_paths": minifasnet_debug_crop_paths,
            "elapsed_ms": elapsed_ms,
            "debug_frame_path": debug_frame_path,
        });
        writeln!(
            metrics_file,
            "{}",
            serde_json::to_string(&metric).map_err(|_| LivenessScreenDebugError::IoFailed)?
        )
        .map_err(|_| LivenessScreenDebugError::IoFailed)?;

        if config.frame_delay_ms > 0 {
            thread::sleep(Duration::from_millis(u64::from(config.frame_delay_ms)));
        }
    }

    camera_provider.close();
    model_provider.unload_models();

    summary.auth_window_spoof_rejected = matches!(
        LivenessWindowPolicy::default().decide(&frame_liveness_results),
        LivenessPolicyDecision::RejectAsSpoof
    );

    let summary_json = json!({
        "requested_frame_count": summary.requested_frame_count,
        "captured_frame_count": summary.captured_frame_count,
        "no_face_frame_count": summary.no_face_frame_count,
        "single_face_frame_count": summary.single_face_frame_count,
        "multiple_face_frame_count": summary.multiple_face_frame_count,
        "screen_rectangle_detected_frame_count": summary.screen_rectangle_detected_frame_count,
        "face_inside_screen_candidate_frame_count": summary.face_inside_screen_candidate_frame_count,
        "screen_candidate_frame_count": summary.face_inside_screen_candidate_frame_count,
        "spoof_rejected_frame_count": summary.spoof_rejected_frame_count,
        "inconclusive_frame_count": summary.inconclusive_frame_count,
        "minifasnet_evaluated_frame_count": summary.minifasnet_evaluated_frame_count,
        "minifasnet_live_accepted_frame_count": summary.minifasnet_live_accepted_frame_count,
        "minifasnet_spoof_rejected_frame_count": summary.minifasnet_spoof_rejected_frame_count,
        "minifasnet_model_spoof_score_frame_count": summary.minifasnet_model_spoof_score_frame_count,
        "minifasnet_inconclusive_frame_count": summary.minifasnet_inconclusive_frame_count,
        "auth_window_spoof_rejected": summary.auth_window_spoof_rejected,
        "liveness_primary_provider": "minifasnet",
        "screen_replay_geometry_mode": if config.screen_replay_geometry_provider_config.is_some() { "diagnostic_only" } else { "disabled" },
        "average_elapsed_ms": summary.average_elapsed_ms(),
        "max_elapsed_ms": summary.max_elapsed_ms,
        "screen_replay_geometry_provider_config": config.screen_replay_geometry_provider_config.as_ref().map(|provider_config| json!({
            "binary_threshold": provider_config.binary_threshold,
            "binary_mask_upper_threshold": provider_config.binary_mask_upper_threshold,
            "min_screen_area_ratio": provider_config.min_screen_area_ratio,
            "max_screen_area_ratio": provider_config.max_screen_area_ratio,
            "min_rectangularity_score": provider_config.min_rectangularity_score,
            "min_brightness_contrast_score": provider_config.min_brightness_contrast_score,
            "min_face_inside_screen_ratio": provider_config.min_face_inside_screen_ratio,
            "min_screen_aspect_ratio": provider_config.min_screen_aspect_ratio,
            "max_screen_aspect_ratio": provider_config.max_screen_aspect_ratio,
        })),
        "minifasnet_enabled": summary.minifasnet_evaluated_frame_count > 0,
        "minifasnet_crops_dir": if config.save_minifasnet_crops { Some(minifasnet_crops_dir) } else { None },
        "metrics_path": metrics_path,
        "annotated_frames_dir": if config.save_debug_images { Some(frames_dir) } else { None },
    });
    fs::write(
        &summary_path,
        serde_json::to_vec_pretty(&summary_json).map_err(|_| LivenessScreenDebugError::IoFailed)?,
    )
    .map_err(|_| LivenessScreenDebugError::IoFailed)?;

    print_liveness_summary(&summary, &camera_id, &metrics_path, &summary_path);

    Ok(())
}

fn print_liveness_summary(
    summary: &LivenessScreenDebugSummary,
    camera_id: &CameraId,
    metrics_path: &std::path::Path,
    summary_path: &std::path::Path,
) {
    println!("liveness_screen_debug_completed（活体屏幕回放检测调试完成）: true");
    println!("camera_id（摄像头 ID）: {}", camera_id.0);
    println!(
        "captured_frame_count（实际采集帧数）: {}",
        summary.captured_frame_count
    );
    println!(
        "single_face_frame_count（检测到单个人脸的帧数）: {}",
        summary.single_face_frame_count
    );
    println!(
        "multiple_face_frame_count（检测到多个人脸的帧数）: {}",
        summary.multiple_face_frame_count
    );
    println!(
        "no_face_frame_count（没有检测到人脸的帧数）: {}",
        summary.no_face_frame_count
    );
    println!(
        "spoof_rejected_frame_count（MiniFASNet 主线确认假体并拒绝的帧数）: {}",
        summary.spoof_rejected_frame_count
    );
    println!(
        "inconclusive_frame_count（MiniFASNet 主线证据不足的帧数）: {}",
        summary.inconclusive_frame_count
    );
    println!(
        "minifasnet_evaluated_frame_count（MiniFASNet 已评估帧数）: {}",
        summary.minifasnet_evaluated_frame_count
    );
    println!(
        "minifasnet_model_spoof_score_frame_count（MiniFASNet 分数达到假体阈值的帧数，仅诊断）: {}",
        summary.minifasnet_model_spoof_score_frame_count
    );
    println!(
        "minifasnet_spoof_rejected_frame_count（MiniFASNet 实际参与拒绝的帧数）: {}",
        summary.minifasnet_spoof_rejected_frame_count
    );
    println!(
        "minifasnet_live_accepted_frame_count（MiniFASNet 判定真人的帧数）: {}",
        summary.minifasnet_live_accepted_frame_count
    );
    println!(
        "auth_window_spoof_rejected（本次采样窗口是否因 MiniFASNet 假体证据而拒绝）: {}",
        summary.auth_window_spoof_rejected
    );
    if let Some(path) = summary_path
        .parent()
        .map(|parent| parent.join("minifasnet_crops"))
        .filter(|path| path.exists())
    {
        println!(
            "minifasnet_crops_dir（MiniFASNet 实际输入裁剪图目录）: {}",
            path.display()
        );
    }
    println!(
        "average_elapsed_ms（平均每帧检测耗时毫秒）: {:?}",
        summary.average_elapsed_ms()
    );
    println!(
        "frame_count_note（计数说明）: 人脸帧数三项互斥相加等于采集帧数；MiniFASNet 是当前活体主线。屏幕矩形几何检测默认不加载；仅在显式启用诊断开关时运行，且不参与拒绝。"
    );
    println!("metrics_path（逐帧明细 JSONL）: {}", metrics_path.display());
    println!("summary_path（汇总 JSON）: {}", summary_path.display());
}

fn mini_fasnet_result_has_spoof_score(result: &face_liveness::LivenessResult) -> bool {
    result.evidence.iter().any(|evidence| match evidence {
        face_liveness::LivenessEvidence::MiniFasNetPrediction { spoof_score, .. } => {
            *spoof_score >= 0.70
        }
        _ => false,
    })
}

fn record_face_count(summary: &mut LivenessScreenDebugSummary, face_count: usize) {
    match face_count {
        0 => summary.no_face_frame_count = summary.no_face_frame_count.saturating_add(1),
        1 => summary.single_face_frame_count = summary.single_face_frame_count.saturating_add(1),
        _ => {
            summary.multiple_face_frame_count = summary.multiple_face_frame_count.saturating_add(1)
        }
    }
}

fn select_largest_face(faces: &[DetectedFace]) -> Option<&DetectedFace> {
    faces.iter().max_by(|left, right| {
        let left_area = left.bounds.width * left.bounds.height;
        let right_area = right.bounds.width * right.bounds.height;
        left_area.total_cmp(&right_area)
    })
}

fn selected_camera_id(
    requested_camera_id: Option<CameraId>,
    sources: &[video_provider::CameraInfo],
) -> Result<CameraId, LivenessScreenDebugError> {
    if let Some(camera_id) = requested_camera_id {
        return Ok(camera_id);
    }

    sources
        .first()
        .map(|source| source.id.clone())
        .ok_or(LivenessScreenDebugError::Video(VideoError::CameraNotFound))
}

#[cfg(test)]
mod tests {
    use face_engine::FaceBox;

    use super::*;

    #[test]
    fn select_largest_face_prefers_largest_bounds_area() {
        let small = DetectedFace {
            bounds: FaceBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            landmarks: Vec::new(),
            confidence: 0.9,
        };
        let large = DetectedFace {
            bounds: FaceBox {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            landmarks: Vec::new(),
            confidence: 0.9,
        };

        let faces = [small, large.clone()];
        let selected = select_largest_face(&faces);

        assert_eq!(selected, Some(&large));
    }
}
