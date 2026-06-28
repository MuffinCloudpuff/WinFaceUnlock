use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    if let Some(target_profile_dir) = target_profile_dir() {
        copy_vcpkg_runtime_dlls(&target_profile_dir);
        copy_vcpkg_runtime_dlls(&target_profile_dir.join("deps"));
    }
}

fn target_profile_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR")?);
    out_dir.ancestors().nth(3).map(Path::to_path_buf)
}

fn copy_vcpkg_runtime_dlls(target_dir: &Path) {
    let Some(workspace_dir) = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|manifest_dir| manifest_dir.parent()?.parent().map(Path::to_path_buf))
    else {
        return;
    };
    let runtime_dir = workspace_dir
        .join("vcpkg_installed")
        .join("x64-windows")
        .join("bin");
    let Ok(entries) = fs::read_dir(runtime_dir) else {
        return;
    };
    let _ = fs::create_dir_all(target_dir);

    for entry in entries.flatten() {
        let source_path = entry.path();
        if source_path.extension().and_then(|ext| ext.to_str()) != Some("dll") {
            continue;
        }
        let Some(file_name) = source_path.file_name() else {
            continue;
        };
        let _ = fs::copy(&source_path, target_dir.join(file_name));
    }
}
