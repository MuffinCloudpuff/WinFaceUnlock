use face_engine::FaceBox;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FaceImageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl FaceImageRect {
    pub fn from_face_box(face_box: &FaceBox) -> Self {
        Self {
            x: face_box.x,
            y: face_box.y,
            width: face_box.width,
            height: face_box.height,
        }
    }

    pub fn area(&self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }

    pub fn intersection_area(&self, other: &Self) -> f32 {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let width = (right - left).max(0.0);
        let height = (bottom - top).max(0.0);
        width * height
    }

    pub fn contains_center_of(&self, other: &Self) -> bool {
        let center_x = other.center_x();
        let center_y = other.center_y();
        center_x >= self.x
            && center_x <= self.x + self.width
            && center_y >= self.y
            && center_y <= self.y + self.height
    }

    pub fn fully_contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum LivenessDecision {
    LiveAccepted,
    SpoofRejected,
    Inconclusive,
    ProviderUnavailable,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum LivenessEvidence {
    ScreenLikeRectangleDetected {
        rectangle: FaceImageRect,
        face_inside_rectangle: bool,
        face_inside_screen_ratio: f32,
        rectangle_area_ratio: f32,
        brightness_contrast_score: f32,
        edge_rectangularity_score: f32,
    },
    MiniFasNetPrediction {
        crop_rectangle: FaceImageRect,
        live_score: f32,
        print_attack_score: f32,
        replay_attack_score: f32,
        spoof_score: f32,
    },
    NoScreenLikeRectangleDetected,
    NoFaceForMiniFasNet,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LivenessResult {
    pub liveness_decision: LivenessDecision,
    pub liveness_score: Option<f32>,
    pub evidence: Vec<LivenessEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LivenessProviderError {
    InvalidFrame,
    InferenceFailed,
    ModelLoadFailed,
    ModelNotLoaded,
    ModelPathMissing,
    UnsupportedPixelFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_area_reports_face_overlap() {
        let screen = FaceImageRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        let face = FaceImageRect {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
        };

        assert_eq!(screen.intersection_area(&face), 60.0 * 60.0);
    }

    #[test]
    fn fully_contains_requires_whole_rectangle_inside() {
        let screen = FaceImageRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        let inside = FaceImageRect {
            x: 20.0,
            y: 20.0,
            width: 40.0,
            height: 40.0,
        };
        let overlapping = FaceImageRect {
            x: 5.0,
            y: 20.0,
            width: 40.0,
            height: 40.0,
        };

        assert!(screen.fully_contains(&inside));
        assert!(!screen.fully_contains(&overlapping));
    }
}
