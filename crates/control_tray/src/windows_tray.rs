use std::{
    ffi::c_void,
    fmt,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    ptr,
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT,
        WPARAM,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        LibraryLoader::GetModuleHandleW,
        Threading::{
            CreateMutexW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
            DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EnumWindows,
            GetCursorPos, GetMessageW, GetWindowThreadProcessId, HMENU, IMAGE_ICON,
            IsWindowVisible, LR_DEFAULTSIZE, LoadImageW, MSG, PostQuitMessage, RegisterClassW,
            SW_RESTORE, SetForegroundWindow, ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
            TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_COMMAND, WM_DESTROY,
            WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

const APP_ICON_RESOURCE_ID: usize = 1;
const TRAY_UID: u32 = 1;
const WM_TRAY: u32 = WM_USER + 42;
const MENU_OPEN: usize = 1001;
const MENU_EXIT: usize = 1002;
const CLASS_NAME: &str = "WinFaceUnlock.ControlTray.Window";
const MUTEX_NAME: &str = "Local\\WinFaceUnlock.ControlTray";

pub fn run() -> Result<(), TrayError> {
    let _single_instance = SingleInstance::acquire()?;
    let control_panel_path = installed_control_panel_path()?;
    let window = TrayWindow::create(control_panel_path)?;
    window.add_icon()?;
    window.message_loop();
    Ok(())
}

fn installed_control_panel_path() -> Result<PathBuf, TrayError> {
    let exe_path = std::env::current_exe()?;
    let install_dir = exe_path
        .parent()
        .ok_or_else(|| TrayError::InvalidInstallDir(exe_path.clone()))?;
    Ok(install_dir.join("WinFaceUnlock.exe"))
}

fn open_or_focus_control_panel(path: &Path) -> Result<(), TrayError> {
    if !path.is_file() {
        return Err(TrayError::MissingControlPanel(path.to_path_buf()));
    }
    if let Some(process_id) = find_control_panel_process(path) {
        if let Some(hwnd) = find_visible_top_level_window(process_id) {
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
        return Ok(());
    }
    let mut command = Command::new(path);
    command.creation_flags(CREATE_NO_WINDOW);
    command.spawn()?;
    Ok(())
}

fn find_control_panel_process(path: &Path) -> Option<u32> {
    let target_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let snapshot = OwnedHandle::new(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Process32FirstW(snapshot.raw, &mut entry) } != 0;
    while has_entry {
        if let Some(image_path) = query_process_image_path(entry.th32ProcessID)
            && same_windows_path(&image_path, &target_path)
        {
            return Some(entry.th32ProcessID);
        }
        has_entry = unsafe { Process32NextW(snapshot.raw, &mut entry) } != 0;
    }
    None
}

fn query_process_image_path(process_id: u32) -> Option<PathBuf> {
    if process_id == std::process::id() {
        return std::env::current_exe().ok();
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let handle = OwnedHandle::new(handle);
    let mut buffer = vec![0_u16; 32768];
    let mut len = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle.raw, 0, buffer.as_mut_ptr(), &mut len) };
    if ok == 0 || len == 0 {
        return None;
    }
    buffer.truncate(len as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

struct WindowSearch {
    process_id: u32,
    hwnd: HWND,
}

fn find_visible_top_level_window(process_id: u32) -> Option<HWND> {
    let mut search = WindowSearch {
        process_id,
        hwnd: ptr::null_mut(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_for_process),
            (&mut search as *mut WindowSearch) as LPARAM,
        );
    }
    if search.hwnd.is_null() {
        None
    } else {
        Some(search.hwnd)
    }
}

unsafe extern "system" fn enum_windows_for_process(hwnd: HWND, lparam: LPARAM) -> i32 {
    let search = unsafe { &mut *(lparam as *mut WindowSearch) };
    let mut window_process_id = 0_u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, &mut window_process_id);
    }
    if window_process_id == search.process_id && unsafe { IsWindowVisible(hwnd) } != 0 {
        search.hwnd = hwnd;
        return 0;
    }
    1
}

struct OwnedHandle {
    raw: HANDLE,
}

impl OwnedHandle {
    fn new(raw: HANDLE) -> Self {
        Self { raw }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() && self.raw != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(self.raw);
            }
        }
    }
}

struct SingleInstance {
    handle: *mut c_void,
}

