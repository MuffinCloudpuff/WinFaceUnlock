use std::{fmt, path::PathBuf};

use opencv::{
    core::{self, AlgorithmHint, Mat, MatSizeTraitConst, MatTraitConst, Scalar, Size},
    dnn::{self, Net, NetTrait},
    imgproc,
    prelude::MatTraitConstManual,
};
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::{Shape, Tensor},
};
use video_provider::{PixelFormat, VideoFrame};

#[derive(Clone, Debug, PartialEq)]
pub enum OpenCvDnnPersonModelFormat {
    CaffeSsd,
    OnnxSsd,
    Yolov8Onnx,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCvDnnPersonDetectorConfig {
    pub model_format: OpenCvDnnPersonModelFormat,
    pub model_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub confidence_threshold: f32,
    pub input_width: i32,
    pub input_height: i32,
    pub scale_factor: f64,
    pub mean_bgr: [f64; 3],
    pub swap_rb: bool,
    pub person_class_id: i32,
    pub nms_threshold: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrtYoloV8PersonDetectorConfig {
    pub model_path: PathBuf,
    pub confidence_threshold: f32,
    pub input_width: usize,
    pub input_height: usize,
    pub person_class_id: i32,
    pub nms_threshold: f32,
    pub intra_threads: usize,
}

impl OrtYoloV8PersonDetectorConfig {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            confidence_threshold: 0.50,
            input_width: 640,
            input_height: 640,
            person_class_id: 0,
            nms_threshold: 0.45,
            intra_threads: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PersonDetectorConfig {
    OpenCvDnn(OpenCvDnnPersonDetectorConfig),
    OrtYoloV8(OrtYoloV8PersonDetectorConfig),
}

impl OpenCvDnnPersonDetectorConfig {
    pub fn mobilenet_ssd(model_path: PathBuf, config_path: PathBuf) -> Self {
        Self {
            model_format: OpenCvDnnPersonModelFormat::CaffeSsd,
            model_path,
            config_path: Some(config_path),
            confidence_threshold: 0.50,
            input_width: 300,
            input_height: 300,
            scale_factor: 0.007_843,
            mean_bgr: [127.5, 127.5, 127.5],
            swap_rb: false,
            person_class_id: 15,
            nms_threshold: 0.45,
        }
    }

    pub fn ssdlite_onnx(model_path: PathBuf) -> Self {
        Self {
            model_format: OpenCvDnnPersonModelFormat::OnnxSsd,
            model_path,
            config_path: None,
            confidence_threshold: 0.50,
            input_width: 320,
            input_height: 320,
            scale_factor: 1.0 / 255.0,
            mean_bgr: [0.0, 0.0, 0.0],
            swap_rb: true,
            person_class_id: 1,
            nms_threshold: 0.45,
        }
    }

    pub fn yolov8_onnx(model_path: PathBuf) -> Self {
        Self {
            model_format: OpenCvDnnPersonModelFormat::Yolov8Onnx,
            model_path,
            config_path: None,
            confidence_threshold: 0.50,
            input_width: 640,
            input_height: 640,
            scale_factor: 1.0 / 255.0,
            mean_bgr: [0.0, 0.0, 0.0],
            swap_rb: true,
            person_class_id: 0,
            nms_threshold: 0.45,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonBoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedPersonBoundingBox {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonDetection {
    pub confidence: f32,
    pub bbox: PersonBoundingBox,
    pub normalized_bbox: NormalizedPersonBoundingBox,
}

pub trait PresenceDetector {
    fn load_model(&mut self) -> Result<(), PresencePersonDetectorError>;
    fn unload_model(&mut self);
    fn detect_persons(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Vec<PersonDetection>, PresencePersonDetectorError>;
}

pub enum PersonDetector {
    OpenCvDnn(OpenCvDnnPersonDetector),
    OrtYoloV8(OrtYoloV8PersonDetector),
}

impl PersonDetector {
    pub fn new(config: PersonDetectorConfig) -> Self {
        match config {
            PersonDetectorConfig::OpenCvDnn(config) => {
                Self::OpenCvDnn(OpenCvDnnPersonDetector::new(config))
            }
            PersonDetectorConfig::OrtYoloV8(config) => {
                Self::OrtYoloV8(OrtYoloV8PersonDetector::new(config))
            }
        }
    }
}

impl PresenceDetector for PersonDetector {
    fn load_model(&mut self) -> Result<(), PresencePersonDetectorError> {
        match self {
            Self::OpenCvDnn(detector) => detector.load_model(),
            Self::OrtYoloV8(detector) => detector.load_model(),
        }
    }

    fn unload_model(&mut self) {
        match self {
            Self::OpenCvDnn(detector) => detector.unload_model(),
            Self::OrtYoloV8(detector) => detector.unload_model(),
        }
    }

    fn detect_persons(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
        match self {
            Self::OpenCvDnn(detector) => detector.detect_persons(frame),
            Self::OrtYoloV8(detector) => detector.detect_persons(frame),
        }
    }
}

pub struct OpenCvDnnPersonDetector {
    config: OpenCvDnnPersonDetectorConfig,
    net: Option<Net>,
}

pub struct OrtYoloV8PersonDetector {
    config: OrtYoloV8PersonDetectorConfig,
    session: Option<Session>,
}

impl OrtYoloV8PersonDetector {
    pub fn new(config: OrtYoloV8PersonDetectorConfig) -> Self {
        Self {
            config,
            session: None,
        }
    }

    fn session_mut(&mut self) -> Result<&mut Session, PresencePersonDetectorError> {
        self.session
            .as_mut()
            .ok_or(PresencePersonDetectorError::ModelNotLoaded)
    }
}

impl OpenCvDnnPersonDetector {
    pub fn new(config: OpenCvDnnPersonDetectorConfig) -> Self {
        Self { config, net: None }
    }

    fn net_mut(&mut self) -> Result<&mut Net, PresencePersonDetectorError> {
        self.net
            .as_mut()
            .ok_or(PresencePersonDetectorError::ModelNotLoaded)
    }
}

impl PresenceDetector for OpenCvDnnPersonDetector {
    fn load_model(&mut self) -> Result<(), PresencePersonDetectorError> {
        if !self.config.model_path.exists() {
            return Err(PresencePersonDetectorError::ModelPathMissing);
        }

        let model_path = self.config.model_path.to_string_lossy();
        let net = match self.config.model_format {
            OpenCvDnnPersonModelFormat::CaffeSsd => {
                let config_path = self
                    .config
                    .config_path
                    .as_ref()
                    .ok_or(PresencePersonDetectorError::ModelConfigPathMissing)?;
                if !config_path.exists() {
                    return Err(PresencePersonDetectorError::ModelConfigPathMissing);
                }
                let config_path = config_path.to_string_lossy();
                dnn::read_net_from_caffe(&config_path, &model_path)
                    .map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?
            }
            OpenCvDnnPersonModelFormat::OnnxSsd | OpenCvDnnPersonModelFormat::Yolov8Onnx => {
                dnn::read_net_from_onnx(&model_path)
                    .map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?
            }
        };
        self.net = Some(net);
        Ok(())
    }

    fn unload_model(&mut self) {
        self.net = None;
    }

    fn detect_persons(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
        frame
            .validate()
            .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
        let image = video_frame_to_mat(frame)?;
        let blob = dnn::blob_from_image(
            &image,
            self.config.scale_factor,
            Size::new(self.config.input_width, self.config.input_height),
            Scalar::new(
                self.config.mean_bgr[0],
                self.config.mean_bgr[1],
                self.config.mean_bgr[2],
                0.0,
            ),
            self.config.swap_rb,
            false,
            core::CV_32F,
        )
        .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;

        let net = self.net_mut()?;
        net.set_input_def(&blob)
            .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;
        let output = net
            .forward_single("")
            .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;

        match self.config.model_format {
            OpenCvDnnPersonModelFormat::CaffeSsd | OpenCvDnnPersonModelFormat::OnnxSsd => {
                parse_ssd_person_detections(
                    &output,
                    frame.width,
                    frame.height,
                    self.config.person_class_id,
                    self.config.confidence_threshold,
                )
            }
            OpenCvDnnPersonModelFormat::Yolov8Onnx => parse_yolov8_person_detections(
                &output,
                YoloFrameGeometry {
                    frame_width: frame.width,
                    frame_height: frame.height,
                    input_width: self.config.input_width,
                    input_height: self.config.input_height,
                },
                self.config.person_class_id,
                self.config.confidence_threshold,
                self.config.nms_threshold,
            ),
        }
    }
}

impl PresenceDetector for OrtYoloV8PersonDetector {
    fn load_model(&mut self) -> Result<(), PresencePersonDetectorError> {
        if !self.config.model_path.exists() {
            return Err(PresencePersonDetectorError::ModelPathMissing);
        }
        let mut builder =
            Session::builder().map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?;
        builder = builder
            .with_intra_threads(self.config.intra_threads.max(1))
            .map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?;
        let session = builder
            .commit_from_file(&self.config.model_path)
            .map_err(|_| PresencePersonDetectorError::ModelLoadFailed)?;
        self.session = Some(session);
        Ok(())
    }

    fn unload_model(&mut self) {
        self.session = None;
    }

    fn detect_persons(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
        frame
            .validate()
            .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
        let input_width = self.config.input_width;
        let input_height = self.config.input_height;
        let person_class_id = self.config.person_class_id;
        let confidence_threshold = self.config.confidence_threshold;
        let nms_threshold = self.config.nms_threshold;
        let input = yolo_frame_to_nchw_input(frame, input_width, input_height)?;
        let input_tensor = Tensor::<f32>::from_array((
            Shape::new([1, 3, input_height as i64, input_width as i64]),
            input.into_boxed_slice(),
        ))
        .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;
        let outputs = self
            .session_mut()?
            .run(ort::inputs![input_tensor])
            .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;
        if outputs.len() == 0 {
            return Err(PresencePersonDetectorError::UnsupportedOutputShape);
        }
        let output = &outputs[0];
        let (shape, values) = output
            .try_extract_tensor::<f32>()
            .map_err(|_| PresencePersonDetectorError::UnsupportedOutputShape)?;
        parse_yolov8_person_values(
            values,
            &shape
                .iter()
                .map(|dimension| *dimension as i32)
                .collect::<Vec<_>>(),
            YoloFrameGeometry {
                frame_width: frame.width,
                frame_height: frame.height,
                input_width: input_width as i32,
                input_height: input_height as i32,
            },
            person_class_id,
            confidence_threshold,
            nms_threshold,
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum PresencePersonDetectorError {
    ModelPathMissing,
    ModelConfigPathMissing,
    ModelLoadFailed,
    ModelNotLoaded,
    InvalidFrame,
    InferenceFailed,
    UnsupportedOutputShape,
}

impl fmt::Display for PresencePersonDetectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelPathMissing => write!(formatter, "person detector model path is missing"),
            Self::ModelConfigPathMissing => {
                write!(formatter, "person detector model config path is missing")
            }
            Self::ModelLoadFailed => write!(formatter, "person detector model load failed"),
            Self::ModelNotLoaded => write!(formatter, "person detector model is not loaded"),
            Self::InvalidFrame => write!(formatter, "person detector received an invalid frame"),
            Self::InferenceFailed => write!(formatter, "person detector inference failed"),
            Self::UnsupportedOutputShape => write!(formatter, "unsupported person detector output"),
        }
    }
}

impl std::error::Error for PresencePersonDetectorError {}

fn parse_ssd_person_detections(
    output: &Mat,
    frame_width: u32,
    frame_height: u32,
    person_class_id: i32,
    confidence_threshold: f32,
) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
    let values = output
        .data_typed::<f32>()
        .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;
    if values.is_empty() || values.len() % SSD_DETECTION_VALUE_COUNT != 0 {
        return Err(PresencePersonDetectorError::UnsupportedOutputShape);
    }

    let dims = output.mat_size().dims();
    if dims != 2 && dims != 4 {
        return Err(PresencePersonDetectorError::UnsupportedOutputShape);
    }

    Ok(person_detections_from_ssd_values(
        values,
        frame_width,
        frame_height,
        person_class_id,
        confidence_threshold,
    ))
}

const SSD_DETECTION_VALUE_COUNT: usize = 7;

fn person_detections_from_ssd_values(
    values: &[f32],
    frame_width: u32,
    frame_height: u32,
    person_class_id: i32,
    confidence_threshold: f32,
) -> Vec<PersonDetection> {
    values
        .chunks_exact(SSD_DETECTION_VALUE_COUNT)
        .filter_map(|detection| {
            let class_id = detection[1].round() as i32;
            let confidence = detection[2];
            if class_id != person_class_id || confidence < confidence_threshold {
                return None;
            }
            let normalized_bbox = NormalizedPersonBoundingBox {
                x_min: detection[3].clamp(0.0, 1.0),
                y_min: detection[4].clamp(0.0, 1.0),
                x_max: detection[5].clamp(0.0, 1.0),
                y_max: detection[6].clamp(0.0, 1.0),
            };
            let bbox = denormalize_bbox(normalized_bbox, frame_width, frame_height)?;
            Some(PersonDetection {
                confidence,
                bbox,
                normalized_bbox,
            })
        })
        .collect()
}

fn denormalize_bbox(
    bbox: NormalizedPersonBoundingBox,
    frame_width: u32,
    frame_height: u32,
) -> Option<PersonBoundingBox> {
    let x_min = (bbox.x_min * frame_width as f32).round() as u32;
    let y_min = (bbox.y_min * frame_height as f32).round() as u32;
    let x_max = (bbox.x_max * frame_width as f32).round() as u32;
    let y_max = (bbox.y_max * frame_height as f32).round() as u32;
    if x_max <= x_min || y_max <= y_min {
        return None;
    }

    Some(PersonBoundingBox {
        x: x_min,
        y: y_min,
        width: x_max - x_min,
        height: y_max - y_min,
    })
}

#[derive(Clone, Copy)]
struct YoloFrameGeometry {
    frame_width: u32,
    frame_height: u32,
    input_width: i32,
    input_height: i32,
}

fn parse_yolov8_person_detections(
    output: &Mat,
    geometry: YoloFrameGeometry,
    person_class_id: i32,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
    let values = output
        .data_typed::<f32>()
        .map_err(|_| PresencePersonDetectorError::InferenceFailed)?;
    let shape = mat_shape(output)?;
    parse_yolov8_person_values(
        values,
        &shape,
        geometry,
        person_class_id,
        confidence_threshold,
        nms_threshold,
    )
}

fn parse_yolov8_person_values(
    values: &[f32],
    shape: &[i32],
    geometry: YoloFrameGeometry,
    person_class_id: i32,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<Vec<PersonDetection>, PresencePersonDetectorError> {
    let layout = YoloOutputLayout::from_shape(&shape)?;
    let candidates = match layout {
        YoloOutputLayout::AttributesByAnchor {
            attribute_count,
            anchor_count,
        } => yolo_candidates_from_attribute_major_output(
            values,
            attribute_count,
            anchor_count,
            geometry,
            person_class_id,
            confidence_threshold,
        ),
        YoloOutputLayout::AnchorByAttributes {
            anchor_count,
            attribute_count,
        } => yolo_candidates_from_anchor_major_output(
            values,
            anchor_count,
            attribute_count,
            geometry,
            person_class_id,
            confidence_threshold,
        ),
    };

    Ok(non_max_suppressed_person_detections(
        candidates,
        nms_threshold,
    ))
}

fn yolo_frame_to_nchw_input(
    frame: &VideoFrame,
    input_width: usize,
    input_height: usize,
) -> Result<Vec<f32>, PresencePersonDetectorError> {
    let image = video_frame_to_mat(frame)?;
    let mut resized = Mat::default();
    imgproc::resize(
        &image,
        &mut resized,
        Size::new(input_width as i32, input_height as i32),
        0.0,
        0.0,
        imgproc::INTER_LINEAR,
    )
    .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
    let bytes = resized
        .data_typed::<u8>()
        .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
    let pixel_count = input_width * input_height;
    if bytes.len() < pixel_count * 3 {
        return Err(PresencePersonDetectorError::InvalidFrame);
    }

    let mut input = vec![0.0_f32; pixel_count * 3];
    for pixel_index in 0..pixel_count {
        let bgr_index = pixel_index * 3;
        input[pixel_index] = bytes[bgr_index + 2] as f32 / 255.0;
        input[pixel_count + pixel_index] = bytes[bgr_index + 1] as f32 / 255.0;
        input[2 * pixel_count + pixel_index] = bytes[bgr_index] as f32 / 255.0;
    }
    Ok(input)
}

fn mat_shape(mat: &Mat) -> Result<Vec<i32>, PresencePersonDetectorError> {
    let mat_size = mat.mat_size();
    let mut shape = Vec::with_capacity(mat_size.dims() as usize);
    for index in 0..mat_size.dims() {
        shape.push(
            mat_size
                .get(index)
                .map_err(|_| PresencePersonDetectorError::UnsupportedOutputShape)?,
        );
    }
    Ok(shape)
}

enum YoloOutputLayout {
    AttributesByAnchor {
        attribute_count: usize,
        anchor_count: usize,
    },
    AnchorByAttributes {
        anchor_count: usize,
        attribute_count: usize,
    },
}

impl YoloOutputLayout {
    fn from_shape(shape: &[i32]) -> Result<Self, PresencePersonDetectorError> {
        let relevant_shape = match shape {
            [1, attributes, anchors] => [*attributes, *anchors],
            [attributes, anchors] => [*attributes, *anchors],
            _ => return Err(PresencePersonDetectorError::UnsupportedOutputShape),
        };
        let first = relevant_shape[0] as usize;
        let second = relevant_shape[1] as usize;
        if first >= YOLOV8_MIN_ATTRIBUTE_COUNT && second > first {
            return Ok(Self::AttributesByAnchor {
                attribute_count: first,
                anchor_count: second,
            });
        }
        if second >= YOLOV8_MIN_ATTRIBUTE_COUNT && first > second {
            return Ok(Self::AnchorByAttributes {
                anchor_count: first,
                attribute_count: second,
            });
        }
        Err(PresencePersonDetectorError::UnsupportedOutputShape)
    }
}

const YOLOV8_MIN_ATTRIBUTE_COUNT: usize = 5;
const YOLOV8_BOX_ATTRIBUTE_COUNT: usize = 4;

fn yolo_candidates_from_attribute_major_output(
    values: &[f32],
    attribute_count: usize,
    anchor_count: usize,
    geometry: YoloFrameGeometry,
    person_class_id: i32,
    confidence_threshold: f32,
) -> Vec<PersonDetection> {
    let class_attribute_index = YOLOV8_BOX_ATTRIBUTE_COUNT + person_class_id.max(0) as usize;
    if class_attribute_index >= attribute_count || values.len() < attribute_count * anchor_count {
        return Vec::new();
    }

    (0..anchor_count)
        .filter_map(|anchor_index| {
            let confidence = values[class_attribute_index * anchor_count + anchor_index];
            if confidence < confidence_threshold {
                return None;
            }
            let center_x = values[anchor_index];
            let center_y = values[anchor_count + anchor_index];
            let width = values[2 * anchor_count + anchor_index];
            let height = values[3 * anchor_count + anchor_index];
            yolo_detection_from_model_box(center_x, center_y, width, height, confidence, geometry)
        })
        .collect()
}

fn yolo_candidates_from_anchor_major_output(
    values: &[f32],
    anchor_count: usize,
    attribute_count: usize,
    geometry: YoloFrameGeometry,
    person_class_id: i32,
    confidence_threshold: f32,
) -> Vec<PersonDetection> {
    let class_attribute_index = YOLOV8_BOX_ATTRIBUTE_COUNT + person_class_id.max(0) as usize;
    if class_attribute_index >= attribute_count || values.len() < attribute_count * anchor_count {
        return Vec::new();
    }

    values
        .chunks_exact(attribute_count)
        .filter_map(|detection| {
            let confidence = detection[class_attribute_index];
            if confidence < confidence_threshold {
                return None;
            }
            yolo_detection_from_model_box(
                detection[0],
                detection[1],
                detection[2],
                detection[3],
                confidence,
                geometry,
            )
        })
        .collect()
}

fn yolo_detection_from_model_box(
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    confidence: f32,
    geometry: YoloFrameGeometry,
) -> Option<PersonDetection> {
    let input_width = geometry.input_width.max(1) as f32;
    let input_height = geometry.input_height.max(1) as f32;
    let x_min = ((center_x - width / 2.0) / input_width).clamp(0.0, 1.0);
    let y_min = ((center_y - height / 2.0) / input_height).clamp(0.0, 1.0);
    let x_max = ((center_x + width / 2.0) / input_width).clamp(0.0, 1.0);
    let y_max = ((center_y + height / 2.0) / input_height).clamp(0.0, 1.0);
    let normalized_bbox = NormalizedPersonBoundingBox {
        x_min,
        y_min,
        x_max,
        y_max,
    };
    let bbox = denormalize_bbox(normalized_bbox, geometry.frame_width, geometry.frame_height)?;
    Some(PersonDetection {
        confidence,
        bbox,
        normalized_bbox,
    })
}

fn non_max_suppressed_person_detections(
    mut candidates: Vec<PersonDetection>,
    nms_threshold: f32,
) -> Vec<PersonDetection> {
    candidates.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
    let mut selected = Vec::new();
    for candidate in candidates {
        if selected
            .iter()
            .all(|selected_detection| person_iou(&candidate, selected_detection) <= nms_threshold)
        {
            selected.push(candidate);
        }
    }
    selected
}

fn person_iou(left: &PersonDetection, right: &PersonDetection) -> f32 {
    let left_x2 = left.bbox.x + left.bbox.width;
    let left_y2 = left.bbox.y + left.bbox.height;
    let right_x2 = right.bbox.x + right.bbox.width;
    let right_y2 = right.bbox.y + right.bbox.height;

    let intersection_x1 = left.bbox.x.max(right.bbox.x);
    let intersection_y1 = left.bbox.y.max(right.bbox.y);
    let intersection_x2 = left_x2.min(right_x2);
    let intersection_y2 = left_y2.min(right_y2);
    if intersection_x2 <= intersection_x1 || intersection_y2 <= intersection_y1 {
        return 0.0;
    }

    let intersection_area =
        (intersection_x2 - intersection_x1) as f32 * (intersection_y2 - intersection_y1) as f32;
    let left_area = left.bbox.width as f32 * left.bbox.height as f32;
    let right_area = right.bbox.width as f32 * right.bbox.height as f32;
    intersection_area / (left_area + right_area - intersection_area).max(1.0)
}

fn video_frame_to_mat(frame: &VideoFrame) -> Result<Mat, PresencePersonDetectorError> {
    let channels = match frame.format {
        PixelFormat::Bgr8 | PixelFormat::Rgb8 => 3,
        PixelFormat::Gray8 => 1,
    };
    let mat =
        Mat::from_slice(&frame.data).map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
    let mat = mat
        .reshape(channels, frame.height as i32)
        .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
    let mut mat = mat
        .try_clone()
        .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;

    if frame.format == PixelFormat::Rgb8 {
        let mut bgr = Mat::default();
        imgproc::cvt_color(
            &mat,
            &mut bgr,
            imgproc::COLOR_RGB2BGR,
            0,
            AlgorithmHint::ALGO_HINT_DEFAULT,
        )
        .map_err(|_| PresencePersonDetectorError::InvalidFrame)?;
        mat = bgr;
    }

    Ok(mat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssd_parser_keeps_person_above_threshold() {
        let values = [
            0.0, 15.0, 0.80, 0.10, 0.20, 0.60, 0.90, 0.0, 7.0, 0.95, 0.0, 0.0, 1.0, 1.0,
        ];

        let detections = person_detections_from_ssd_values(&values, 640, 480, 15, 0.50);

        assert_eq!(
            detections,
            vec![PersonDetection {
                confidence: 0.80,
                bbox: PersonBoundingBox {
                    x: 64,
                    y: 96,
                    width: 320,
                    height: 336,
                },
                normalized_bbox: NormalizedPersonBoundingBox {
                    x_min: 0.10,
                    y_min: 0.20,
                    x_max: 0.60,
                    y_max: 0.90,
                },
            }]
        );
    }

    #[test]
    fn ssd_parser_rejects_low_confidence_person() {
        let values = [0.0, 15.0, 0.49, 0.10, 0.20, 0.60, 0.90];

        let detections = person_detections_from_ssd_values(&values, 640, 480, 15, 0.50);

        assert!(detections.is_empty());
    }

    #[test]
    fn denormalize_bbox_rejects_empty_boxes() {
        let bbox = NormalizedPersonBoundingBox {
            x_min: 0.5,
            y_min: 0.2,
            x_max: 0.5,
            y_max: 0.9,
        };

        assert_eq!(denormalize_bbox(bbox, 640, 480), None);
    }

    #[test]
    fn yolov8_attribute_major_parser_keeps_person_class() -> Result<(), PresencePersonDetectorError>
    {
        let attribute_count = 5;
        let anchor_count = 2;
        let values = [
            320.0, 100.0, 240.0, 100.0, 200.0, 50.0, 300.0, 50.0, 0.90, 0.20,
        ];

        let detections = yolo_candidates_from_attribute_major_output(
            &values,
            attribute_count,
            anchor_count,
            YoloFrameGeometry {
                frame_width: 640,
                frame_height: 480,
                input_width: 640,
                input_height: 640,
            },
            0,
            0.50,
        );

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].confidence, 0.90);
        assert_eq!(detections[0].bbox.width, 200);
        Ok(())
    }

    #[test]
    fn nms_suppresses_overlapping_lower_confidence_detection() {
        let first = PersonDetection {
            confidence: 0.90,
            bbox: PersonBoundingBox {
                x: 10,
                y: 10,
                width: 100,
                height: 100,
            },
            normalized_bbox: NormalizedPersonBoundingBox {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 1.0,
                y_max: 1.0,
            },
        };
        let second = PersonDetection {
            confidence: 0.70,
            bbox: PersonBoundingBox {
                x: 12,
                y: 12,
                width: 100,
                height: 100,
            },
            normalized_bbox: first.normalized_bbox,
        };

        let detections = non_max_suppressed_person_detections(vec![second, first], 0.45);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].confidence, 0.90);
    }
}
