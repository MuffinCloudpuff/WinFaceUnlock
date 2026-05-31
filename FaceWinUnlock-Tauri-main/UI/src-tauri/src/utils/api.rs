use std::{os::windows::process::CommandExt, process::Command};

use crate::{
    modules::options::{write_to_registry, RegistryItem},
    utils::custom_result::CustomResult,
    OpenCVResource, APP_STATE, GLOBAL_TRAY, ROOT_DIR,
};
use opencv::{
    core::{Mat, MatTraitConst, Size},
    objdetect::{FaceDetectorYN, FaceRecognizerSF},
    videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager};
use tauri_plugin_log::log::{error, info, warn};
use windows::{
    core::{BSTR, HSTRING, PWSTR},
    Win32::{
        Foundation::{E_UNEXPECTED, HWND},
        Media::{
            DirectShow::ICreateDevEnum,
            MediaFoundation::{CLSID_SystemDeviceEnum, CLSID_VideoInputDeviceCategory},
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IEnumMoniker,
                StructuredStorage::IPropertyBag, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            RemoteDesktop::WTSUnRegisterSessionNotification,
            Shutdown::LockWorkStation,
            Variant::{VariantClear, VARIANT},
            WindowsProgramming::GetUserNameW,
        },
    },
};

use super::pipe::Client;

#[derive(Debug, Clone, Serialize)]
struct ValidCameraInfo {
    camera_name: String,
    capture_index: String,
    is_valid: bool,
}

// 定义摄像头后端类型枚举
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CameraBackend {
    Any,   // CAP_ANY
    DShow, // CAP_DSHOW
    MSMF,  // CAP_MSMF
    VFW,   // CAP_VFW
}

impl From<CameraBackend> for i32 {
    fn from(backend: CameraBackend) -> Self {
        match backend {
            CameraBackend::Any => videoio::CAP_ANY,
            CameraBackend::DShow => videoio::CAP_DSHOW,
            CameraBackend::MSMF => videoio::CAP_MSMF,
            CameraBackend::VFW => videoio::CAP_VFW,
        }
    }
}

// 获取当前用户名
#[tauri::command]
pub fn get_now_username() -> Result<CustomResult, CustomResult> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。
}

// 测试 WinLogon 是否加载成功
#[tauri::command]
pub fn test_win_logon(user_name: String, password: String) -> Result<CustomResult, CustomResult> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。
    return Ok(CustomResult::success(None, None));
}

// 初始化模型
#[tauri::command]
pub fn init_model() -> Result<CustomResult, CustomResult> {
    // 加载模型
    let resource_path = ROOT_DIR
        .join("resources")
        .join("face_detection_yunet_2023mar.onnx");

    // 这个不用检查文件是否存在，不存在opencv会报错
    let _ = FaceDetectorYN::create(
        resource_path.to_str().unwrap_or(""),
        "",
        Size::new(320, 320), // 初始尺寸，后面会动态更新
        0.9,
        0.3,
        5000,
        0,
        0,
    )
    .map_err(|e| CustomResult::error(Some(format!("初始化检测器模型失败: {:?}", e)), None))?;

    let resource_path = ROOT_DIR
        .join("resources")
        .join("face_recognition_sface_2021dec.onnx");
    let _ = FaceRecognizerSF::create(resource_path.to_str().unwrap_or(""), "", 0, 0)
        .map_err(|e| CustomResult::error(Some(format!("初始化识别器模型失败: {:?}", e)), None))?;

    // 加载活体检测模型
    let _ = opencv::dnn::read_net_from_onnx(ROOT_DIR.join("resources").join("face_liveness.onnx").to_str().unwrap())
            .map_err(|e| CustomResult::error(Some(format!("初始化活体检测模型失败: {:?}", e)), None))?;

    Ok(CustomResult::success(None, None))
}

// 获取windows所有摄像头
#[tauri::command]
pub fn get_camera() -> Result<CustomResult, CustomResult> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。

    Ok(CustomResult::success(None, Some(json!(valid_cameras))))
}

