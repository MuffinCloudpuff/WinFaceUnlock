use video_provider::{PixelFormat, VideoFrame};

pub struct BlackFrameFilter {
    max_frames_to_check: u32,
    checked_frames: u32,
    is_active: bool,
}

impl Default for BlackFrameFilter {
    fn default() -> Self {
        Self {
            max_frames_to_check: 30, // Default to checking up to 30 frames
            checked_frames: 0,
            is_active: true,
        }
    }
}

impl BlackFrameFilter {
    pub fn new(max_frames_to_check: u32) -> Self {
        Self {
            max_frames_to_check,
            checked_frames: 0,
            is_active: true,
        }
    }

    /// Evaluates if the frame is a black frame (e.g. from camera startup).
    /// If it returns true, the frame should be ignored.
    /// If it returns false, or if the filter is no longer active, the frame should be processed.
    pub fn is_black_frame(&mut self, frame: &VideoFrame) -> bool {
        if !self.is_active {
            return false;
        }

        self.checked_frames += 1;
        if self.checked_frames > self.max_frames_to_check {
            self.is_active = false;
            return false;
        }

        let luma_threshold = 5u32;
        let sample_count = 1000usize;

        let channels = frame.format.channel_count();
        let total_pixels = (frame.width * frame.height) as usize;

        if total_pixels == 0 {
            return true;
        }

        let step = (total_pixels / sample_count).max(1);
        let mut sum_luma = 0u64;
        let mut actual_samples = 0;

        for i in 0..sample_count {
            let pixel_idx = i * step;
            if pixel_idx >= total_pixels {
                break;
            }

            let byte_idx = pixel_idx * channels;
            if byte_idx >= frame.data.len() {
                break;
            }

            let luma = match frame.format {
                PixelFormat::Bgr8 => {
                    if byte_idx + 2 < frame.data.len() {
                        let b = frame.data[byte_idx] as u32;
                        let g = frame.data[byte_idx + 1] as u32;
                        let r = frame.data[byte_idx + 2] as u32;
                        (r * 299 + g * 587 + b * 114) / 1000
                    } else {
                        0
                    }
                }
                PixelFormat::Rgb8 => {
                    if byte_idx + 2 < frame.data.len() {
                        let r = frame.data[byte_idx] as u32;
                        let g = frame.data[byte_idx + 1] as u32;
                        let b = frame.data[byte_idx + 2] as u32;
                        (r * 299 + g * 587 + b * 114) / 1000
                    } else {
                        0
                    }
                }
                PixelFormat::Gray8 => frame.data[byte_idx] as u32,
            };

            sum_luma += luma as u64;
            actual_samples += 1;
        }

        if actual_samples == 0 {
            return true;
        }

        let avg_luma = (sum_luma / actual_samples as u64) as u32;

        if avg_luma < luma_threshold {
            true // It is a black frame
        } else {
            // Once we see a valid frame, we disable the filter.
            self.is_active = false;
            false
        }
    }
}