impl SingleInstance {
    fn acquire() -> Result<Self, TrayError> {
        let mutex_name = to_wide_null(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(ptr::null(), 1, mutex_name.as_ptr()) };
        if handle.is_null() {
            return Err(TrayError::Win32("CreateMutexW", unsafe { GetLastError() }));
        }
        let last_error = unsafe { GetLastError() };
        if last_error == ERROR_ALREADY_EXISTS {
            return Err(TrayError::AlreadyRunning);
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

struct TrayWindow {
    hwnd: HWND,
}

impl TrayWindow {
    fn create(control_panel_path: PathBuf) -> Result<Self, TrayError> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(TrayError::Win32("GetModuleHandleW", unsafe {
                GetLastError()
            }));
        }

        let class_name = to_wide_null(CLASS_NAME);
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        let atom = unsafe { RegisterClassW(&window_class) };
        if atom == 0 {
            return Err(TrayError::Win32("RegisterClassW", unsafe {
                GetLastError()
            }));
        }

        let boxed_path = Box::new(control_panel_path);
        let boxed_path_raw = Box::into_raw(boxed_path);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                to_wide_null("WinFaceUnlock Tray").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                boxed_path_raw.cast::<c_void>(),
            )
        };
        if hwnd.is_null() {
            unsafe {
                drop(Box::from_raw(boxed_path_raw));
            }
            return Err(TrayError::Win32("CreateWindowExW", unsafe {
                GetLastError()
            }));
        }

        Ok(Self { hwnd })
    }

    fn add_icon(&self) -> Result<(), TrayError> {
        let mut data = notify_icon_data(self.hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = load_app_icon();
        set_tip(&mut data, "WinFaceUnlock");
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        if ok == 0 {
            return Err(TrayError::Win32("Shell_NotifyIconW(NIM_ADD)", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }

    fn message_loop(&self) {
        let mut message = unsafe { std::mem::zeroed::<MSG>() };
        while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

fn load_app_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        LoadImageW(
            GetModuleHandleW(ptr::null()),
            APP_ICON_RESOURCE_ID as *const u16,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )
        .cast()
    }
}

impl Drop for TrayWindow {
    fn drop(&mut self) {
        let data = notify_icon_data(self.hwnd);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_NCCREATE => {
            let create =
                lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
            let path_ptr = unsafe { (*create).lpCreateParams }.cast::<PathBuf>();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    path_ptr as isize,
                );
            }
            1
        }
        WM_TRAY if wparam as u32 == TRAY_UID => {
            match lparam as u32 {
                WM_LBUTTONUP => {
                    let _ = with_control_panel_path(hwnd, open_or_focus_control_panel);
                }
                WM_RBUTTONUP => {
                    show_menu(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match low_word(wparam) {
                MENU_OPEN => {
                    let _ = with_control_panel_path(hwnd, open_or_focus_control_panel);
                }
                MENU_EXIT => unsafe {
                    PostQuitMessage(0);
                },
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            let path_ptr = unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                )
            } as *mut PathBuf;
            if !path_ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(path_ptr));
                    windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                        hwnd,
                        windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                        0,
                    );
                }
            }
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn with_control_panel_path<T>(
    hwnd: HWND,
    action: impl FnOnce(&Path) -> Result<T, TrayError>,
) -> Result<T, TrayError> {
    let path_ptr = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        )
    } as *const PathBuf;
    if path_ptr.is_null() {
        return Err(TrayError::InvalidWindowState);
    }
    action(unsafe { &*path_ptr })
}

fn show_menu(hwnd: HWND) {
    let menu: HMENU = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    unsafe {
        let _ = AppendMenuW(menu, 0, MENU_OPEN, to_wide_null("打开控制面板").as_ptr());
        let _ = AppendMenuW(menu, 0, MENU_EXIT, to_wide_null("退出托盘").as_ptr());
        let mut point = std::mem::zeroed();
        if GetCursorPos(&mut point) != 0 {
            SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            );
        }
        DestroyMenu(menu);
    }
}

fn notify_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    data
}

fn set_tip(data: &mut NOTIFYICONDATAW, value: &str) {
    let wide = to_wide_null(value);
    let copy_len = wide.len().min(data.szTip.len());
    data.szTip[..copy_len].copy_from_slice(&wide[..copy_len]);
}

fn low_word(value: usize) -> usize {
    value & 0xffff
}

fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug)]
pub enum TrayError {
    AlreadyRunning,
    InvalidInstallDir(PathBuf),
    InvalidWindowState,
    Io(std::io::Error),
    MissingControlPanel(PathBuf),
    Win32(&'static str, u32),
}

impl fmt::Display for TrayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => write!(formatter, "tray process is already running"),
            Self::InvalidInstallDir(path) => {
                write!(formatter, "invalid tray install path: {}", path.display())
            }
            Self::InvalidWindowState => write!(formatter, "tray window state is unavailable"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::MissingControlPanel(path) => {
                write!(
                    formatter,
                    "control panel executable is missing: {}",
                    path.display()
                )
            }
            Self::Win32(operation, code) => {
                write!(formatter, "{operation} failed with Windows error {code}")
            }
        }
    }
}

impl std::error::Error for TrayError {}

impl From<std::io::Error> for TrayError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_word_extracts_command_id() {
        assert_eq!(low_word(0x1234_5678), 0x5678);
    }

    #[test]
    fn wide_string_is_null_terminated() {
        let encoded = to_wide_null("WinFaceUnlock");

        assert_eq!(encoded.last(), Some(&0));
    }
}
