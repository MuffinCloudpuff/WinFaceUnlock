use crate::VideoError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Bgr8,
    Rgb8,
    Gray8,
}

impl PixelFormat {
    pub fn channel_count(&self) -> usize {
        match self {
            Self::Bgr8 | Self::Rgb8 => 3,
            Self::Gray8 => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl VideoFrame {
    pub fn validate(&self) -> Result<(), VideoError> {
        if self.width == 0 || self.height == 0 || self.data.is_empty() {
            return Err(VideoError::EmptyFrame);
        }

        let expected_len = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|pixel_count| pixel_count.checked_mul(self.format.channel_count()))
            .ok_or(VideoError::UnsupportedFormat)?;

        if self.data.len() != expected_len {
            return Err(VideoError::UnsupportedFormat);
        }

        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.validate().is_err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_sized_frame_is_empty() {
        let frame = VideoFrame {
            width: 0,
            height: 480,
            format: PixelFormat::Bgr8,
            data: vec![0; 8],
        };

        assert!(frame.is_empty());
    }

    #[test]
    fn valid_bgr_frame_has_expected_byte_count() {
        let frame = VideoFrame {
            width: 2,
            height: 2,
            format: PixelFormat::Bgr8,
            data: vec![0; 12],
        };

        assert_eq!(frame.validate(), Ok(()));
    }
}
