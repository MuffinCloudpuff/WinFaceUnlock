#![allow(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Cursor},
    mem::size_of,
    path::{Path, PathBuf},
    process::Command,
};

use zip::ZipArchive;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::CloseHandle,
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

include!(concat!(env!("OUT_DIR"), "/embedded_bundle.rs"));

const APP_RELATIVE_PATH: &str = "app\\WinFaceUnlock.Setup.App.exe";
const BACKEND_RELATIVE_PATH: &str = "payload\\installer_cli.exe";
const PAYLOAD_MANIFEST_RELATIVE_PATH: &str = "payload\\winfaceunlock-payload.json";
const MARKER_FILE_NAME: &str = ".winfaceunlock-setup-bundle.sha256";
const VALIDATE_ARG: &str = "--winfaceunlock-bootstrapper-validate";
const VALIDATE_OUTPUT_ENV: &str = "WINFACEUNLOCK_BOOTSTRAPPER_VALIDATE_OUTPUT";

fn main() {
    let exit_code = match run(env::args_os().skip(1).collect()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("WinFaceUnlock setup bootstrapper failed: {error}");
            1
        }
    };
    std::process::exit(exit_code);
}

fn run(args: Vec<OsString>) -> Result<i32, Box<dyn Error>> {
    if BUNDLE_ZIP.is_empty() {
        return Err("setup bundle is not embedded; run scripts\\build_setup_package.ps1".into());
    }

    if args.len() == 1 && args[0].as_os_str() == VALIDATE_ARG {
        let extraction_dir = prepare_bundle()?;
        return validate_prepared_bundle(&extraction_dir);
    }
    if relaunch_elevated_if_needed(&args)? {
        return Ok(0);
    }

    let extraction_dir = prepare_bundle()?;
    let app_path = extraction_dir.join(APP_RELATIVE_PATH);
    let app_dir = app_path
        .parent()
        .ok_or("WinUI setup app path does not have a parent directory")?;
    let mut child = Command::new(&app_path)
        .current_dir(app_dir)
        .args(args)
        .spawn()?;
    let status = child.wait()?;
    if let Err(error) = cleanup_prepared_bundle(&extraction_dir) {
        eprintln!(
            "WinFaceUnlock setup bootstrapper could not clean temporary bundle directory {}: {error}",
            extraction_dir.display()
        );
    }
    Ok(status.code().unwrap_or(1))
}

#[cfg(windows)]
fn relaunch_elevated_if_needed(args: &[OsString]) -> Result<bool, Box<dyn Error>> {
    if process_is_elevated()? {
        return Ok(false);
    }

    let current_exe = env::current_exe()?;
    let executable = wide_null(current_exe.as_os_str());
    let verb = wide_null("runas");
    let parameters = wide_null(quote_shell_execute_arguments(args));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            executable.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        return Err(
            format!("failed to relaunch setup elevated; ShellExecuteW returned {result}").into(),
        );
    }

    Ok(true)
}

#[cfg(not(windows))]
fn relaunch_elevated_if_needed(_args: &[OsString]) -> Result<bool, Box<dyn Error>> {
    Ok(false)
}

#[cfg(windows)]
fn process_is_elevated() -> Result<bool, Box<dyn Error>> {
    let mut token = std::ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(io::Error::last_os_error().into());
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned_len = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut TOKEN_ELEVATION as *mut _,
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned_len,
        )
    };
    let close_result = unsafe { CloseHandle(token) };
    if result == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if close_result == 0 {
        return Err(io::Error::last_os_error().into());
    }

    Ok(elevation.TokenIsElevated != 0)
}

