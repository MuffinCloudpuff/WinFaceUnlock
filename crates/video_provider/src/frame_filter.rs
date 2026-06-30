use crate::frame::VideoFrame;

/// Checks if a video frame is completely black or contains only very dark noise.
/// Returns true if the frame is deemed empty/black.
pub fn is_black_or_noise(frame: &VideoFrame) -> bool {
    if frame.data.is_empty() || frame.width == 0 || frame.height == 0 {
        return true;
    }

    // Sample pixels across the frame to estimate brightness.
    // To keep it fast, we sample up to 1000 pixels evenly distributed.
    let sample_count = 1000;
    let step = (frame.data.len() / sample_count).max(1);

    let mut dark_count = 0;
    let mut total_sampled = 0;

    // We consider a byte value less than 10 to be "dark" or "noise"
    // on a typical 8-bit channel (0-255).
    const DARK_THRESHOLD: u8 = 10;

    for i in (0..frame.data.len()).step_by(step) {
        total_sampled += 1;
        if frame.data[i] < DARK_THRESHOLD {
            dark_count += 1;
        }
    }

    if total_sampled == 0 {
        return true;
    }

    // If more than 98% of the sampled pixels are extremely dark, 
    // it's highly likely a black frame (e.g. IR camera hardware initializing, or lens covered).
    let dark_ratio = dark_count as f32 / total_sampled as f32;
    dark_ratio > 0.98
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::PixelFormat;

    #[test]
    fn test_black_frame_is_detected() {
        let frame = VideoFrame {
            width: 640,
            height: 480,
            format: PixelFormat::Gray8,
            data: vec![0; 640 * 480],
        };
        assert!(is_black_or_noise(&frame));
    }

    #[test]
    fn test_normal_frame_is_not_black() {
        let mut data = vec![0; 640 * 480];
        // Fill half the frame with bright pixels
        for i in 0..(data.len() / 2) {
            data[i] = 128;
        }
        let frame = VideoFrame {
            width: 640,
            height: 480,
            format: PixelFormat::Gray8,
            data,
        };
        assert!(!is_black_or_noise(&frame));
    }
}
