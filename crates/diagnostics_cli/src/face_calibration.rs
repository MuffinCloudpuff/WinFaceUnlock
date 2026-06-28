use std::{
    fmt,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use face_auth::RecognitionTemplates;
use face_engine::{
    FaceEngineError, FaceModelProvider, FaceTemplateMatcher, HybridFaceModelConfig,
    HybridFaceModelProvider,
};
use serde_json::json;
use video_provider::{
    CameraId, OpenCvCameraProvider, OpenCvCameraProviderConfig, VideoError, VideoFrameProvider,
};

#[derive(Clone, Debug)]
pub struct FaceCalibrationConfig {
    pub output_dir: PathBuf,
    pub scenario: String,
    pub start_delay_seconds: u32,
    pub camera_id: Option<CameraId>,
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
    pub frames: u32,
    pub frame_delay_ms: u32,
    pub model_config: HybridFaceModelConfig,
    pub templates: RecognitionTemplates,
    pub threshold_min: f32,
    pub threshold_max: f32,
    pub threshold_step: f32,
    pub required_consecutive_match_count: u32,
}

#[derive(Debug)]
pub enum FaceCalibrationError {
    IoFailed,
    InvalidThresholdRange,
    EmptyTemplateSet,
    Video(VideoError),
    Face(FaceEngineError),
}

impl fmt::Display for FaceCalibrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoFailed => write!(formatter, "I/O operation failed"),
            Self::InvalidThresholdRange => write!(formatter, "invalid threshold range"),
            Self::EmptyTemplateSet => write!(formatter, "template set is empty"),
            Self::Video(error) => write!(formatter, "video error: {error:?}"),
            Self::Face(error) => write!(formatter, "face engine error: {error:?}"),
        }
    }
}

impl From<VideoError> for FaceCalibrationError {
    fn from(value: VideoError) -> Self {
        Self::Video(value)
    }
}

impl From<FaceEngineError> for FaceCalibrationError {
    fn from(value: FaceEngineError) -> Self {
        Self::Face(value)
    }
}

#[derive(Clone, Debug, Default)]
struct CalibrationSummary {
    requested_frame_count: u32,
    captured_frame_count: u32,
    no_face_frame_count: u32,
    multiple_face_frame_count: u32,
    model_mismatch_frame_count: u32,
    matched_score_count: u32,
    extraction_failed_frame_count: u32,
    total_detection_and_extract_elapsed_ms: u128,
    max_detection_and_extract_elapsed_ms: u128,
}

impl CalibrationSummary {
    fn elapsed_observed(&mut self, elapsed_ms: u128) {
        self.total_detection_and_extract_elapsed_ms = self
            .total_detection_and_extract_elapsed_ms
            .saturating_add(elapsed_ms);
        self.max_detection_and_extract_elapsed_ms =
            self.max_detection_and_extract_elapsed_ms.max(elapsed_ms);
    }

    fn average_elapsed_ms(&self) -> Option<f64> {
        if self.captured_frame_count == 0 {
            return None;
        }
        Some(
            self.total_detection_and_extract_elapsed_ms as f64
                / f64::from(self.captured_frame_count),
        )
    }
}

