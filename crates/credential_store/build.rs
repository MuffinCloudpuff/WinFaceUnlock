use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../vcpkg_installed/x64-windows/lib");
    println!("cargo:rerun-if-changed=../../vcpkg_installed/x64-windows/bin");
    println!("cargo:rerun-if-changed=../../vcpkg_installed/x64-windows/debug/bin");

    let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) else {
        return;
    };
    let repository_root = manifest_dir.join("..").join("..");
    let vcpkg_triplet_dir = repository_root.join("vcpkg_installed").join("x64-windows");
    let vcpkg_lib_dir = vcpkg_triplet_dir.join("lib");

    if vcpkg_lib_dir.exists() {
        println!("cargo:rustc-link-search=native={}", vcpkg_lib_dir.display());
    }

    copy_vcpkg_runtime_dlls(&vcpkg_triplet_dir);
}

fn copy_vcpkg_runtime_dlls(vcpkg_triplet_dir: &Path) {
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(profile) = std::env::var_os("PROFILE") else {
        return;
    };

    let profile = profile.to_string_lossy();
    let source_bin_dir = if profile == "debug" {
        vcpkg_triplet_dir.join("debug").join("bin")
    } else {
        vcpkg_triplet_dir.join("bin")
    };
    if !source_bin_dir.exists() {
        return;
    }

    let Some(target_profile_dir) = find_target_profile_dir(&out_dir, &profile) else {
        return;
    };
    let target_deps_dir = target_profile_dir.join("deps");
    if !target_deps_dir.exists() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(source_bin_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let source_path = entry.path();
        if source_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("dll")
        {
            continue;
        }
        if let Some(file_name) = source_path.file_name() {
            let _ = std::fs::copy(&source_path, target_deps_dir.join(file_name));
            let _ = std::fs::copy(&source_path, target_profile_dir.join(file_name));
        }
    }
}

fn find_target_profile_dir(out_dir: &Path, profile: &str) -> Option<PathBuf> {
    for ancestor in out_dir.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some(profile) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
