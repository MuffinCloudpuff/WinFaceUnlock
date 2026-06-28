use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSnapshot {
    pub image_path: PathBuf,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ScreenSnapshotError {
    PlatformUnavailable,
    InvalidScreenSize,
    CaptureFailed,
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn capture_primary_screen_to_bmp(
    image_path: &Path,
) -> Result<ScreenSnapshot, ScreenSnapshotError> {
    use std::{fs::File, io::Write, ptr};

    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetDeviceCaps, HGDIOBJ, HORZRES,
        ReleaseDC, SRCCOPY, SelectObject, VERTRES,
    };

    let screen_dc = unsafe { GetDC(ptr::null_mut()) };
    if screen_dc.is_null() {
        return Err(ScreenSnapshotError::CaptureFailed);
    }

    let width = unsafe { GetDeviceCaps(screen_dc, HORZRES as i32) };
    let height = unsafe { GetDeviceCaps(screen_dc, VERTRES as i32) };
    if width <= 0 || height <= 0 {
        unsafe {
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
        return Err(ScreenSnapshotError::InvalidScreenSize);
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_null() {
        unsafe {
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
        return Err(ScreenSnapshotError::CaptureFailed);
    }

    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
    if bitmap.is_null() {
        unsafe {
            DeleteDC(memory_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
        return Err(ScreenSnapshotError::CaptureFailed);
    }

    let previous_object = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    let blit_succeeded =
        unsafe { BitBlt(memory_dc, 0, 0, width, height, screen_dc, 0, 0, SRCCOPY) };
    if blit_succeeded == 0 {
        unsafe {
            SelectObject(memory_dc, previous_object);
            DeleteObject(bitmap as HGDIOBJ);
            DeleteDC(memory_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
        }
        return Err(ScreenSnapshotError::CaptureFailed);
    }

    let row_stride = ((width * 3 + 3) / 4) * 4;
    let image_size = (row_stride * height) as usize;
    let mut pixels = vec![0_u8; image_size];
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB,
            biSizeImage: image_size as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default(); 1],
    };

    let scan_lines = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };

    unsafe {
        SelectObject(memory_dc, previous_object);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory_dc);
        ReleaseDC(ptr::null_mut(), screen_dc);
    }

    if scan_lines == 0 {
        return Err(ScreenSnapshotError::CaptureFailed);
    }

    let mut file = File::create(image_path).map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    let pixel_offset = 14 + std::mem::size_of::<BITMAPINFOHEADER>();
    let file_size = pixel_offset + pixels.len();

    file.write_all(&0x4D42_u16.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&(file_size as u32).to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&0_u16.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&0_u16.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&(pixel_offset as u32).to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;

    file.write_all(&bitmap_info.bmiHeader.biSize.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biWidth.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biHeight.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biPlanes.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biBitCount.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biCompression.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biSizeImage.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biXPelsPerMeter.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biYPelsPerMeter.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biClrUsed.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&bitmap_info.bmiHeader.biClrImportant.to_le_bytes())
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;
    file.write_all(&pixels)
        .map_err(|_| ScreenSnapshotError::CaptureFailed)?;

    Ok(ScreenSnapshot {
        image_path: image_path.to_path_buf(),
        width,
        height,
    })
}

#[cfg(not(windows))]
pub fn capture_primary_screen_to_bmp(
    _image_path: &Path,
) -> Result<ScreenSnapshot, ScreenSnapshotError> {
    Err(ScreenSnapshotError::PlatformUnavailable)
}

pub fn screen_snapshot_default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_snapshot_audit_defaults_to_enabled() {
        assert!(screen_snapshot_default_enabled());
    }
}