pub fn run_face_calibration(config: FaceCalibrationConfig) -> Result<(), FaceCalibrationError> {
    if config.templates.is_empty() {
        return Err(FaceCalibrationError::EmptyTemplateSet);
    }
    let thresholds = threshold_values(
        config.threshold_min,
        config.threshold_max,
        config.threshold_step,
    )?;

    fs::create_dir_all(&config.output_dir).map_err(|_| FaceCalibrationError::IoFailed)?;
    let metrics_path = config.output_dir.join("calibration_frames.jsonl");
    let summary_path = config.output_dir.join("calibration_summary.json");
    let mut metrics_file =
        File::create(&metrics_path).map_err(|_| FaceCalibrationError::IoFailed)?;

    let mut camera_provider = OpenCvCameraProvider::new(OpenCvCameraProviderConfig {
        max_camera_index: config.max_camera_index,
        requested_frame_width: config.requested_frame_width,
        requested_frame_height: config.requested_frame_height,
        preferred_backend: None,
    });
    let sources = camera_provider.list_sources()?;
    let camera_id = selected_camera_id(config.camera_id, &sources)?;
    camera_provider.open(&camera_id)?;

    let mut model_provider = HybridFaceModelProvider::new(config.model_config);
    model_provider.load_models()?;
    let recognition_model = model_provider.recognition_model().clone();
    let matcher = FaceTemplateMatcher::new(f32::INFINITY);

    print_scenario_prompt(&config.scenario, config.start_delay_seconds);

    let mut summary = CalibrationSummary {
        requested_frame_count: config.frames,
        ..CalibrationSummary::default()
    };
    let mut scores = Vec::new();

    for frame_index in 0..config.frames {
        let frame = camera_provider.read_frame()?;
        summary.captured_frame_count = summary.captured_frame_count.saturating_add(1);

        let started_at = Instant::now();
        let faces = model_provider.detect(&frame)?;
        if faces.is_empty() {
            summary.no_face_frame_count = summary.no_face_frame_count.saturating_add(1);
            write_metric(
                &mut metrics_file,
                frame_index,
                faces.len(),
                None,
                None,
                started_at.elapsed().as_millis(),
            )?;
            sleep_frame_delay(config.frame_delay_ms);
            continue;
        }
        if faces.len() > 1 {
            summary.multiple_face_frame_count = summary.multiple_face_frame_count.saturating_add(1);
            write_metric(
                &mut metrics_file,
                frame_index,
                faces.len(),
                None,
                None,
                started_at.elapsed().as_millis(),
            )?;
            sleep_frame_delay(config.frame_delay_ms);
            continue;
        }

        let embedding = match model_provider.extract(&frame, &faces[0]) {
            Ok(embedding) => embedding,
            Err(_) => {
                summary.extraction_failed_frame_count =
                    summary.extraction_failed_frame_count.saturating_add(1);
                write_metric(
                    &mut metrics_file,
                    frame_index,
                    faces.len(),
                    None,
                    None,
                    started_at.elapsed().as_millis(),
                )?;
                sleep_frame_delay(config.frame_delay_ms);
                continue;
            }
        };

        let best_match = matcher.best_compatible_match(
            config.templates.as_slice(),
            &recognition_model,
            &embedding,
        );
        let elapsed_ms = started_at.elapsed().as_millis();
        summary.elapsed_observed(elapsed_ms);

        if let Some(best_match) = best_match {
            summary.matched_score_count = summary.matched_score_count.saturating_add(1);
            scores.push(best_match.score);
            write_metric(
                &mut metrics_file,
                frame_index,
                faces.len(),
                Some(best_match.score),
                Some(best_match.template_ref.0),
                elapsed_ms,
            )?;
        } else {
            summary.model_mismatch_frame_count =
                summary.model_mismatch_frame_count.saturating_add(1);
            write_metric(
                &mut metrics_file,
                frame_index,
                faces.len(),
                None,
                None,
                elapsed_ms,
            )?;
        }

        sleep_frame_delay(config.frame_delay_ms);
    }

    camera_provider.close();
    model_provider.unload_models();

    let scores_in_observed_order = scores;
    let sorted_scores = sorted_scores(scores_in_observed_order.clone());
    let threshold_reports = thresholds
        .iter()
        .map(|threshold| {
            let passed_frame_count = sorted_scores
                .iter()
                .filter(|score| **score >= *threshold)
                .count();
            json!({
                "threshold": threshold,
                "passed_frame_count": passed_frame_count,
                "observed_score_count": sorted_scores.len(),
                "pass_ratio": ratio(passed_frame_count, sorted_scores.len()),
                "required_consecutive_match_count": config.required_consecutive_match_count,
                "sequence_auth_would_pass": longest_consecutive_pass_count(&scores_in_observed_order, *threshold)
                    >= config.required_consecutive_match_count,
            })
        })
        .collect::<Vec<_>>();

    let summary_json = json!({
        "scenario": config.scenario,
        "camera_id": camera_id.0,
        "requested_frame_count": summary.requested_frame_count,
        "captured_frame_count": summary.captured_frame_count,
        "no_face_frame_count": summary.no_face_frame_count,
        "multiple_face_frame_count": summary.multiple_face_frame_count,
        "model_mismatch_frame_count": summary.model_mismatch_frame_count,
        "matched_score_count": summary.matched_score_count,
        "extraction_failed_frame_count": summary.extraction_failed_frame_count,
        "score_min": sorted_scores.first(),
        "score_avg": average(&sorted_scores),
        "score_max": sorted_scores.last(),
        "score_p10": percentile_sorted(&sorted_scores, 0.10),
        "score_p50": percentile_sorted(&sorted_scores, 0.50),
        "score_p90": percentile_sorted(&sorted_scores, 0.90),
        "average_detection_and_extract_elapsed_ms": summary.average_elapsed_ms(),
        "max_detection_and_extract_elapsed_ms": summary.max_detection_and_extract_elapsed_ms,
        "threshold_reports": threshold_reports,
        "metrics_path": metrics_path,
    });

    fs::write(
        &summary_path,
        serde_json::to_vec_pretty(&summary_json).map_err(|_| FaceCalibrationError::IoFailed)?,
    )
    .map_err(|_| FaceCalibrationError::IoFailed)?;

    println!("face_calibration_completed: true");
    println!("scenario: {}", config.scenario);
    println!("captured_frame_count: {}", summary.captured_frame_count);
    println!("matched_score_count: {}", summary.matched_score_count);
    println!("score_p50: {:?}", percentile_sorted(&sorted_scores, 0.50));
    println!("score_p90: {:?}", percentile_sorted(&sorted_scores, 0.90));
    println!("metrics_path: {}", metrics_path.display());
    println!("summary_path: {}", summary_path.display());

    Ok(())
}