#[cfg(windows)]
fn wide_null(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn quote_shell_execute_arguments(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| quote_shell_execute_argument(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_shell_execute_argument(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    if !value
        .chars()
        .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_owned();
    }

    let mut quoted = String::from("\"");
    for character in value.chars() {
        if character == '"' {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn prepare_bundle() -> Result<PathBuf, Box<dyn Error>> {
    let extraction_dir = bundle_extraction_dir()?;
    cleanup_stale_bundle_dirs(&extraction_dir);
    if !bundle_is_ready(&extraction_dir) {
        reset_bundle_dir(&extraction_dir)?;
        extract_bundle(&extraction_dir)?;
        fs::write(extraction_dir.join(MARKER_FILE_NAME), BUNDLE_SHA256)?;
    }

    Ok(extraction_dir)
}

fn validate_prepared_bundle(extraction_dir: &Path) -> Result<i32, Box<dyn Error>> {
    let app_path = extraction_dir.join(APP_RELATIVE_PATH);
    let backend_path = extraction_dir.join(BACKEND_RELATIVE_PATH);
    let payload_manifest_path = extraction_dir.join(PAYLOAD_MANIFEST_RELATIVE_PATH);
    if !app_path.is_file() {
        return Err(format!("WinUI setup app is missing: {}", app_path.display()).into());
    }
    if !backend_path.is_file() {
        return Err(format!("setup backend is missing: {}", backend_path.display()).into());
    }
    if !payload_manifest_path.is_file() {
        return Err(format!(
            "setup payload manifest is missing: {}",
            payload_manifest_path.display()
        )
        .into());
    }

    let payload_root_dir = payload_manifest_path
        .parent()
        .ok_or("payload manifest has no parent directory")?;
    let validation_report = [
        "winfaceunlock_bootstrapper_validation=succeeded".to_string(),
        format!("bundle_extraction_dir={}", extraction_dir.display()),
        format!("app_entrypoint={}", app_path.display()),
        format!("backend_exe={}", backend_path.display()),
        format!("payload_root_dir={}", payload_root_dir.display()),
        format!("payload_manifest={}", payload_manifest_path.display()),
    ]
    .join("\n");

    println!("{validation_report}");
    if let Some(output_path) = env::var_os(VALIDATE_OUTPUT_ENV) {
        fs::write(output_path, validation_report)?;
    }
    Ok(0)
}

fn bundle_extraction_dir() -> Result<PathBuf, io::Error> {
    let hash_segment = BUNDLE_SHA256.get(..12).unwrap_or(BUNDLE_SHA256);
    Ok(env::temp_dir()
        .join("WinFaceUnlockSetup")
        .join(format!("bundle-{hash_segment}-{BUNDLE_SIZE}")))
}

fn bundle_is_ready(extraction_dir: &Path) -> bool {
    let marker_path = extraction_dir.join(MARKER_FILE_NAME);
    let Ok(marker) = fs::read_to_string(marker_path) else {
        return false;
    };

    marker == BUNDLE_SHA256
        && extraction_dir.join(APP_RELATIVE_PATH).is_file()
        && extraction_dir.join(BACKEND_RELATIVE_PATH).is_file()
        && extraction_dir
            .join(PAYLOAD_MANIFEST_RELATIVE_PATH)
            .is_file()
}

fn reset_bundle_dir(extraction_dir: &Path) -> Result<(), io::Error> {
    if extraction_dir.exists() {
        fs::remove_dir_all(extraction_dir)?;
    }
    fs::create_dir_all(extraction_dir)
}

fn cleanup_prepared_bundle(extraction_dir: &Path) -> Result<(), io::Error> {
    cleanup_marked_bundle_dir(extraction_dir)
}

fn cleanup_stale_bundle_dirs(current_extraction_dir: &Path) {
    let setup_temp_root = env::temp_dir().join("WinFaceUnlockSetup");
    let Ok(entries) = fs::read_dir(setup_temp_root) else {
        return;
    };
    let current_extraction_dir = current_extraction_dir.canonicalize().ok();

    for entry in entries.flatten() {
        let path = entry.path();
        if current_extraction_dir.as_ref().is_some_and(|current| {
            path.canonicalize()
                .is_ok_and(|candidate| candidate == *current)
        }) {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("bundle-"))
        {
            let _ = remove_dir_all_with_retry(&path);
        }
    }
}

fn cleanup_marked_bundle_dir(extraction_dir: &Path) -> Result<(), io::Error> {
    let setup_temp_root = env::temp_dir().join("WinFaceUnlockSetup");
    let extraction_dir = extraction_dir.canonicalize()?;
    let setup_temp_root = setup_temp_root.canonicalize()?;
    if !extraction_dir.starts_with(&setup_temp_root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to clean a directory outside the WinFaceUnlock setup temp root",
        ));
    }

    let marker_path = extraction_dir.join(MARKER_FILE_NAME);
    let marker = fs::read_to_string(marker_path)?;
    if marker != BUNDLE_SHA256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "temporary bundle marker does not match the embedded package",
        ));
    }

    remove_dir_all_with_retry(&extraction_dir)
}