// 打开摄像头
#[tauri::command]
pub fn open_camera(
    backend: Option<CameraBackend>,
    camear_index: i32,
) -> Result<CustomResult, CustomResult> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。

    // 所有后端都尝试失败
    Err(CustomResult::error(
        Some("所有摄像头后端均尝试失败，请检查设备是否连接/被占用/有权限".to_string()),
        None,
    ))
}

// 关闭摄像头
#[tauri::command]
pub fn stop_camera() -> Result<CustomResult, CustomResult> {
    let mut app_state = APP_STATE
        .lock()
        .map_err(|e| CustomResult::error(Some(format!("获取app状态失败 {}", e)), None))?;
    app_state.camera = None;
    Ok(CustomResult::success(None, None))
}

// 打开指定目录用资源管理器
#[tauri::command]
pub fn open_directory(path: String) -> Result<CustomResult, CustomResult> {
    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(CustomResult::error(
            Some(format!("路径不存在 {}", path.display())),
            None,
        ));
    }

    std::process::Command::new("explorer")
        .arg(path)
        .status()
        .map_err(|e| {
            CustomResult::error(
                Some(format!(
                    "打开文件夹失败：{}<br>请手动打开文件夹：{:?}",
                    e,
                    path.to_str()
                )),
                None,
            )
        })?;

    Ok(CustomResult::success(None, None))
}

// 自启代码由 Google Gemini 3 生成
// 我写不了出来了，注册表不管用 哭**
const CREATE_NO_WINDOW: u32 = 0x08000000;
/// 通用计划任务创建函数
/// 参数说明：
/// - path: 程序相对路径（如 "Unlock.exe"）
/// - task_name: 任务名称
/// - is_server: 是否为无GUI（SYSTEM账户）模式
/// - silent: 是否静默运行
/// - run_on_system_start: 是否系统启动就运行（而非登录后），该参数为true时is_server强制为true
/// - run_immediately: 是否创建后立即运行任务
#[tauri::command]
pub fn add_scheduled_task(
    path: String,
    task_name: String,
    is_server: bool,
    silent: bool,
    run_on_system_start: bool,
    run_immediately: bool,
) -> Result<CustomResult, CustomResult> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。

    Ok(CustomResult::success(None, None))
}

