use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    embed_resource::compile("windows_resources.rc", embed_resource::NONE).manifest_optional()?;

    println!("cargo:rerun-if-env-changed=WINFACEUNLOCK_SETUP_BUNDLE_ZIP");

    let output_path =
        PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("embedded_bundle.rs");

    let Some(bundle_zip) = env::var_os("WINFACEUNLOCK_SETUP_BUNDLE_ZIP") else {
        write_empty_bundle(&output_path);
        return Ok(());
    };

    let bundle_zip = PathBuf::from(bundle_zip);
    println!("cargo:rerun-if-changed={}", bundle_zip.display());

    if let Err(error) = write_embedded_bundle(&output_path, &bundle_zip) {
        eprintln!("failed to embed setup bundle: {error}");
        std::process::exit(1);
    }

    Ok(())
}

fn write_empty_bundle(output_path: &Path) {
    let source = r#"
pub const BUNDLE_ZIP: &[u8] = &[];
pub const BUNDLE_SHA256: &str = "";
pub const BUNDLE_SIZE: usize = 0;
"#;
    if let Err(error) = fs::write(output_path, source) {
        eprintln!("failed to write empty embedded bundle source: {error}");
        std::process::exit(1);
    }
}

fn write_embedded_bundle(
    output_path: &Path,
    bundle_zip: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let canonical_zip = fs::canonicalize(bundle_zip)?;
    let bytes = fs::read(&canonical_zip)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let sha256 = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let include_path = canonical_zip
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let source = format!(
        r##"
pub const BUNDLE_ZIP: &[u8] = include_bytes!(r#"{include_path}"#);
pub const BUNDLE_SHA256: &str = "{sha256}";
pub const BUNDLE_SIZE: usize = {size};
"##,
        size = bytes.len()
    );
    fs::write(output_path, source)?;
    Ok(())
}