fn write_metric(
    metrics_file: &mut File,
    frame_index: u32,
    detected_face_count: usize,
    best_score: Option<f32>,
    best_template_ref: Option<String>,
    elapsed_ms: u128,
) -> Result<(), FaceCalibrationError> {
    let metric = json!({
        "frame_index": frame_index,
        "detected_face_count": detected_face_count,
        "best_score": best_score,
        "best_template_ref": best_template_ref,
        "detection_and_extract_elapsed_ms": elapsed_ms,
    });
    writeln!(
        metrics_file,
        "{}",
        serde_json::to_string(&metric).map_err(|_| FaceCalibrationError::IoFailed)?
    )
    .map_err(|_| FaceCalibrationError::IoFailed)
}

fn selected_camera_id(
    requested_camera_id: Option<CameraId>,
    sources: &[video_provider::CameraInfo],
) -> Result<CameraId, FaceCalibrationError> {
    if let Some(camera_id) = requested_camera_id {
        return Ok(camera_id);
    }

    sources
        .first()
        .map(|source| source.id.clone())
        .ok_or(FaceCalibrationError::Video(VideoError::CameraNotFound))
}

fn sleep_frame_delay(frame_delay_ms: u32) {
    if frame_delay_ms > 0 {
        thread::sleep(Duration::from_millis(u64::from(frame_delay_ms)));
    }
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
        _ => "请保持当前姿态，系统将采集校准帧",
    }
}

fn threshold_values(
    threshold_min: f32,
    threshold_max: f32,
    threshold_step: f32,
) -> Result<Vec<f32>, FaceCalibrationError> {
    if !threshold_min.is_finite()
        || !threshold_max.is_finite()
        || !threshold_step.is_finite()
        || threshold_step <= 0.0
        || threshold_min > threshold_max
    {
        return Err(FaceCalibrationError::InvalidThresholdRange);
    }

    let mut thresholds = Vec::new();
    let mut threshold = threshold_min;
    while threshold <= threshold_max + f32::EPSILON {
        thresholds.push((threshold * 1000.0).round() / 1000.0);
        threshold += threshold_step;
    }
    Ok(thresholds)
}

fn sorted_scores(mut scores: Vec<f32>) -> Vec<f32> {
    scores.retain(|score| score.is_finite());
    scores.sort_by(|left, right| left.total_cmp(right));
    scores
}

fn average(scores: &[f32]) -> Option<f32> {
    if scores.is_empty() {
        return None;
    }
    Some(scores.iter().sum::<f32>() / scores.len() as f32)
}

fn percentile_sorted(sorted_scores: &[f32], percentile: f32) -> Option<f32> {
    if sorted_scores.is_empty() {
        return None;
    }
    if sorted_scores.len() == 1 {
        return Some(sorted_scores[0]);
    }

    let clamped_percentile = percentile.clamp(0.0, 1.0);
    let last_index = sorted_scores.len() - 1;
    let index = (last_index as f32 * clamped_percentile).round() as usize;
    sorted_scores.get(index.min(last_index)).copied()
}

fn ratio(numerator: usize, denominator: usize) -> Option<f32> {
    if denominator == 0 {
        return None;
    }
    Some(numerator as f32 / denominator as f32)
}

fn longest_consecutive_pass_count(scores: &[f32], threshold: f32) -> u32 {
    let mut longest = 0_u32;
    let mut current = 0_u32;
    for score in scores {
        if *score >= threshold {
            current = current.saturating_add(1);
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_values_include_range_end() -> Result<(), FaceCalibrationError> {
        let thresholds = threshold_values(0.4, 0.5, 0.05)?;

        assert_eq!(thresholds, vec![0.4, 0.45, 0.5]);
        Ok(())
    }

    #[test]
    fn longest_consecutive_pass_count_respects_order() {
        let scores = vec![0.7, 0.8, 0.3, 0.9, 0.91, 0.92];

        assert_eq!(longest_consecutive_pass_count(&scores, 0.75), 3);
    }

    #[test]
    fn percentile_returns_none_for_empty_scores() {
        assert_eq!(percentile_sorted(&[], 0.5), None);
    }
}