// 禁用全用户自启动
#[tauri::command]
pub fn disable_scheduled_task(task_name: String) -> Result<CustomResult, CustomResult> {
    let output = Command::new("schtasks")
        .args(&["/Delete", "/TN", &task_name, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| CustomResult::error(Some(format!("执行系统命令失败: {}", e)), None))?;

    if output.status.success() {
        Ok(CustomResult::success(None, None))
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        // 如果任务本身不存在，删除会报错，这里可以根据需要判断是否视为成功
        Err(CustomResult::error(
            Some(format!("删除计划任务失败: {}", err_msg)),
            None,
        ))
    }
}

// 检查是否已开启全用户自启动
#[tauri::command]
pub fn check_scheduled_task(task_name: String) -> Result<CustomResult, CustomResult> {
    // /Query 检查任务是否存在
    let output = Command::new("schtasks")
        .args(&["/Query", "/TN", &task_name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| CustomResult::error(Some(format!("查询系统命令失败: {}", e)), None))?;

    // 如果状态码为 0，说明任务存在
    let is_enabled = output.status.success();

    Ok(CustomResult::success(
        None,
        Some(json!({"enable": is_enabled})),
    ))
}

#[tauri::command]
pub fn check_process_running() -> Result<CustomResult, CustomResult> {
    let client = Client::new(HSTRING::from(r"\\.\pipe\MansonWindowsUnlockRustUnlock"));
    if client.is_err() {
        return Err(CustomResult::error(
            Some(format!("pipe错误: {}", client.err().unwrap())),
            None,
        ));
    }

    let client = client.unwrap();
    if let Err(e) = crate::utils::pipe::write(client.handle, String::from("hello server")) {
        return Err(CustomResult::error(
            Some(format!("向客户端写入数据失败: {:?}", e)),
            None,
        ));
    }

    Ok(CustomResult::success(None, None))
}

#[tauri::command]
pub fn delete_process_running() -> Result<CustomResult, CustomResult> {
    let client = Client::new(HSTRING::from(r"\\.\pipe\MansonWindowsUnlockRustUnlock"));
    if client.is_err() {
        return Err(CustomResult::error(
            Some(format!("pipe错误: {}", client.err().unwrap())),
            None,
        ));
    }

    let client = client.unwrap();
    if let Err(e) = crate::utils::pipe::write(client.handle, String::from("exit")) {
        return Err(CustomResult::error(
            Some(format!("向客户端写入数据失败: {:?}", e)),
            None,
        ));
    }

    Ok(CustomResult::success(None, None))
}

// 检查当前服务启动状态
#[tauri::command]
pub fn check_trigger_via_xml(task_name: &str) -> Result<String, String> {
    let output = Command::new("schtasks")
        .args(&["/Query", "/TN", task_name, "/XML"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("执行系统命令失败: {}", e))?;

    let xml_content = String::from_utf8_lossy(&output.stdout);

    if xml_content.contains("<LogonTrigger>") {
        Ok("OnLogon".to_string())
    } else if xml_content.contains("<BootTrigger>") {
        Ok("OnStart".to_string())
    } else {
        Ok("Unknown".to_string())
    }
}

// 关闭软件
#[tauri::command]
pub fn close_app(app_handle: AppHandle) -> Result<CustomResult, CustomResult> {
    let window = app_handle.get_webview_window("main").unwrap();
    let hwnd = window.hwnd().unwrap();
    unsafe {
        // 注销 WTS 通知
        let _ = WTSUnRegisterSessionNotification(HWND(hwnd.0));
    }

    // 关闭系统托盘
    let mut guard = GLOBAL_TRAY
        .lock()
        .map_err(|e| CustomResult::error(Some(format!("锁定托盘全局变量失败: {}", e)), None))?;
    if let Some(tray_any) = guard.as_mut() {
        tray_any
            .set_visible(false)
            .map_err(|e| CustomResult::error(Some(format!("隐藏托盘图标失败: {}", e)), None))?;
    }

    app_handle.exit(0);

    Ok(CustomResult::success(None, None))
}
#[tauri::command]
// 加载opencv模型
pub fn load_opencv_model() -> Result<(), String> {
    // 加载模型
    let mut app_state = APP_STATE
        .lock()
        .map_err(|e| format!("获取app状态失败 {}", e))?;

    if app_state.detector.is_none() {
        let resource_path = ROOT_DIR
            .join("resources")
            .join("face_detection_yunet_2023mar.onnx");

        // 这个不用检查文件是否存在，不存在opencv会报错
        let detector = FaceDetectorYN::create(
            resource_path.to_str().unwrap_or(""),
            "",
            Size::new(320, 320), // 初始尺寸，后面会动态更新
            0.9,
            0.3,
            5000,
            0,
            0,
        )
        .map_err(|e| format!("初始化检测器模型失败: {:?}", e))?;

        app_state.detector = Some(OpenCVResource { inner: detector });
    }

    if app_state.recognizer.is_none() {
        let resource_path = ROOT_DIR
            .join("resources")
            .join("face_recognition_sface_2021dec.onnx");
        let recognizer = FaceRecognizerSF::create(resource_path.to_str().unwrap_or(""), "", 0, 0)
            .map_err(|e| format!("初始化识别器模型失败: {:?}", e))?;

        app_state.recognizer = Some(OpenCVResource { inner: recognizer });
    }

    if app_state.liveness.is_none() {
        let resource_path = ROOT_DIR
            .join("resources")
            .join("face_liveness.onnx");
        let liveness = opencv::dnn::read_net_from_onnx(resource_path.to_str().unwrap_or(""))
            .map_err(|e| format!("初始化活体检测模型失败: {:?}", e))?;

        app_state.liveness = Some(OpenCVResource { inner: liveness });
    }

    Ok(())
}

#[tauri::command]
// 卸载模型
pub fn unload_model() -> Result<(), String> {
    let mut app_state = APP_STATE
        .lock()
        .map_err(|e| format!("获取app状态失败 {}", e))?;

    if app_state.detector.is_some() {
        app_state.detector = None;
    }

    if app_state.recognizer.is_some() {
        app_state.recognizer = None;
    }

    if app_state.liveness.is_some() {
        app_state.liveness = None;
    }
    Ok(())
}

#[tauri::command]
// 获取uuid v4
pub fn get_uuid_v4() -> Result<String, String> {
    let uuid = uuid::Uuid::new_v4();
    Ok(uuid.to_string())
}

#[tauri::command]
// 获取软件的缓存目录
pub fn get_cache_dir() -> Result<String, String> {
    let app_data = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    let webview_data_dir = format!("{}\\facewinunlock-tauri\\EBWebView", app_data);
    Ok(webview_data_dir)
}

#[tauri::command]
// 执行计划任务
pub fn run_scheduled_task(task_name: &str) -> Result<(), String> {
    // 执行 schtasks /Run 命令
    let run_output = Command::new("schtasks")
        .args(&["/Run", "/TN", task_name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("执行任务命令失败: {}", e))?;

    if !run_output.status.success() {
        let err_msg = fix_gbk_encoding(&run_output.stderr);
        return Err(format!("任务启动失败: {}", err_msg));
    }

    Ok(())
}

/// 处理带参数的路径，确保引号只包裹可执行文件路径，参数在外部
fn quote_exe_path_with_args(exe_path: &str, args: Option<&str>) -> String {
    // 只给可执行文件路径加引号（如果有空格），参数保持在引号外
    let quoted_exe = if exe_path.contains(' ') && !exe_path.starts_with('"') {
        format!("\"{}\"", exe_path)
    } else {
        exe_path.to_string()
    };

    // 拼接参数（如果有）
    match args {
        Some(arg) => format!("{} {}", quoted_exe, arg),
        None => quoted_exe,
    }
}

fn fix_gbk_encoding(bytes: &[u8]) -> String {
    let (s, _, _) = encoding_rs::GBK.decode(bytes);
    s.trim().to_string()
}

// 使用指定后端尝试打开摄像头并验证读取帧
fn try_open_camera_with_backend(
    backend: CameraBackend,
    camear_index: i32,
) -> Result<VideoCapture, Box<dyn std::error::Error>> {
    let mut cam = VideoCapture::new(camear_index, backend.into())?;

    if !cam.is_opened()? {
        return Err(format!("后端 {:?} 打开摄像头后状态为未激活", backend).into());
    }

    // 激活摄像头
    let mut frame = Mat::default();
    let read_result = cam.read(&mut frame);

    match read_result {
        Ok(_) => {
            if frame.empty() {
                return Err(format!("后端 {:?} 读取到空帧", backend).into());
            }
        }
        Err(e) => {
            return Err(format!("后端 {:?} 读取帧失败: {}", backend, e).into());
        }
    }

    Ok(cam)
}
// 获取windows所有摄像头
fn get_windows_video_devices() -> windows::core::Result<Vec<(String, u32)>> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。

    Ok(devices)
}

// 验证摄像头有效性
fn is_camera_index_valid(index: u32) -> opencv::Result<bool> {
    let mut capture = VideoCapture::new(index as i32, opencv::videoio::CAP_ANY)?;
    let is_valid = capture.is_opened()?;

    // 立即释放资源，避免占用摄像头
    if is_valid {
        capture.release()?;
    }

    Ok(is_valid)
}

// 解锁屏幕
pub fn unlock(user_name: String, password: String) -> windows::core::Result<()> {
    // 因发现市面上有人在盗卖本项目，更有甚者改个软件名字，就当成自己软件在卖，多次举报无果。所以从2026年3月1日开始，本项目闭源。
    // 如果你对程序某一块功能感兴趣，可以提交 issues，我看到后会给你提供一些支持。

    Ok(())
}