fn remove_dir_all_with_retry(path: &Path) -> Result<(), io::Error> {
    let mut last_err = io::Error::from_raw_os_error(0);
    for _ in 0..20 {
        match fs::remove_dir_all(path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if !path.exists() {
                    return Ok(());
                }
                last_err = e;
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
    Err(last_err)
}

fn extract_bundle(extraction_dir: &Path) -> Result<(), Box<dyn Error>> {
    let cursor = Cursor::new(BUNDLE_ZIP);
    let mut archive = ZipArchive::new(cursor)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative_path) = entry.enclosed_name() else {
            return Err(format!("bundle zip contains unsafe path: {}", entry.name()).into());
        };
        let output_path = extraction_dir.join(relative_path);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output_file = File::create(&output_path)?;
        io::copy(&mut entry, &mut output_file)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_TEMP_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn bundle_ready_requires_marker_and_key_files() -> Result<(), Box<dyn Error>> {
        let root = env::temp_dir().join(format!(
            "winfaceunlock-bootstrapper-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app"))?;
        fs::create_dir_all(root.join("payload"))?;
        fs::write(root.join(APP_RELATIVE_PATH), b"app")?;
        fs::write(root.join(BACKEND_RELATIVE_PATH), b"backend")?;
        fs::write(root.join(PAYLOAD_MANIFEST_RELATIVE_PATH), b"{}")?;
        fs::write(root.join(MARKER_FILE_NAME), BUNDLE_SHA256)?;

        assert!(bundle_is_ready(&root));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn bundle_ready_rejects_wrong_marker() -> Result<(), Box<dyn Error>> {
        let root = env::temp_dir().join(format!(
            "winfaceunlock-bootstrapper-test-wrong-marker-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app"))?;
        fs::create_dir_all(root.join("payload"))?;
        fs::write(root.join(APP_RELATIVE_PATH), b"app")?;
        fs::write(root.join(BACKEND_RELATIVE_PATH), b"backend")?;
        fs::write(root.join(PAYLOAD_MANIFEST_RELATIVE_PATH), b"{}")?;
        fs::write(root.join(MARKER_FILE_NAME), "wrong")?;

        assert!(!bundle_is_ready(&root));

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn cleanup_prepared_bundle_removes_only_marked_bundle_dir() -> Result<(), Box<dyn Error>> {
        let _guard = TEST_TEMP_LOCK
            .lock()
            .map_err(|_| io::Error::other("test temp lock poisoned"))?;
        let root = env::temp_dir()
            .join("WinFaceUnlockSetup")
            .join(format!("bundle-cleanup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        fs::write(root.join(MARKER_FILE_NAME), BUNDLE_SHA256)?;

        cleanup_prepared_bundle(&root)?;

        assert!(!root.exists());
        Ok(())
    }

    #[test]
    fn cleanup_prepared_bundle_rejects_wrong_marker() -> Result<(), Box<dyn Error>> {
        let _guard = TEST_TEMP_LOCK
            .lock()
            .map_err(|_| io::Error::other("test temp lock poisoned"))?;
        let root = env::temp_dir().join("WinFaceUnlockSetup").join(format!(
            "bundle-cleanup-wrong-marker-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        fs::write(root.join(MARKER_FILE_NAME), "wrong")?;

        assert!(cleanup_prepared_bundle(&root).is_err());

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn cleanup_stale_bundle_dirs_removes_other_marked_bundle_dirs() -> Result<(), Box<dyn Error>> {
        let _guard = TEST_TEMP_LOCK
            .lock()
            .map_err(|_| io::Error::other("test temp lock poisoned"))?;
        let setup_temp_root = env::temp_dir().join("WinFaceUnlockSetup");
        let stale = setup_temp_root.join(format!("bundle-stale-test-{}", std::process::id()));
        let current = setup_temp_root.join(format!("bundle-current-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&stale);
        let _ = fs::remove_dir_all(&current);
        fs::create_dir_all(&stale)?;
        fs::create_dir_all(&current)?;
        fs::write(current.join(MARKER_FILE_NAME), BUNDLE_SHA256)?;

        cleanup_stale_bundle_dirs(&current);

        assert!(!stale.exists());
        assert!(current.exists());

        let _ = fs::remove_dir_all(current);
        Ok(())
    }
}
