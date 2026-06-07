use tauri::{Manager, PhysicalPosition, PhysicalSize};

#[tauri::command]
fn get_backend_status() -> serde_json::Value {
    serde_json::json!({
        "connection_state": "disconnected",
        "message": "后端未连接，前端仍可独立运行。"
    })
}

fn configure_webview2_low_memory_mode() {
    let low_memory_enabled = std::env::var("WINFACEUNLOCK_WEBVIEW2_LOW_MEMORY")
        .map(|value| value == "1")
        .unwrap_or(false);

    if low_memory_enabled && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-gpu --disable-background-networking --disable-component-update --disable-extensions --disable-sync --disable-features=Translate,AutofillServerCommunication",
        );
    }
}

fn fit_main_window_to_monitor(app: &tauri::App) -> tauri::Result<()> {
    const ASPECT_WIDTH: f64 = 4.0;
    const ASPECT_HEIGHT: f64 = 3.0;
    const TARGET_SCREEN_AREA: f64 = 0.30;
    const MAX_SCREEN_SCALE: f64 = 0.9;
    const MIN_WIDTH: f64 = 560.0;
    const MIN_HEIGHT: f64 = 420.0;

    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        window.center()?;
        return Ok(());
    };

    let work_area = monitor.work_area();
    let work_width = work_area.size.width as f64;
    let work_height = work_area.size.height as f64;
    let aspect_ratio = ASPECT_WIDTH / ASPECT_HEIGHT;

    let max_width = work_width * MAX_SCREEN_SCALE;
    let max_height = work_height * MAX_SCREEN_SCALE;
    let target_area = work_width * work_height * TARGET_SCREEN_AREA;

    let mut width = (target_area * aspect_ratio).sqrt().clamp(MIN_WIDTH, max_width);
    let mut height = (width / aspect_ratio).clamp(MIN_HEIGHT, max_height);

    if height >= max_height {
        height = max_height;
        width = (height * aspect_ratio).clamp(MIN_WIDTH, max_width);
    }

    let width = width.round() as u32;
    let height = height.round() as u32;
    let x = work_area.position.x + ((work_area.size.width.saturating_sub(width)) / 2) as i32;
    let y = work_area.position.y + ((work_area.size.height.saturating_sub(height)) / 2) as i32;

    window.set_size(PhysicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(x, y))?;
    window.set_maximizable(true)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_webview2_low_memory_mode();

    tauri::Builder::default()
        .setup(|app| {
            fit_main_window_to_monitor(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_backend_status])
        .run(tauri::generate_context!())
        .expect("error while running WinFaceUnlock control panel");
}
