use std::{
    fmt, fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use setup_api::{
    InspectPayloadPayload, RequiredPayloadFile, SETUP_PAYLOAD_MANIFEST_VERSION,
    SetupPayloadManifest, StagePayloadFile, StagePayloadPayload,
};

const PROVIDER_DLL_FILE_NAME: &str = "windows_provider.dll";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagePayloadOutcome {
    pub staged_files: Vec<StagedPayloadFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedPayloadFile {
    pub file_id: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub stage_file_status: StageFileStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageFileStatus {
    Copied,
    Replaced,
    SkippedExistingIdentical,
}

impl StageFileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::Replaced => "replaced",
            Self::SkippedExistingIdentical => "skipped_existing_identical",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectPayloadOutcome {
    pub manifest_path: PathBuf,
    pub inspected_files: Vec<InspectedPayloadFile>,
    pub missing_required_payload_files: Vec<RequiredPayloadFile>,
    pub stage_payload_files: Vec<StagePayloadFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectedPayloadFile {
    pub file_id: String,
    pub source_relative_path: PathBuf,
    pub target_relative_path: PathBuf,
    pub source_path: PathBuf,
    pub required: bool,
    pub payload_file_presence_status: PayloadFilePresenceStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadFilePresenceStatus {
    Present,
    MissingRequired,
    MissingOptional,
}

impl PayloadFilePresenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::MissingRequired => "missing_required",
            Self::MissingOptional => "missing_optional",
        }
    }
}

pub fn inspect_payload(
    payload: &InspectPayloadPayload,
) -> Result<InspectPayloadOutcome, StagePayloadError> {
    validate_payload_root_dir(Some(&payload.payload_root_dir))?;
    validate_manifest_relative_path(&payload.manifest_relative_path)?;
    let manifest_path = payload
        .payload_root_dir
        .join(&payload.manifest_relative_path);
    if !manifest_path.is_file() {
        return Err(StagePayloadError::MissingPayloadManifest(manifest_path));
    }

    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest = serde_json::from_slice::<SetupPayloadManifest>(&manifest_bytes)
        .map_err(StagePayloadError::InvalidManifestJson)?;
    if manifest.manifest_version != SETUP_PAYLOAD_MANIFEST_VERSION {
        return Err(StagePayloadError::UnsupportedManifestVersion(
            manifest.manifest_version,
        ));
    }
    if manifest.payload_files.is_empty() {
        return Err(StagePayloadError::NoPayloadFiles);
    }

    let mut inspected_files = Vec::with_capacity(manifest.payload_files.len());
    let mut missing_required_payload_files = Vec::new();
    let mut stage_payload_files = Vec::new();

    for manifest_file in manifest.payload_files {
        validate_file_id(&manifest_file.file_id)?;
        validate_source_relative_path(&manifest_file.source_relative_path)?;
        let target_relative_path = manifest_file
            .target_relative_path
            .unwrap_or_else(|| manifest_file.source_relative_path.clone());
        validate_target_relative_path(&target_relative_path)?;

        let source_path = payload
            .payload_root_dir
            .join(&manifest_file.source_relative_path);
        let payload_file_presence_status = if source_path.is_file() {
            stage_payload_files.push(StagePayloadFile {
                file_id: manifest_file.file_id.clone(),
                source_path: manifest_file.source_relative_path.clone(),
                target_relative_path: target_relative_path.clone(),
            });
            PayloadFilePresenceStatus::Present
        } else if manifest_file.required {
            missing_required_payload_files.push(RequiredPayloadFile {
                file_id: manifest_file.file_id.clone(),
                path: source_path.clone(),
            });
            PayloadFilePresenceStatus::MissingRequired
        } else {
            PayloadFilePresenceStatus::MissingOptional
        };

        inspected_files.push(InspectedPayloadFile {
            file_id: manifest_file.file_id,
            source_relative_path: manifest_file.source_relative_path,
            target_relative_path,
            source_path,
            required: manifest_file.required,
            payload_file_presence_status,
        });
    }

    Ok(InspectPayloadOutcome {
        manifest_path,
        inspected_files,
        missing_required_payload_files,
        stage_payload_files,
    })
}

pub fn stage_payload(
    payload: &StagePayloadPayload,
) -> Result<StagePayloadOutcome, StagePayloadError> {
    validate_install_dir(&payload.install_dir)?;
    validate_payload_root_dir(payload.payload_root_dir.as_deref())?;
    validate_payload_files_present(&payload.payload_files)?;
    fs::create_dir_all(&payload.install_dir)?;

    let mut staged_files = Vec::with_capacity(payload.payload_files.len());
    for payload_file in &payload.payload_files {
        staged_files.push(stage_payload_file(
            &payload.install_dir,
            payload.payload_root_dir.as_deref(),
            payload.overwrite_existing,
            payload_file,
        )?);
    }

    Ok(StagePayloadOutcome { staged_files })
}

pub fn required_payload_files_for_preflight(
    payload: &StagePayloadPayload,
) -> Result<Vec<RequiredPayloadFile>, StagePayloadError> {
    validate_payload_root_dir(payload.payload_root_dir.as_deref())?;
    payload
        .payload_files
        .iter()
        .map(|file| {
            Ok(RequiredPayloadFile {
                file_id: file.file_id.clone(),
                path: resolve_source_path(payload.payload_root_dir.as_deref(), &file.source_path)?,
            })
        })
        .collect()
}

fn stage_payload_file(
    install_dir: &Path,
    payload_root_dir: Option<&Path>,
    overwrite_existing: bool,
    payload_file: &StagePayloadFile,
) -> Result<StagedPayloadFile, StagePayloadError> {
    validate_file_id(&payload_file.file_id)?;
    let source_path = resolve_source_path(payload_root_dir, &payload_file.source_path)?;
    validate_source_file(&source_path)?;
    validate_target_relative_path(&payload_file.target_relative_path)?;

    let target_path = install_dir.join(&payload_file.target_relative_path);
    let target_parent = target_path.parent().ok_or_else(|| {
        StagePayloadError::InvalidTargetPath(payload_file.target_relative_path.clone())
    })?;
    fs::create_dir_all(target_parent)?;

    if target_path.exists() {
        if target_path.is_dir() {
            return Err(StagePayloadError::TargetPathIsDirectory(target_path));
        }
        if files_are_identical(&source_path, &target_path)? {
            return Ok(StagedPayloadFile {
                file_id: payload_file.file_id.clone(),
                source_path,
                target_path,
                stage_file_status: StageFileStatus::SkippedExistingIdentical,
            });
        }
        if is_provider_dll_target(&target_path) {
            return Err(StagePayloadError::ProviderDllInPlaceOverwriteBlocked(
                target_path,
            ));
        }
        if !overwrite_existing {
            return Err(StagePayloadError::TargetExistsDifferent(target_path));
        }

        copy_via_temporary_file(&source_path, &target_path)?;
        return Ok(StagedPayloadFile {
            file_id: payload_file.file_id.clone(),
            source_path,
            target_path,
            stage_file_status: StageFileStatus::Replaced,
        });
    }

    copy_via_temporary_file(&source_path, &target_path)?;
    Ok(StagedPayloadFile {
        file_id: payload_file.file_id.clone(),
        source_path,
        target_path,
        stage_file_status: StageFileStatus::Copied,
    })
}

fn validate_install_dir(install_dir: &Path) -> Result<(), StagePayloadError> {
    if install_dir.as_os_str().is_empty() || !install_dir.is_absolute() {
        return Err(StagePayloadError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    if install_dir.parent().is_none() || install_dir.file_name().is_none() {
        return Err(StagePayloadError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    Ok(())
}

fn validate_payload_files_present(
    payload_files: &[StagePayloadFile],
) -> Result<(), StagePayloadError> {
    if payload_files.is_empty() {
        Err(StagePayloadError::NoPayloadFiles)
    } else {
        Ok(())
    }
}

fn validate_payload_root_dir(payload_root_dir: Option<&Path>) -> Result<(), StagePayloadError> {
    let Some(payload_root_dir) = payload_root_dir else {
        return Ok(());
    };
    if payload_root_dir.as_os_str().is_empty()
        || !payload_root_dir.is_absolute()
        || !payload_root_dir.is_dir()
    {
        return Err(StagePayloadError::InvalidPayloadRootDir(
            payload_root_dir.to_path_buf(),
        ));
    }
    Ok(())
}

fn validate_manifest_relative_path(relative_path: &Path) -> Result<(), StagePayloadError> {
    if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
        return Err(StagePayloadError::InvalidManifestRelativePath(
            relative_path.to_path_buf(),
        ));
    }
    for component in relative_path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(StagePayloadError::InvalidManifestRelativePath(
                    relative_path.to_path_buf(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_file_id(file_id: &str) -> Result<(), StagePayloadError> {
    if file_id.trim().is_empty() {
        Err(StagePayloadError::InvalidFileId)
    } else {
        Ok(())
    }
}

fn validate_source_file(source_path: &Path) -> Result<(), StagePayloadError> {
    if source_path.is_file() {
        Ok(())
    } else {
        Err(StagePayloadError::MissingSourceFile(
            source_path.to_path_buf(),
        ))
    }
}

fn resolve_source_path(
    payload_root_dir: Option<&Path>,
    source_path: &Path,
) -> Result<PathBuf, StagePayloadError> {
    if source_path.is_absolute() {
        return Ok(source_path.to_path_buf());
    }

    validate_source_relative_path(source_path)?;
    let payload_root_dir = payload_root_dir.ok_or_else(|| {
        StagePayloadError::SourceRelativePathRequiresPayloadRoot {
            source_path: source_path.to_path_buf(),
        }
    })?;
    Ok(payload_root_dir.join(source_path))
}

fn validate_source_relative_path(source_path: &Path) -> Result<(), StagePayloadError> {
    if source_path.as_os_str().is_empty() {
        return Err(StagePayloadError::InvalidSourceRelativePath(
            source_path.to_path_buf(),
        ));
    }
    for component in source_path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(StagePayloadError::InvalidSourceRelativePath(
                    source_path.to_path_buf(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_target_relative_path(target_relative_path: &Path) -> Result<(), StagePayloadError> {
    if target_relative_path.as_os_str().is_empty() || target_relative_path.is_absolute() {
        return Err(StagePayloadError::InvalidTargetPath(
            target_relative_path.to_path_buf(),
        ));
    }
    for component in target_relative_path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(StagePayloadError::InvalidTargetPath(
                    target_relative_path.to_path_buf(),
                ));
            }
        }
    }
    Ok(())
}

fn is_provider_dll_target(target_path: &Path) -> bool {
    target_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name.eq_ignore_ascii_case(PROVIDER_DLL_FILE_NAME))
}

fn files_are_identical(source_path: &Path, target_path: &Path) -> Result<bool, StagePayloadError> {
    let source_metadata = fs::metadata(source_path)?;
    let target_metadata = fs::metadata(target_path)?;
    if source_metadata.len() != target_metadata.len() {
        return Ok(false);
    }

    let mut source_file = fs::File::open(source_path)?;
    let mut target_file = fs::File::open(target_path)?;
    let mut source_buffer = [0_u8; 8192];
    let mut target_buffer = [0_u8; 8192];

    loop {
        let source_read = source_file.read(&mut source_buffer)?;
        let target_read = target_file.read(&mut target_buffer)?;
        if source_read != target_read {
            return Ok(false);
        }
        if source_read == 0 {
            return Ok(true);
        }
        if source_buffer[..source_read] != target_buffer[..target_read] {
            return Ok(false);
        }
    }
}

fn copy_via_temporary_file(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), StagePayloadError> {
    let target_parent = target_path
        .parent()
        .ok_or_else(|| StagePayloadError::InvalidTargetPath(target_path.to_path_buf()))?;
    let temporary_path =
        target_parent.join(format!(".winfaceunlock-stage-{}", unique_stage_suffix()));

    let copy_result = copy_then_rename(source_path, target_path, &temporary_path);
    if copy_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    copy_result
}

fn copy_then_rename(
    source_path: &Path,
    target_path: &Path,
    temporary_path: &Path,
) -> Result<(), StagePayloadError> {
    {
        let mut source_file = fs::File::open(source_path)
            .map_err(|error| StagePayloadError::io_context("open source", source_path, error))?;
        let mut temporary_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary_path)
            .map_err(|error| {
                StagePayloadError::io_context("create temporary", temporary_path, error)
            })?;
        std::io::copy(&mut source_file, &mut temporary_file)
            .map_err(|error| StagePayloadError::io_context("copy", target_path, error))?;
        temporary_file
            .flush()
            .map_err(|error| StagePayloadError::io_context("flush", temporary_path, error))?;
        temporary_file
            .sync_all()
            .map_err(|error| StagePayloadError::io_context("sync", temporary_path, error))?;
    }

    if target_path.exists() {
        fs::remove_file(target_path)
            .map_err(|error| StagePayloadError::io_context("remove target", target_path, error))?;
    }
    fs::rename(temporary_path, target_path)
        .map_err(|error| StagePayloadError::io_context("replace target", target_path, error))?;
    Ok(())
}

fn unique_stage_suffix() -> String {
    let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(_) => 0,
    };
    format!("{}-{timestamp}", std::process::id())
}

#[derive(Debug)]
pub enum StagePayloadError {
    InvalidInstallDir(PathBuf),
    InvalidPayloadRootDir(PathBuf),
    InvalidManifestRelativePath(PathBuf),
    MissingPayloadManifest(PathBuf),
    InvalidManifestJson(serde_json::Error),
    UnsupportedManifestVersion(u32),
    NoPayloadFiles,
    InvalidFileId,
    SourceRelativePathRequiresPayloadRoot {
        source_path: PathBuf,
    },
    InvalidSourceRelativePath(PathBuf),
    MissingSourceFile(PathBuf),
    InvalidTargetPath(PathBuf),
    TargetPathIsDirectory(PathBuf),
    TargetExistsDifferent(PathBuf),
    ProviderDllInPlaceOverwriteBlocked(PathBuf),
    IoWithPath {
        operation: &'static str,
        path: PathBuf,
        error: std::io::Error,
    },
    Io(std::io::Error),
}

impl StagePayloadError {
    fn io_context(operation: &'static str, path: &Path, error: std::io::Error) -> Self {
        Self::IoWithPath {
            operation,
            path: path.to_path_buf(),
            error,
        }
    }

    pub fn is_provider_dll_in_place_overwrite_blocked(&self) -> bool {
        matches!(self, Self::ProviderDllInPlaceOverwriteBlocked(_))
    }

    pub fn is_missing_source_file(&self) -> bool {
        matches!(
            self,
            Self::MissingSourceFile(_) | Self::MissingPayloadManifest(_)
        )
    }

    pub fn is_invalid_install_dir(&self) -> bool {
        matches!(self, Self::InvalidInstallDir(_))
    }

    pub fn is_invalid_request(&self) -> bool {
        matches!(
            self,
            Self::InvalidPayloadRootDir(_)
                | Self::InvalidManifestRelativePath(_)
                | Self::InvalidManifestJson(_)
                | Self::UnsupportedManifestVersion(_)
                | Self::InvalidFileId
                | Self::SourceRelativePathRequiresPayloadRoot { .. }
                | Self::InvalidSourceRelativePath(_)
                | Self::InvalidTargetPath(_)
        )
    }
}

impl fmt::Display for StagePayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstallDir(path) => {
                write!(formatter, "invalid install directory: {}", path.display())
            }
            Self::InvalidPayloadRootDir(path) => {
                write!(
                    formatter,
                    "invalid payload root directory: {}",
                    path.display()
                )
            }
            Self::InvalidManifestRelativePath(path) => {
                write!(
                    formatter,
                    "invalid payload manifest relative path: {}",
                    path.display()
                )
            }
            Self::MissingPayloadManifest(path) => {
                write!(
                    formatter,
                    "payload manifest does not exist: {}",
                    path.display()
                )
            }
            Self::InvalidManifestJson(error) => {
                write!(formatter, "payload manifest JSON is invalid: {error}")
            }
            Self::UnsupportedManifestVersion(version) => {
                write!(formatter, "unsupported payload manifest version: {version}")
            }
            Self::NoPayloadFiles => write!(formatter, "stage payload requires at least one file"),
            Self::InvalidFileId => write!(formatter, "payload file id must not be empty"),
            Self::SourceRelativePathRequiresPayloadRoot { source_path } => write!(
                formatter,
                "relative payload source path requires payload_root_dir: {}",
                source_path.display()
            ),
            Self::InvalidSourceRelativePath(path) => {
                write!(
                    formatter,
                    "invalid payload source relative path: {}",
                    path.display()
                )
            }
            Self::MissingSourceFile(path) => {
                write!(
                    formatter,
                    "payload source file does not exist: {}",
                    path.display()
                )
            }
            Self::InvalidTargetPath(path) => {
                write!(
                    formatter,
                    "invalid target relative path: {}",
                    path.display()
                )
            }
            Self::TargetPathIsDirectory(path) => {
                write!(
                    formatter,
                    "payload target path is a directory: {}",
                    path.display()
                )
            }
            Self::TargetExistsDifferent(path) => write!(
                formatter,
                "payload target already exists with different content: {}",
                path.display()
            ),
            Self::ProviderDllInPlaceOverwriteBlocked(path) => write!(
                formatter,
                "provider DLL in-place overwrite is blocked: {}",
                path.display()
            ),
            Self::IoWithPath {
                operation,
                path,
                error,
            } => write!(
                formatter,
                "payload staging {operation} failed for {}: {error}",
                path.display()
            ),
            Self::Io(error) => write!(formatter, "payload staging io error: {error}"),
        }
    }
}

impl std::error::Error for StagePayloadError {}

impl From<std::io::Error> for StagePayloadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_payload_copies_to_relative_target() -> Result<(), StagePayloadError> {
        let root = unique_temp_dir("copy");
        let source_dir = root.join("source");
        let install_dir = root.join("install");
        fs::create_dir_all(&source_dir)?;
        let source_path = source_dir.join("win_service.exe");
        fs::write(&source_path, b"service")?;

        let outcome = stage_payload(&StagePayloadPayload {
            install_dir: install_dir.clone(),
            payload_root_dir: None,
            overwrite_existing: false,
            payload_files: vec![StagePayloadFile {
                file_id: "win_service".to_owned(),
                source_path,
                target_relative_path: PathBuf::from("win_service.exe"),
            }],
        })?;

        assert_eq!(
            outcome.staged_files[0].stage_file_status,
            StageFileStatus::Copied
        );
        assert_eq!(fs::read(install_dir.join("win_service.exe"))?, b"service");
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn stage_payload_skips_existing_identical_file() -> Result<(), StagePayloadError> {
        let root = unique_temp_dir("identical");
        let source_dir = root.join("source");
        let install_dir = root.join("install");
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&install_dir)?;
        let source_path = source_dir.join("model.onnx");
        let target_path = install_dir.join("models").join("model.onnx");
        fs::create_dir_all(target_path.parent().unwrap_or(&install_dir))?;
        fs::write(&source_path, b"model")?;
        fs::write(&target_path, b"model")?;

        let outcome = stage_payload(&StagePayloadPayload {
            install_dir,
            payload_root_dir: None,
            overwrite_existing: false,
            payload_files: vec![StagePayloadFile {
                file_id: "model".to_owned(),
                source_path,
                target_relative_path: PathBuf::from(r"models\model.onnx"),
            }],
        })?;

        assert_eq!(
            outcome.staged_files[0].stage_file_status,
            StageFileStatus::SkippedExistingIdentical
        );
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn stage_payload_blocks_provider_dll_in_place_overwrite() -> Result<(), StagePayloadError> {
        let root = unique_temp_dir("provider-block");
        let source_dir = root.join("source");
        let install_dir = root.join("install");
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&install_dir)?;
        let source_path = source_dir.join(PROVIDER_DLL_FILE_NAME);
        fs::write(&source_path, b"new-provider")?;
        fs::write(install_dir.join(PROVIDER_DLL_FILE_NAME), b"old-provider")?;

        let error = stage_payload(&StagePayloadPayload {
            install_dir,
            payload_root_dir: None,
            overwrite_existing: true,
            payload_files: vec![StagePayloadFile {
                file_id: "windows_provider".to_owned(),
                source_path,
                target_relative_path: PathBuf::from(PROVIDER_DLL_FILE_NAME),
            }],
        })
        .err();

        assert!(matches!(
            error,
            Some(StagePayloadError::ProviderDllInPlaceOverwriteBlocked(_))
        ));
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn io_error_message_names_target_path() {
        let error = StagePayloadError::io_context(
            "replace target",
            Path::new(r"D:\Apps\WinFaceUnlock\desktop_input_agent.exe"),
            std::io::Error::from_raw_os_error(5),
        );

        let message = error.to_string();
        assert!(message.contains("replace target"));
        assert!(message.contains(r"D:\Apps\WinFaceUnlock\desktop_input_agent.exe"));
    }

    #[test]
    fn target_relative_path_rejects_parent_traversal() {
        let result = validate_target_relative_path(Path::new(r"..\windows_provider.dll"));

        assert!(matches!(
            result,
            Err(StagePayloadError::InvalidTargetPath(_))
        ));
    }

    #[test]
    fn inspect_payload_reads_manifest_and_omits_missing_optional_file()
    -> Result<(), StagePayloadError> {
        let root = unique_temp_dir("inspect");
        let payload_root_dir = root.join("payload");
        fs::create_dir_all(payload_root_dir.join("models"))?;
        fs::write(payload_root_dir.join("win_service.exe"), b"service")?;
        fs::write(
            payload_root_dir.join(r"models\face_detection_yunet_2023mar.onnx"),
            b"yunet",
        )?;
        fs::write(
            payload_root_dir.join("winfaceunlock-payload.json"),
            serde_json::to_vec(&serde_json::json!({
                "manifest_version": SETUP_PAYLOAD_MANIFEST_VERSION,
                "payload_files": [
                    {
                        "file_id": "win_service",
                        "source_relative_path": "win_service.exe",
                        "target_relative_path": "win_service.exe"
                    },
                    {
                        "file_id": "yunet_model",
                        "source_relative_path": "models\\face_detection_yunet_2023mar.onnx",
                        "target_relative_path": "models\\face_detection_yunet_2023mar.onnx"
                    },
                    {
                        "file_id": "optional_yolox_model",
                        "source_relative_path": "models\\yolox_nano.onnx",
                        "target_relative_path": "models\\yolox_nano.onnx",
                        "required": false
                    }
                ]
            }))
            .map_err(StagePayloadError::InvalidManifestJson)?,
        )?;

        let outcome = inspect_payload(&InspectPayloadPayload {
            payload_root_dir: payload_root_dir.clone(),
            manifest_relative_path: PathBuf::from("winfaceunlock-payload.json"),
        })?;

        assert_eq!(outcome.inspected_files.len(), 3);
        assert_eq!(outcome.missing_required_payload_files, Vec::new());
        assert_eq!(outcome.stage_payload_files.len(), 2);
        assert_eq!(
            outcome.inspected_files[2].payload_file_presence_status,
            PayloadFilePresenceStatus::MissingOptional
        );
        assert_eq!(
            outcome.stage_payload_files[1].source_path,
            PathBuf::from(r"models\face_detection_yunet_2023mar.onnx")
        );
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn inspect_payload_reports_missing_required_files() -> Result<(), StagePayloadError> {
        let root = unique_temp_dir("inspect-missing");
        let payload_root_dir = root.join("payload");
        fs::create_dir_all(&payload_root_dir)?;
        fs::write(
            payload_root_dir.join("winfaceunlock-payload.json"),
            serde_json::to_vec(&serde_json::json!({
                "manifest_version": SETUP_PAYLOAD_MANIFEST_VERSION,
                "payload_files": [
                    {
                        "file_id": "win_service",
                        "source_relative_path": "win_service.exe"
                    }
                ]
            }))
            .map_err(StagePayloadError::InvalidManifestJson)?,
        )?;

        let outcome = inspect_payload(&InspectPayloadPayload {
            payload_root_dir: payload_root_dir.clone(),
            manifest_relative_path: PathBuf::from("winfaceunlock-payload.json"),
        })?;

        assert_eq!(
            outcome.missing_required_payload_files,
            vec![RequiredPayloadFile {
                file_id: "win_service".to_owned(),
                path: payload_root_dir.join("win_service.exe"),
            }]
        );
        assert!(outcome.stage_payload_files.is_empty());
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn inspect_payload_rejects_manifest_path_traversal() {
        let result = validate_manifest_relative_path(Path::new(r"..\payload.json"));

        assert!(matches!(
            result,
            Err(StagePayloadError::InvalidManifestRelativePath(_))
        ));
    }

    #[test]
    fn stage_payload_resolves_relative_source_under_payload_root() -> Result<(), StagePayloadError>
    {
        let root = unique_temp_dir("relative-source");
        let payload_root_dir = root.join("payload");
        let install_dir = root.join("install");
        fs::create_dir_all(payload_root_dir.join("models"))?;
        fs::write(
            payload_root_dir.join(r"models\face_detection_yunet_2023mar.onnx"),
            b"yunet",
        )?;

        let outcome = stage_payload(&StagePayloadPayload {
            install_dir: install_dir.clone(),
            payload_root_dir: Some(payload_root_dir.clone()),
            overwrite_existing: false,
            payload_files: vec![StagePayloadFile {
                file_id: "yunet_model".to_owned(),
                source_path: PathBuf::from(r"models\face_detection_yunet_2023mar.onnx"),
                target_relative_path: PathBuf::from(r"models\face_detection_yunet_2023mar.onnx"),
            }],
        })?;

        assert_eq!(
            outcome.staged_files[0].source_path,
            payload_root_dir.join(r"models\face_detection_yunet_2023mar.onnx")
        );
        assert_eq!(
            fs::read(install_dir.join(r"models\face_detection_yunet_2023mar.onnx"))?,
            b"yunet"
        );
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn relative_source_requires_payload_root() {
        let result = resolve_source_path(None, Path::new(r"models\model.onnx"));

        assert!(matches!(
            result,
            Err(StagePayloadError::SourceRelativePathRequiresPayloadRoot { .. })
        ));
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "winfaceunlock-stage-{name}-{}",
            unique_stage_suffix()
        ))
    }

    fn remove_temp_dir(path: &Path) {
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }
}
