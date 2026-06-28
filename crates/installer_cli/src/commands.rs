use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::{
    installation::{FullInstallPlan, FullUninstallPlan, InstallerOrchestrator},
    provider_registry::{ProviderInstallPlan, ProviderRegistry, default_provider_binary_path},
    resource_directory::ResourceDirectoryPlan,
    service_manager::{
        InstallerError, ServiceInstallPlan, ServiceLifecycleCommand, ServiceManagerFacade,
    },
    service_registry::{
        ServiceAuthRegistry, ServiceAuthRegistryConfig, ServicePresenceRegistryPatch,
    },
    user_startup::UserStartupPlan,
};
use windows_provider::{
    TILE_VISIBILITY_HIDDEN_UNTIL_READY, TILE_VISIBILITY_VISIBLE, WAKE_AUTH_SOURCE_LOCAL_CAMERA,
    WAKE_AUTH_SOURCE_MANUAL_TEST,
};

#[derive(Clone, Debug, PartialEq)]
pub enum InstallerCommand {
    Install {
        plan: Box<FullInstallPlan>,
        dry_run: bool,
    },
    Uninstall {
        plan: Box<FullUninstallPlan>,
        dry_run: bool,
    },
    Repair {
        plan: Box<FullInstallPlan>,
        dry_run: bool,
    },
    EmergencyDisable {
        dry_run: bool,
    },
    InstallService {
        service_binary_path: PathBuf,
        start_after_install: bool,
        dry_run: bool,
    },
    UninstallService {
        stop_first: bool,
        dry_run: bool,
    },
    StartService,
    StopService,
    ServiceStatus,
    RepairService {
        service_binary_path: PathBuf,
        dry_run: bool,
    },
    ConfigureServiceAuth {
        config: Box<ServiceAuthRegistryConfig>,
        dry_run: bool,
    },
    ConfigurePresenceLock {
        patch: Box<ServicePresenceRegistryPatch>,
        dry_run: bool,
    },
    ServiceAuthStatus,
    InstallProvider {
        provider_binary_path: PathBuf,
        wake_auth_source: &'static str,
        tile_visibility: &'static str,
        auto_wake_on_advise: bool,
        dry_run: bool,
    },
    UninstallProvider {
        dry_run: bool,
    },
    EmergencyDisableProvider {
        dry_run: bool,
    },
    ProviderStatus,
    Help,
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> Result<(), InstallerError> {
    let args: Vec<String> = args.into_iter().collect();
    let command = parse_command(&args)?;
    run_command(command)
}

fn run_command(command: InstallerCommand) -> Result<(), InstallerError> {
    match command {
        InstallerCommand::Install { plan, dry_run } => {
            if dry_run {
                print_full_install_plan("install dry-run", &plan);
                return Ok(());
            }
            InstallerOrchestrator::install(&plan)?;
            println!("installed WinFaceUnlock");
        }
        InstallerCommand::Uninstall { plan, dry_run } => {
            if dry_run {
                print_full_uninstall_plan("uninstall dry-run", &plan);
                return Ok(());
            }
            InstallerOrchestrator::uninstall(&plan)?;
            println!("uninstalled WinFaceUnlock");
        }
        InstallerCommand::Repair { plan, dry_run } => {
            if dry_run {
                print_full_install_plan("repair dry-run", &plan);
                return Ok(());
            }
            InstallerOrchestrator::repair(&plan)?;
            println!("repaired WinFaceUnlock");
        }
        InstallerCommand::EmergencyDisable { dry_run } => {
            if dry_run {
                println!(
                    "emergency-disable dry-run: credential provider enumeration key will be removed"
                );
                return Ok(());
            }
            InstallerOrchestrator::emergency_disable_provider()?;
            println!("emergency disabled WinFaceUnlock");
        }
        InstallerCommand::InstallService {
            service_binary_path,
            start_after_install,
            dry_run,
        } => {
            let plan = ServiceInstallPlan::new(service_binary_path);
            if dry_run {
                print_install_plan("install-service dry-run", &plan);
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect_for_installation()?;
            manager.install_service(&plan)?;
            println!("installed {}", plan.service_name.to_string_lossy());
            if start_after_install {
                manager.run_lifecycle_command(ServiceLifecycleCommand::Start)?;
                println!("started {}", plan.service_name.to_string_lossy());
            }
        }
        InstallerCommand::UninstallService {
            stop_first,
            dry_run,
        } => {
            if dry_run {
                println!("uninstall-service dry-run: service will be stopped first: {stop_first}");
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect()?;
            manager.uninstall_service(stop_first)?;
            println!("uninstalled WinFaceUnlockService");
        }
        InstallerCommand::StartService => {
            let manager = ServiceManagerFacade::connect()?;
            manager.run_lifecycle_command(ServiceLifecycleCommand::Start)?;
            println!("started WinFaceUnlockService");
        }
        InstallerCommand::StopService => {
            let manager = ServiceManagerFacade::connect()?;
            manager.run_lifecycle_command(ServiceLifecycleCommand::Stop)?;
            println!("stopped WinFaceUnlockService");
        }
        InstallerCommand::ServiceStatus => {
            let manager = ServiceManagerFacade::connect()?;
            let status = manager.query_service_status()?;
            println!("WinFaceUnlockService status: {:?}", status.current_state);
            if let Some(process_id) = status.process_id {
                println!("process_id: {process_id}");
            }
        }
        InstallerCommand::RepairService {
            service_binary_path,
            dry_run,
        } => {
            let plan = ServiceInstallPlan::new(service_binary_path);
            if dry_run {
                print_install_plan("repair-service dry-run", &plan);
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect_for_installation()?;
            manager.repair_service(&plan)?;
            println!("repaired {}", plan.service_name.to_string_lossy());
        }
        InstallerCommand::ConfigureServiceAuth { config, dry_run } => {
            if dry_run {
                print_service_auth_config("configure-service-auth dry-run", &config);
                return Ok(());
            }
            ServiceAuthRegistry::configure_local_camera(&config)?;
            println!("configured WinFaceUnlockService auth");
            print_service_auth_config("service auth config", &config);
        }
        InstallerCommand::ConfigurePresenceLock { patch, dry_run } => {
            if dry_run {
                print_presence_registry_patch("configure-presence-lock dry-run", &patch);
                return Ok(());
            }
            ServiceAuthRegistry::configure_presence_lock(&patch)?;
            println!("configured WinFaceUnlockService presence lock");
            print_presence_registry_patch("presence lock patch", &patch);
        }
        InstallerCommand::ServiceAuthStatus => {
            let status = ServiceAuthRegistry::status();
            println!(
                "service_auth_registry_config_exists: {}",
                status.registry_config_exists
            );
            println!(
                "auth_mode: {}",
                status.auth_mode.as_deref().unwrap_or("<unset>")
            );
            println!(
                "face_template_path: {}",
                status.face_template_path.as_deref().unwrap_or("<unset>")
            );
            println!(
                "camera_id: {}",
                status.camera_id.as_deref().unwrap_or("<unset>")
            );
            println!(
                "match_threshold: {}",
                status.match_threshold.as_deref().unwrap_or("<unset>")
            );
            println!(
                "minifasnet_model_path: {}",
                status.minifasnet_model_path.as_deref().unwrap_or("<unset>")
            );
            println!(
                "minifasnet_max_spoof_frame_ratio: {}",
                status
                    .minifasnet_max_spoof_frame_ratio
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_lock_enabled: {}",
                status.presence_lock_enabled.as_deref().unwrap_or("<unset>")
            );
            println!(
                "presence_owner_match_threshold: {}",
                status
                    .presence_owner_match_threshold
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_detector_kind: {}",
                status
                    .presence_detector_kind
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_tracking_mode: {}",
                status
                    .presence_tracking_mode
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_detector_fps: {}",
                status.presence_detector_fps.as_deref().unwrap_or("<unset>")
            );
            println!(
                "presence_unload_model_when_idle: {}",
                status
                    .presence_unload_model_when_idle
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_confidence_threshold: {}",
                status
                    .presence_person_confidence_threshold
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_detector_model: {}",
                status
                    .presence_person_detector_model
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_suspect_fps: {}",
                status
                    .presence_person_suspect_fps
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_absent_required_frames: {}",
                status
                    .presence_absent_required_frames
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_boundary_margin_ratio: {}",
                status
                    .presence_boundary_margin_ratio
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_movement_delta_ratio: {}",
                status
                    .presence_movement_delta_ratio
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_model_path: {}",
                status
                    .presence_person_model_path
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_model_config_path: {}",
                status
                    .presence_person_model_config_path
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_person_debug_output_dir: {}",
                status
                    .presence_person_debug_output_dir
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_pose_bridge_dll_path: {}",
                status
                    .presence_pose_bridge_dll_path
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_pose_model_path: {}",
                status
                    .presence_pose_model_path
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_pose_min_landmark_visibility: {}",
                status
                    .presence_pose_min_landmark_visibility
                    .as_deref()
                    .unwrap_or("<unset>")
            );
            println!(
                "presence_pose_min_landmark_presence: {}",
                status
                    .presence_pose_min_landmark_presence
                    .as_deref()
                    .unwrap_or("<unset>")
            );
        }
        InstallerCommand::InstallProvider {
            provider_binary_path,
            wake_auth_source,
            tile_visibility,
            auto_wake_on_advise,
            dry_run,
        } => {
            let plan = ProviderInstallPlan::new(provider_binary_path)
                .with_wake_auth_source(wake_auth_source)
                .with_tile_visibility(tile_visibility)
                .with_auto_wake_on_advise(auto_wake_on_advise);
            if dry_run {
                print_provider_install_plan("install-provider dry-run", &plan);
                return Ok(());
            }
            ProviderRegistry::install_provider(&plan)?;
            println!("installed {}", plan.provider_name);
            println!("provider_clsid: {}", plan.provider_clsid);
        }
        InstallerCommand::UninstallProvider { dry_run } => {
            if dry_run {
                println!("uninstall-provider dry-run: provider registry keys will be removed");
                return Ok(());
            }
            ProviderRegistry::uninstall_provider()?;
            println!("uninstalled WinFaceUnlockProvider");
        }
        InstallerCommand::EmergencyDisableProvider { dry_run } => {
            if dry_run {
                println!(
                    "emergency-disable-provider dry-run: credential provider enumeration key will be removed"
                );
                return Ok(());
            }
            ProviderRegistry::emergency_disable_provider()?;
            println!("emergency disabled WinFaceUnlockProvider");
        }
        InstallerCommand::ProviderStatus => {
            let status = ProviderRegistry::provider_status();
            println!(
                "WinFaceUnlockProvider registered: {}",
                status.is_registered()
            );
            println!(
                "credential_provider_registered: {}",
                status.credential_provider_registered
            );
            println!("com_server_registered: {}", status.com_server_registered);
            println!(
                "project_config_registered: {}",
                status.project_config_registered
            );
        }
        InstallerCommand::Help => print_usage(),
    }

    Ok(())
}

fn parse_command(args: &[String]) -> Result<InstallerCommand, InstallerError> {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let service_binary_path = argument_value(args, "--service-binary")
        .map(PathBuf::from)
        .unwrap_or(default_service_binary_path()?);
    let provider_binary_path = argument_value(args, "--provider-binary")
        .map(PathBuf::from)
        .unwrap_or(default_provider_binary_path()?);
    let dry_run = has_flag(args, "--dry-run");

    match command {
        "install" => Ok(InstallerCommand::Install {
            plan: Box::new(parse_full_install_plan(
                args,
                service_binary_path,
                provider_binary_path,
            )?),
            dry_run,
        }),
        "uninstall" => Ok(InstallerCommand::Uninstall {
            plan: Box::new(parse_full_uninstall_plan(args)?),
            dry_run,
        }),
        "repair" => Ok(InstallerCommand::Repair {
            plan: Box::new(parse_full_install_plan(
                args,
                service_binary_path,
                provider_binary_path,
            )?),
            dry_run,
        }),
        "emergency-disable" => Ok(InstallerCommand::EmergencyDisable { dry_run }),
        "install-service" => Ok(InstallerCommand::InstallService {
            service_binary_path,
            start_after_install: has_flag(args, "--start"),
            dry_run,
        }),
        "uninstall-service" => Ok(InstallerCommand::UninstallService {
            stop_first: !has_flag(args, "--no-stop"),
            dry_run,
        }),
        "start-service" => Ok(InstallerCommand::StartService),
        "stop-service" => Ok(InstallerCommand::StopService),
        "service-status" => Ok(InstallerCommand::ServiceStatus),
        "repair-service" => Ok(InstallerCommand::RepairService {
            service_binary_path,
            dry_run,
        }),
        "configure-service-auth" => Ok(InstallerCommand::ConfigureServiceAuth {
            config: Box::new(parse_service_auth_config(args)?),
            dry_run,
        }),
        "configure-presence-lock" => Ok(InstallerCommand::ConfigurePresenceLock {
            patch: Box::new(parse_presence_registry_patch(args)?),
            dry_run,
        }),
        "service-auth-status" => Ok(InstallerCommand::ServiceAuthStatus),
        "install-provider" => Ok(InstallerCommand::InstallProvider {
            provider_binary_path,
            wake_auth_source: wake_auth_source_argument(args)?,
            tile_visibility: tile_visibility_argument(args),
            auto_wake_on_advise: !has_flag(args, "--no-auto-wake-on-advise"),
            dry_run,
        }),
        "uninstall-provider" => Ok(InstallerCommand::UninstallProvider { dry_run }),
        "emergency-disable-provider" => Ok(InstallerCommand::EmergencyDisableProvider { dry_run }),
        "provider-status" => Ok(InstallerCommand::ProviderStatus),
        "help" | "--help" | "-h" => Ok(InstallerCommand::Help),
        other => Err(InstallerError::InvalidArguments(format!(
            "unknown command: {other}"
        ))),
    }
}

fn parse_full_install_plan(
    args: &[String],
    service_binary_path: PathBuf,
    provider_binary_path: PathBuf,
) -> Result<FullInstallPlan, InstallerError> {
    let provider_plan = ProviderInstallPlan::new(provider_binary_path)
        .with_wake_auth_source(wake_auth_source_argument(args)?)
        .with_tile_visibility(tile_visibility_argument(args))
        .with_auto_wake_on_advise(!has_flag(args, "--no-auto-wake-on-advise"));
    let auth_config = if argument_value(args, "--face-template").is_some() {
        Some(parse_service_auth_config(args)?)
    } else {
        None
    };

    let resource_plan = ResourceDirectoryPlan::from_root_dir(
        install_root_from_service_binary_path(&service_binary_path)?,
    );
    let tray_binary_path =
        install_root_from_service_binary_path(&service_binary_path)?.join("control_tray.exe");

    Ok(FullInstallPlan {
        service_plan: ServiceInstallPlan::new(service_binary_path),
        provider_plan,
        resource_plan,
        auth_config,
        user_startup_plan: UserStartupPlan::new(tray_binary_path),
        start_service: has_flag(args, "--start") || has_flag(args, "--start-service"),
    })
}

fn parse_full_uninstall_plan(args: &[String]) -> Result<FullUninstallPlan, InstallerError> {
    if has_flag(args, "--delete-data") && has_flag(args, "--preserve-data") {
        return Err(InstallerError::InvalidArguments(
            "--delete-data and --preserve-data cannot be used together".to_owned(),
        ));
    }

    Ok(FullUninstallPlan {
        resource_plan: ResourceDirectoryPlan::from_environment_or_default(),
        stop_service_first: !has_flag(args, "--no-stop"),
        delete_data: !has_flag(args, "--preserve-data"),
        remove_user_startup: !has_flag(args, "--keep-user-startup"),
    })
}

fn tile_visibility_argument(args: &[String]) -> &'static str {
    if has_flag(args, "--show-tile-before-ready") {
        TILE_VISIBILITY_VISIBLE
    } else {
        TILE_VISIBILITY_HIDDEN_UNTIL_READY
    }
}

fn wake_auth_source_argument(args: &[String]) -> Result<&'static str, InstallerError> {
    match argument_value(args, "--wake-source").unwrap_or(WAKE_AUTH_SOURCE_LOCAL_CAMERA) {
        WAKE_AUTH_SOURCE_LOCAL_CAMERA => Ok(WAKE_AUTH_SOURCE_LOCAL_CAMERA),
        WAKE_AUTH_SOURCE_MANUAL_TEST => Ok(WAKE_AUTH_SOURCE_MANUAL_TEST),
        other => Err(InstallerError::InvalidArguments(format!(
            "--wake-source must be local-camera or manual-test, got {other}"
        ))),
    }
}

fn parse_service_auth_config(args: &[String]) -> Result<ServiceAuthRegistryConfig, InstallerError> {
    let face_template_path = argument_value(args, "--face-template")
        .map(PathBuf::from)
        .ok_or_else(|| {
            InstallerError::InvalidArguments("--face-template is required".to_owned())
        })?;
    let yunet_model_path = argument_value(args, "--yunet-model")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"models\face_detection_yunet_2023mar.onnx"));
    let sface_model_path = argument_value(args, "--sface-model")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"models\face_recognition_sface_2021dec.onnx"));
    let mut config = ServiceAuthRegistryConfig::local_camera(
        face_template_path,
        yunet_model_path,
        sface_model_path,
    );
    if let Some(minifasnet_model_path) = argument_value(args, "--minifasnet-model") {
        config.minifasnet_model_path = PathBuf::from(minifasnet_model_path);
    }

    if let Some(camera_id) = argument_value(args, "--camera-id") {
        config.camera_id = camera_id.to_owned();
    }
    config.frame_width = optional_u32(args, "--frame-width")?;
    config.frame_height = optional_u32(args, "--frame-height")?;
    if let Some(max_auth_frames) = optional_u32(args, "--max-auth-frames")? {
        config.max_auth_frames = max_auth_frames;
    }
    if let Some(required_consecutive) = optional_u32(args, "--required-consecutive")? {
        config.required_consecutive_match_count = required_consecutive;
    }
    if let Some(match_threshold) = optional_f32(args, "--match-threshold")? {
        config.match_threshold = match_threshold;
    }
    if has_flag(args, "--enable-presence-lock") && has_flag(args, "--disable-presence-lock") {
        return Err(InstallerError::InvalidArguments(
            "--enable-presence-lock and --disable-presence-lock cannot be used together".to_owned(),
        ));
    }
    config.presence_lock_enabled = has_flag(args, "--enable-presence-lock");
    if let Some(presence_owner_match_threshold) =
        optional_f32(args, "--presence-owner-match-threshold")?
    {
        config.presence_owner_match_threshold = presence_owner_match_threshold;
    }
    if let Some(presence_detector_kind) = argument_value(args, "--presence-detector-kind") {
        validate_presence_detector_kind(presence_detector_kind)?;
        config.presence_detector_kind = presence_detector_kind.to_owned();
    }
    if let Some(presence_tracking_mode) = argument_value(args, "--presence-tracking-mode") {
        validate_presence_tracking_mode(presence_tracking_mode)?;
        config.presence_tracking_mode = presence_tracking_mode.to_owned();
    }
    if let Some(presence_detector_fps) = optional_f32(args, "--presence-detector-fps")? {
        config.presence_detector_fps = presence_detector_fps;
    }
    config.presence_unload_model_when_idle = has_flag(args, "--presence-unload-model-when-idle");
    if let Some(presence_person_confidence_threshold) =
        optional_f32(args, "--presence-person-confidence-threshold")?
    {
        config.presence_person_confidence_threshold = presence_person_confidence_threshold;
    }
    if let Some(presence_person_detector_model) =
        argument_value(args, "--presence-person-detector-model")
    {
        validate_presence_person_detector_model(presence_person_detector_model)?;
        apply_presence_person_detector_model_defaults(&mut config, presence_person_detector_model);
    }
    if let Some(presence_person_suspect_fps) = optional_f32(args, "--presence-person-suspect-fps")?
    {
        config.presence_person_suspect_fps = presence_person_suspect_fps;
    }
    if let Some(presence_absent_required_frames) =
        optional_u32(args, "--presence-absent-required-frames")?
    {
        config.presence_absent_required_frames = presence_absent_required_frames;
    }
    if let Some(presence_boundary_margin_ratio) =
        optional_f32(args, "--presence-boundary-margin-ratio")?
    {
        config.presence_boundary_margin_ratio = presence_boundary_margin_ratio;
    }
    if let Some(presence_movement_delta_ratio) =
        optional_f32(args, "--presence-movement-delta-ratio")?
    {
        config.presence_movement_delta_ratio = presence_movement_delta_ratio;
    }
    if let Some(presence_person_model_path) = argument_value(args, "--presence-person-model") {
        config.presence_person_model_path = PathBuf::from(presence_person_model_path);
    }
    if let Some(presence_person_model_config_path) =
        argument_value(args, "--presence-person-model-config")
    {
        config.presence_person_model_config_path =
            Some(PathBuf::from(presence_person_model_config_path));
    }
    if let Some(path) = argument_value(args, "--presence-pose-bridge-dll") {
        config.presence_pose_bridge_dll_path = PathBuf::from(path);
    }
    if let Some(path) = argument_value(args, "--presence-pose-model") {
        config.presence_pose_model_path = PathBuf::from(path);
    }
    if let Some(value) = optional_f32(args, "--presence-pose-min-landmark-visibility")? {
        config.presence_pose_min_landmark_visibility = value;
    }
    if let Some(value) = optional_f32(args, "--presence-pose-min-landmark-presence")? {
        config.presence_pose_min_landmark_presence = value;
    }
    if let Some(minifasnet_crop_scale) = optional_f32(args, "--minifasnet-crop-scale")? {
        config.minifasnet_crop_scale = minifasnet_crop_scale;
    }
    if let Some(minifasnet_min_live_score) = optional_f32(args, "--minifasnet-min-live-score")? {
        config.minifasnet_min_live_score = minifasnet_min_live_score;
    }
    if let Some(minifasnet_min_spoof_score) = optional_f32(args, "--minifasnet-min-spoof-score")? {
        config.minifasnet_min_spoof_score = minifasnet_min_spoof_score;
    }
    if let Some(minifasnet_max_spoof_frame_ratio) =
        optional_f32(args, "--minifasnet-max-spoof-frame-ratio")?
    {
        config.minifasnet_max_spoof_frame_ratio = minifasnet_max_spoof_frame_ratio;
    }
    Ok(config)
}

fn parse_presence_registry_patch(
    args: &[String],
) -> Result<ServicePresenceRegistryPatch, InstallerError> {
    if has_flag(args, "--enable-presence-lock") && has_flag(args, "--disable-presence-lock") {
        return Err(InstallerError::InvalidArguments(
            "--enable-presence-lock and --disable-presence-lock cannot be used together".to_owned(),
        ));
    }
    let mut patch = ServicePresenceRegistryPatch {
        presence_lock_enabled: if has_flag(args, "--enable-presence-lock") {
            Some(true)
        } else if has_flag(args, "--disable-presence-lock") {
            Some(false)
        } else {
            None
        },
        ..ServicePresenceRegistryPatch::default()
    };

    patch.presence_owner_match_threshold = optional_f32(args, "--presence-owner-match-threshold")?;
    if let Some(presence_detector_kind) = argument_value(args, "--presence-detector-kind") {
        validate_presence_detector_kind(presence_detector_kind)?;
        patch.presence_detector_kind = Some(presence_detector_kind.to_owned());
    }
    if let Some(presence_tracking_mode) = argument_value(args, "--presence-tracking-mode") {
        validate_presence_tracking_mode(presence_tracking_mode)?;
        patch.presence_tracking_mode = Some(presence_tracking_mode.to_owned());
    }
    patch.presence_detector_fps = optional_f32(args, "--presence-detector-fps")?;
    if has_flag(args, "--presence-unload-model-when-idle") {
        patch.presence_unload_model_when_idle = Some(true);
    }
    if has_flag(args, "--presence-keep-model-loaded") {
        patch.presence_unload_model_when_idle = Some(false);
    }
    patch.presence_person_confidence_threshold =
        optional_f32(args, "--presence-person-confidence-threshold")?;
    if let Some(presence_person_detector_model) =
        argument_value(args, "--presence-person-detector-model")
    {
        validate_presence_person_detector_model(presence_person_detector_model)?;
        apply_presence_person_detector_model_patch_defaults(
            &mut patch,
            presence_person_detector_model,
        );
    }
    patch.presence_person_suspect_fps = optional_f32(args, "--presence-person-suspect-fps")?;
    patch.presence_absent_required_frames =
        optional_u32(args, "--presence-absent-required-frames")?;
    patch.presence_boundary_margin_ratio = optional_f32(args, "--presence-boundary-margin-ratio")?;
    patch.presence_movement_delta_ratio = optional_f32(args, "--presence-movement-delta-ratio")?;
    if let Some(presence_person_model_path) = argument_value(args, "--presence-person-model") {
        patch.presence_person_model_path = Some(PathBuf::from(presence_person_model_path));
    }
    if let Some(presence_person_model_config_path) =
        argument_value(args, "--presence-person-model-config")
    {
        patch.presence_person_model_config_path =
            Some(Some(PathBuf::from(presence_person_model_config_path)));
    }
    if has_flag(args, "--clear-presence-person-model-config") {
        patch.presence_person_model_config_path = Some(None);
    }
    if let Some(debug_output_dir) = argument_value(args, "--presence-person-debug-output-dir") {
        patch.presence_person_debug_output_dir = Some(Some(PathBuf::from(debug_output_dir)));
    }
    if has_flag(args, "--clear-presence-person-debug-output-dir") {
        patch.presence_person_debug_output_dir = Some(None);
    }
    if let Some(path) = argument_value(args, "--presence-pose-bridge-dll") {
        patch.presence_pose_bridge_dll_path = Some(PathBuf::from(path));
    }
    if let Some(path) = argument_value(args, "--presence-pose-model") {
        patch.presence_pose_model_path = Some(PathBuf::from(path));
    }
    patch.presence_pose_min_landmark_visibility =
        optional_f32(args, "--presence-pose-min-landmark-visibility")?;
    patch.presence_pose_min_landmark_presence =
        optional_f32(args, "--presence-pose-min-landmark-presence")?;

    if !presence_registry_patch_has_changes(&patch) {
        return Err(InstallerError::InvalidArguments(
            "configure-presence-lock requires at least one presence option".to_owned(),
        ));
    }
    Ok(patch)
}

fn optional_u32(args: &[String], name: &str) -> Result<Option<u32>, InstallerError> {
    argument_value(args, name)
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| {
            InstallerError::InvalidArguments(format!("{name} must be an unsigned integer"))
        })
}

fn optional_f32(args: &[String], name: &str) -> Result<Option<f32>, InstallerError> {
    argument_value(args, name)
        .map(str::parse::<f32>)
        .transpose()
        .map_err(|_| InstallerError::InvalidArguments(format!("{name} must be a number")))
}

fn validate_presence_detector_kind(value: &str) -> Result<(), InstallerError> {
    match value {
        "face-owner-match" | "opencv-dnn-person" | "mediapipe-pose-lite" => Ok(()),
        _ => Err(InstallerError::InvalidArguments(
            "--presence-detector-kind must be face-owner-match, opencv-dnn-person, or mediapipe-pose-lite".to_owned(),
        )),
    }
}

fn validate_presence_tracking_mode(value: &str) -> Result<(), InstallerError> {
    match value {
        "face-policy" | "continuous-low-fps" => Ok(()),
        _ => Err(InstallerError::InvalidArguments(
            "--presence-tracking-mode must be face-policy or continuous-low-fps".to_owned(),
        )),
    }
}

fn validate_presence_person_detector_model(value: &str) -> Result<(), InstallerError> {
    match value {
        "mobilenet-ssd" | "yolov8-onnx" | "ort-yolov8-onnx" => Ok(()),
        _ => Err(InstallerError::InvalidArguments(
            "--presence-person-detector-model must be mobilenet-ssd, yolov8-onnx, or ort-yolov8-onnx".to_owned(),
        )),
    }
}

fn apply_presence_person_detector_model_defaults(
    config: &mut ServiceAuthRegistryConfig,
    detector_model: &str,
) {
    config.presence_person_detector_model = detector_model.to_owned();
    match detector_model {
        "mobilenet-ssd" => {
            config.presence_person_model_path =
                PathBuf::from(r"models\MobileNetSSD_deploy.caffemodel");
            config.presence_person_model_config_path =
                Some(PathBuf::from(r"models\MobileNetSSD_deploy.prototxt"));
        }
        "yolov8-onnx" | "ort-yolov8-onnx" => {
            config.presence_person_model_path = PathBuf::from(r"models\yolov8n.onnx");
            config.presence_person_model_config_path = None;
        }
        _ => {}
    }
}

fn apply_presence_person_detector_model_patch_defaults(
    patch: &mut ServicePresenceRegistryPatch,
    detector_model: &str,
) {
    patch.presence_person_detector_model = Some(detector_model.to_owned());
    match detector_model {
        "mobilenet-ssd" => {
            patch.presence_person_model_path =
                Some(PathBuf::from(r"models\MobileNetSSD_deploy.caffemodel"));
            patch.presence_person_model_config_path =
                Some(Some(PathBuf::from(r"models\MobileNetSSD_deploy.prototxt")));
        }
        "yolov8-onnx" | "ort-yolov8-onnx" => {
            patch.presence_person_model_path = Some(PathBuf::from(r"models\yolov8n.onnx"));
            patch.presence_person_model_config_path = Some(None);
        }
        _ => {}
    }
}

fn presence_registry_patch_has_changes(patch: &ServicePresenceRegistryPatch) -> bool {
    patch.presence_lock_enabled.is_some()
        || patch.presence_owner_match_threshold.is_some()
        || patch.presence_detector_kind.is_some()
        || patch.presence_tracking_mode.is_some()
        || patch.presence_detector_fps.is_some()
        || patch.presence_unload_model_when_idle.is_some()
        || patch.presence_person_confidence_threshold.is_some()
        || patch.presence_person_detector_model.is_some()
        || patch.presence_person_suspect_fps.is_some()
        || patch.presence_absent_required_frames.is_some()
        || patch.presence_boundary_margin_ratio.is_some()
        || patch.presence_movement_delta_ratio.is_some()
        || patch.presence_person_model_path.is_some()
        || patch.presence_person_model_config_path.is_some()
        || patch.presence_person_debug_output_dir.is_some()
        || patch.presence_pose_bridge_dll_path.is_some()
        || patch.presence_pose_model_path.is_some()
        || patch.presence_pose_min_landmark_visibility.is_some()
        || patch.presence_pose_min_landmark_presence.is_some()
}

fn default_service_binary_path() -> Result<PathBuf, InstallerError> {
    let installer_path = std::env::current_exe().map_err(InstallerError::Io)?;
    Ok(installer_path.with_file_name(service_binary_file_name()))
}

fn service_binary_file_name() -> OsString {
    if cfg!(windows) {
        OsString::from("win_service.exe")
    } else {
        OsString::from("win_service")
    }
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn argument_value<'args>(args: &'args [String], name: &str) -> Option<&'args str> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].as_str())
}

fn print_install_plan(label: &str, plan: &ServiceInstallPlan) {
    println!("{label}");
    println!("service_name: {}", plan.service_name.to_string_lossy());
    println!("display_name: {}", plan.display_name.to_string_lossy());
    println!("binary: {}", plan.service_binary_path.display());
    println!("launch_argument: --service");
    println!("account: LocalSystem");
}

fn print_full_install_plan(label: &str, plan: &FullInstallPlan) {
    println!("{label}");
    print_resource_directory_plan(&plan.resource_plan);
    print_install_plan("service plan", &plan.service_plan);
    println!(
        "user_tray_startup: {}",
        plan.user_startup_plan.tray_binary_path.display()
    );
    if let Some(config) = &plan.auth_config {
        print_service_auth_config("service auth config", config);
    } else {
        println!("service auth config: <unchanged>");
    }
    print_provider_install_plan("provider plan", &plan.provider_plan);
    println!("start_service: {}", plan.start_service);
}

fn print_full_uninstall_plan(label: &str, plan: &FullUninstallPlan) {
    println!("{label}");
    print_resource_directory_plan(&plan.resource_plan);
    println!("stop_service_first: {}", plan.stop_service_first);
    println!("delete_data: {}", plan.delete_data);
    println!("remove_user_startup: {}", plan.remove_user_startup);
}

fn print_resource_directory_plan(plan: &ResourceDirectoryPlan) {
    println!("resource_root_dir: {}", plan.root_dir.display());
    println!("runtime_dir: {}", plan.runtime_dir.display());
    println!("logs_dir: {}", plan.logs_dir.display());
    println!(
        "credential_store_dir: {}",
        plan.credential_store_dir.display()
    );
    println!("presence_audit_dir: {}", plan.presence_audit_dir.display());
    println!("resource_acl: root/bin/models/config restricted; runtime/logs user-writable");
}

fn install_root_from_service_binary_path(
    service_binary_path: &Path,
) -> Result<PathBuf, InstallerError> {
    service_binary_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            InstallerError::InvalidArguments(format!(
                "service binary path has no install root: {}",
                service_binary_path.display()
            ))
        })
}

fn print_provider_install_plan(label: &str, plan: &ProviderInstallPlan) {
    println!("{label}");
    println!("provider_name: {}", plan.provider_name);
    println!("provider_clsid: {}", plan.provider_clsid);
    println!("binary: {}", plan.provider_binary_path.display());
    println!(
        "credential_provider_registry_path: HKLM\\{}",
        plan.credential_provider_registry_path
    );
    println!(
        "com_inproc_server_registry_path: HKLM\\{}",
        plan.com_inproc_server_registry_path
    );
    println!("threading_model: Apartment");
    println!("tile_visibility: {}", plan.tile_visibility);
    println!("auto_wake_on_advise: {}", plan.auto_wake_on_advise);
    println!("wake_auth_source: {}", plan.wake_auth_source);
    println!(
        "logon_wake_mode: {}",
        plan.logon_wake_mode.unwrap_or("<disabled>")
    );
}

fn print_service_auth_config(label: &str, config: &ServiceAuthRegistryConfig) {
    println!("{label}");
    println!("auth_mode: {}", config.auth_mode);
    println!(
        "face_template_path: {}",
        config.face_template_path.display()
    );
    println!("camera_id: {}", config.camera_id);
    println!("yunet_model_path: {}", config.yunet_model_path.display());
    println!("sface_model_path: {}", config.sface_model_path.display());
    println!(
        "minifasnet_model_path: {}",
        config.minifasnet_model_path.display()
    );
    println!("minifasnet_crop_scale: {}", config.minifasnet_crop_scale);
    println!(
        "minifasnet_min_live_score: {}",
        config.minifasnet_min_live_score
    );
    println!(
        "minifasnet_min_spoof_score: {}",
        config.minifasnet_min_spoof_score
    );
    println!(
        "minifasnet_max_spoof_frame_ratio: {}",
        config.minifasnet_max_spoof_frame_ratio
    );
    println!(
        "frame_width: {}",
        config
            .frame_width
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<camera-default>".to_owned())
    );
    println!(
        "frame_height: {}",
        config
            .frame_height
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<camera-default>".to_owned())
    );
    println!("max_auth_frames: {}", config.max_auth_frames);
    println!(
        "required_consecutive_match_count: {}",
        config.required_consecutive_match_count
    );
    println!("match_threshold: {}", config.match_threshold);
    println!("presence_lock_enabled: {}", config.presence_lock_enabled);
    println!(
        "presence_owner_match_threshold: {}",
        config.presence_owner_match_threshold
    );
    println!("presence_detector_kind: {}", config.presence_detector_kind);
    println!("presence_tracking_mode: {}", config.presence_tracking_mode);
    println!("presence_detector_fps: {}", config.presence_detector_fps);
    println!(
        "presence_unload_model_when_idle: {}",
        config.presence_unload_model_when_idle
    );
    println!(
        "presence_person_confidence_threshold: {}",
        config.presence_person_confidence_threshold
    );
    println!(
        "presence_person_detector_model: {}",
        config.presence_person_detector_model
    );
    println!(
        "presence_person_suspect_fps: {}",
        config.presence_person_suspect_fps
    );
    println!(
        "presence_absent_required_frames: {}",
        config.presence_absent_required_frames
    );
    println!(
        "presence_boundary_margin_ratio: {}",
        config.presence_boundary_margin_ratio
    );
    println!(
        "presence_movement_delta_ratio: {}",
        config.presence_movement_delta_ratio
    );
    println!(
        "presence_person_model_path: {}",
        config.presence_person_model_path.display()
    );
    println!(
        "presence_person_model_config_path: {}",
        config
            .presence_person_model_config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_owned())
    );
}

fn print_presence_registry_patch(label: &str, patch: &ServicePresenceRegistryPatch) {
    println!("{label}");
    print_optional_bool("presence_lock_enabled", patch.presence_lock_enabled);
    print_optional_f32(
        "presence_owner_match_threshold",
        patch.presence_owner_match_threshold,
    );
    print_optional_string(
        "presence_detector_kind",
        patch.presence_detector_kind.as_deref(),
    );
    print_optional_string(
        "presence_tracking_mode",
        patch.presence_tracking_mode.as_deref(),
    );
    print_optional_f32("presence_detector_fps", patch.presence_detector_fps);
    print_optional_bool(
        "presence_unload_model_when_idle",
        patch.presence_unload_model_when_idle,
    );
    print_optional_f32(
        "presence_person_confidence_threshold",
        patch.presence_person_confidence_threshold,
    );
    print_optional_string(
        "presence_person_detector_model",
        patch.presence_person_detector_model.as_deref(),
    );
    print_optional_f32(
        "presence_person_suspect_fps",
        patch.presence_person_suspect_fps,
    );
    print_optional_u32(
        "presence_absent_required_frames",
        patch.presence_absent_required_frames,
    );
    print_optional_f32(
        "presence_boundary_margin_ratio",
        patch.presence_boundary_margin_ratio,
    );
    print_optional_f32(
        "presence_movement_delta_ratio",
        patch.presence_movement_delta_ratio,
    );
    print_optional_path(
        "presence_person_model_path",
        &patch.presence_person_model_path,
    );
    print_optional_nullable_path(
        "presence_person_model_config_path",
        &patch.presence_person_model_config_path,
    );
    print_optional_nullable_path(
        "presence_person_debug_output_dir",
        &patch.presence_person_debug_output_dir,
    );
    print_optional_path(
        "presence_pose_bridge_dll_path",
        &patch.presence_pose_bridge_dll_path,
    );
    print_optional_path("presence_pose_model_path", &patch.presence_pose_model_path);
    print_optional_f32(
        "presence_pose_min_landmark_visibility",
        patch.presence_pose_min_landmark_visibility,
    );
    print_optional_f32(
        "presence_pose_min_landmark_presence",
        patch.presence_pose_min_landmark_presence,
    );
}

fn print_optional_bool(name: &str, value: Option<bool>) {
    if let Some(value) = value {
        println!("{name}: {value}");
    }
}

fn print_optional_u32(name: &str, value: Option<u32>) {
    if let Some(value) = value {
        println!("{name}: {value}");
    }
}

fn print_optional_f32(name: &str, value: Option<f32>) {
    if let Some(value) = value {
        println!("{name}: {value}");
    }
}

fn print_optional_string(name: &str, value: Option<&str>) {
    if let Some(value) = value {
        println!("{name}: {value}");
    }
}

fn print_optional_path(name: &str, value: &Option<PathBuf>) {
    if let Some(value) = value {
        println!("{name}: {}", value.display());
    }
}

fn print_optional_nullable_path(name: &str, value: &Option<Option<PathBuf>>) {
    if let Some(value) = value {
        println!(
            "{name}: {}",
            value
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_owned())
        );
    }
}

fn print_usage() {
    println!("WinFaceUnlock installer");
    println!("Usage:");
    println!(
        "  installer_cli install [--service-binary <path>] [--provider-binary <path>] [--face-template <path>] [--start|--start-service] [--wake-source local-camera|manual-test] [--show-tile-before-ready] [--no-auto-wake-on-advise] [--dry-run]"
    );
    println!("  installer_cli uninstall [--no-stop] [--preserve-data|--delete-data] [--dry-run]");
    println!(
        "  installer_cli repair [--service-binary <path>] [--provider-binary <path>] [--face-template <path>] [--start|--start-service] [--dry-run]"
    );
    println!("  installer_cli emergency-disable [--dry-run]");
    println!("  installer_cli install-service [--service-binary <path>] [--start] [--dry-run]");
    println!("  installer_cli uninstall-service [--no-stop] [--dry-run]");
    println!("  installer_cli start-service");
    println!("  installer_cli stop-service");
    println!("  installer_cli service-status");
    println!("  installer_cli repair-service [--service-binary <path>] [--dry-run]");
    println!(
        "  installer_cli configure-service-auth --face-template <path> [--camera-id opencv-index:0] [--yunet-model <path>] [--sface-model <path>] [--minifasnet-model <path>] [--minifasnet-crop-scale 2.7] [--minifasnet-min-live-score 0.80] [--minifasnet-min-spoof-score 0.70] [--minifasnet-max-spoof-frame-ratio 0.40] [--match-threshold 0.75] [--enable-presence-lock|--disable-presence-lock] [--presence-owner-match-threshold 0.50] [--presence-detector-kind face-owner-match|opencv-dnn-person|mediapipe-pose-lite] [--presence-tracking-mode face-policy|continuous-low-fps] [--presence-detector-fps 2.0] [--presence-person-detector-model mobilenet-ssd|yolov8-onnx|ort-yolov8-onnx] [--presence-person-suspect-fps 5.0] [--presence-person-confidence-threshold 0.50] [--presence-absent-required-frames 6] [--presence-boundary-margin-ratio 0.12] [--presence-movement-delta-ratio 0.04] [--presence-person-model <path>] [--presence-person-model-config <path>] [--presence-pose-bridge-dll <path>] [--presence-pose-model <path>] [--presence-pose-min-landmark-visibility 0.45] [--presence-pose-min-landmark-presence 0.45] [--presence-unload-model-when-idle] [--dry-run]"
    );
    println!(
        "  installer_cli configure-presence-lock [--enable-presence-lock|--disable-presence-lock] [--presence-detector-kind face-owner-match|opencv-dnn-person|mediapipe-pose-lite] [--presence-tracking-mode face-policy|continuous-low-fps] [--presence-detector-fps 2.0] [--presence-person-detector-model mobilenet-ssd|yolov8-onnx|ort-yolov8-onnx] [--presence-person-suspect-fps 5.0] [--presence-person-confidence-threshold 0.50] [--presence-absent-required-frames 6] [--presence-boundary-margin-ratio 0.12] [--presence-movement-delta-ratio 0.04] [--presence-person-model <path>] [--presence-person-model-config <path>|--clear-presence-person-model-config] [--presence-person-debug-output-dir <path>|--clear-presence-person-debug-output-dir] [--presence-pose-bridge-dll <path>] [--presence-pose-model <path>] [--presence-pose-min-landmark-visibility 0.45] [--presence-pose-min-landmark-presence 0.45] [--dry-run]"
    );
    println!("  installer_cli service-auth-status");
    println!(
        "  installer_cli install-provider [--provider-binary <path>] [--wake-source local-camera|manual-test] [--show-tile-before-ready] [--no-auto-wake-on-advise] [--dry-run]"
    );
    println!("  installer_cli uninstall-provider [--dry-run]");
    println!("  installer_cli emergency-disable-provider [--dry-run]");
    println!("  installer_cli provider-status");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_install_service_with_explicit_binary_and_start() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-service".to_owned(),
            "--service-binary".to_owned(),
            r"C:\WinFaceUnlock\win_service.exe".to_owned(),
            "--start".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallService {
                service_binary_path: PathBuf::from(r"C:\WinFaceUnlock\win_service.exe"),
                start_after_install: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_uninstall_service_defaults_to_stop_first() -> Result<(), InstallerError> {
        let args = vec!["installer_cli".to_owned(), "uninstall-service".to_owned()];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::UninstallService {
                stop_first: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_full_install_combines_service_provider_and_optional_auth() -> Result<(), InstallerError>
    {
        let args = vec![
            "installer_cli".to_owned(),
            "install".to_owned(),
            "--service-binary".to_owned(),
            r"C:\WinFaceUnlock\win_service.exe".to_owned(),
            "--provider-binary".to_owned(),
            r"C:\WinFaceUnlock\windows_provider.dll".to_owned(),
            "--face-template".to_owned(),
            r"C:\WinFaceUnlock\phase4-face-template.json".to_owned(),
            "--wake-source".to_owned(),
            "manual-test".to_owned(),
            "--show-tile-before-ready".to_owned(),
            "--start-service".to_owned(),
            "--dry-run".to_owned(),
        ];

        let InstallerCommand::Install { plan, dry_run } = parse_command(&args)? else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(dry_run);
        assert_eq!(
            plan.service_plan.service_binary_path,
            PathBuf::from(r"C:\WinFaceUnlock\win_service.exe")
        );
        assert_eq!(
            plan.provider_plan.provider_binary_path,
            PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll")
        );
        assert_eq!(
            plan.provider_plan.wake_auth_source,
            WAKE_AUTH_SOURCE_MANUAL_TEST
        );
        assert_eq!(plan.provider_plan.tile_visibility, TILE_VISIBILITY_VISIBLE);
        assert_eq!(
            plan.auth_config
                .as_ref()
                .map(|config| &config.face_template_path),
            Some(&PathBuf::from(
                r"C:\WinFaceUnlock\phase4-face-template.json"
            ))
        );
        assert!(plan.start_service);
        Ok(())
    }

    #[test]
    fn parse_full_uninstall_deletes_data_by_default() -> Result<(), InstallerError> {
        let args = vec!["installer_cli".to_owned(), "uninstall".to_owned()];

        let InstallerCommand::Uninstall { plan, dry_run } = parse_command(&args)? else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!dry_run);
        assert!(plan.stop_service_first);
        assert!(plan.delete_data);
        Ok(())
    }

    #[test]
    fn parse_full_uninstall_can_preserve_data_explicitly() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "uninstall".to_owned(),
            "--preserve-data".to_owned(),
            "--no-stop".to_owned(),
        ];

        let InstallerCommand::Uninstall { plan, .. } = parse_command(&args)? else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!plan.stop_service_first);
        assert!(!plan.delete_data);
        Ok(())
    }

    #[test]
    fn parse_full_uninstall_rejects_conflicting_data_flags() {
        let args = vec![
            "installer_cli".to_owned(),
            "uninstall".to_owned(),
            "--delete-data".to_owned(),
            "--preserve-data".to_owned(),
        ];

        let result = parse_command(&args);

        assert!(matches!(result, Err(InstallerError::InvalidArguments(_))));
    }

    #[test]
    fn parse_full_emergency_disable() -> Result<(), InstallerError> {
        let args = vec!["installer_cli".to_owned(), "emergency-disable".to_owned()];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::EmergencyDisable { dry_run: false }
        );
        Ok(())
    }

    #[test]
    fn argument_value_reads_service_binary_path() {
        let args = vec![
            "installer_cli".to_owned(),
            "repair-service".to_owned(),
            "--service-binary".to_owned(),
            "service.exe".to_owned(),
        ];

        assert_eq!(
            argument_value(&args, "--service-binary"),
            Some("service.exe")
        );
    }

    #[test]
    fn parse_install_provider_with_explicit_binary() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-provider".to_owned(),
            "--provider-binary".to_owned(),
            r"C:\WinFaceUnlock\windows_provider.dll".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallProvider {
                provider_binary_path: PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll"),
                wake_auth_source: WAKE_AUTH_SOURCE_LOCAL_CAMERA,
                tile_visibility: TILE_VISIBILITY_HIDDEN_UNTIL_READY,
                auto_wake_on_advise: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_install_provider_with_manual_test_wake_source() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-provider".to_owned(),
            "--provider-binary".to_owned(),
            r"C:\WinFaceUnlock\windows_provider.dll".to_owned(),
            "--wake-source".to_owned(),
            "manual-test".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallProvider {
                provider_binary_path: PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll"),
                wake_auth_source: WAKE_AUTH_SOURCE_MANUAL_TEST,
                tile_visibility: TILE_VISIBILITY_HIDDEN_UNTIL_READY,
                auto_wake_on_advise: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_install_provider_with_visible_tile_for_debugging() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-provider".to_owned(),
            "--provider-binary".to_owned(),
            r"C:\WinFaceUnlock\windows_provider.dll".to_owned(),
            "--show-tile-before-ready".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallProvider {
                provider_binary_path: PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll"),
                wake_auth_source: WAKE_AUTH_SOURCE_LOCAL_CAMERA,
                tile_visibility: TILE_VISIBILITY_VISIBLE,
                auto_wake_on_advise: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_install_provider_can_disable_auto_wake_for_manual_debugging()
    -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-provider".to_owned(),
            "--provider-binary".to_owned(),
            r"C:\WinFaceUnlock\windows_provider.dll".to_owned(),
            "--no-auto-wake-on-advise".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallProvider {
                provider_binary_path: PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll"),
                wake_auth_source: WAKE_AUTH_SOURCE_LOCAL_CAMERA,
                tile_visibility: TILE_VISIBILITY_HIDDEN_UNTIL_READY,
                auto_wake_on_advise: false,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_emergency_disable_provider() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "emergency-disable-provider".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::EmergencyDisableProvider { dry_run: false }
        );
        Ok(())
    }

    #[test]
    fn parse_provider_status() -> Result<(), InstallerError> {
        let args = vec!["installer_cli".to_owned(), "provider-status".to_owned()];

        assert_eq!(parse_command(&args)?, InstallerCommand::ProviderStatus);
        Ok(())
    }

    #[test]
    fn parse_configure_service_auth_with_explicit_camera_values() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-service-auth".to_owned(),
            "--face-template".to_owned(),
            r"D:\WinFaceUnlock\phase4-face-template.json".to_owned(),
            "--camera-id".to_owned(),
            "opencv-index:1".to_owned(),
            "--match-threshold".to_owned(),
            "0.6".to_owned(),
            "--required-consecutive".to_owned(),
            "1".to_owned(),
            "--enable-presence-lock".to_owned(),
            "--presence-owner-match-threshold".to_owned(),
            "0.48".to_owned(),
            "--presence-detector-kind".to_owned(),
            "opencv-dnn-person".to_owned(),
            "--presence-tracking-mode".to_owned(),
            "continuous-low-fps".to_owned(),
            "--presence-detector-fps".to_owned(),
            "3.0".to_owned(),
            "--presence-person-detector-model".to_owned(),
            "yolov8-onnx".to_owned(),
            "--presence-person-suspect-fps".to_owned(),
            "5.0".to_owned(),
            "--presence-person-confidence-threshold".to_owned(),
            "0.62".to_owned(),
            "--presence-absent-required-frames".to_owned(),
            "8".to_owned(),
            "--presence-boundary-margin-ratio".to_owned(),
            "0.16".to_owned(),
            "--presence-movement-delta-ratio".to_owned(),
            "0.06".to_owned(),
            "--presence-person-model".to_owned(),
            r"D:\WinFaceUnlock\person.onnx".to_owned(),
            "--minifasnet-model".to_owned(),
            r"D:\WinFaceUnlock\minifasnet.onnx".to_owned(),
            "--minifasnet-crop-scale".to_owned(),
            "1.3".to_owned(),
            "--minifasnet-max-spoof-frame-ratio".to_owned(),
            "0.45".to_owned(),
        ];

        let InstallerCommand::ConfigureServiceAuth { config, dry_run } = parse_command(&args)?
        else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!dry_run);
        assert_eq!(
            config.face_template_path,
            PathBuf::from(r"D:\WinFaceUnlock\phase4-face-template.json")
        );
        assert_eq!(config.camera_id, "opencv-index:1");
        assert_eq!(config.match_threshold, 0.6);
        assert_eq!(config.required_consecutive_match_count, 1);
        assert!(config.presence_lock_enabled);
        assert_eq!(config.presence_owner_match_threshold, 0.48);
        assert_eq!(config.presence_detector_kind, "opencv-dnn-person");
        assert_eq!(config.presence_tracking_mode, "continuous-low-fps");
        assert_eq!(config.presence_detector_fps, 3.0);
        assert!(!config.presence_unload_model_when_idle);
        assert_eq!(config.presence_person_detector_model, "yolov8-onnx");
        assert_eq!(config.presence_person_suspect_fps, 5.0);
        assert_eq!(config.presence_person_confidence_threshold, 0.62);
        assert_eq!(config.presence_absent_required_frames, 8);
        assert_eq!(config.presence_boundary_margin_ratio, 0.16);
        assert_eq!(config.presence_movement_delta_ratio, 0.06);
        assert_eq!(
            config.presence_person_model_path,
            PathBuf::from(r"D:\WinFaceUnlock\person.onnx")
        );
        assert_eq!(config.presence_person_model_config_path, None);
        assert_eq!(
            config.minifasnet_model_path,
            PathBuf::from(r"D:\WinFaceUnlock\minifasnet.onnx")
        );
        assert_eq!(config.minifasnet_crop_scale, 1.3);
        assert_eq!(config.minifasnet_max_spoof_frame_ratio, 0.45);
        Ok(())
    }

    #[test]
    fn parse_configure_service_auth_can_disable_presence_lock() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-service-auth".to_owned(),
            "--face-template".to_owned(),
            r"D:\WinFaceUnlock\phase4-face-template.json".to_owned(),
            "--disable-presence-lock".to_owned(),
        ];

        let InstallerCommand::ConfigureServiceAuth { config, dry_run } = parse_command(&args)?
        else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!dry_run);
        assert!(!config.presence_lock_enabled);
        Ok(())
    }

    #[test]
    fn parse_configure_service_auth_does_not_enable_presence_lock_by_default()
    -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-service-auth".to_owned(),
            "--face-template".to_owned(),
            r"D:\WinFaceUnlock\phase4-face-template.json".to_owned(),
        ];

        let InstallerCommand::ConfigureServiceAuth { config, .. } = parse_command(&args)? else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!config.presence_lock_enabled);
        Ok(())
    }

    #[test]
    fn parse_configure_service_auth_rejects_conflicting_presence_lock_flags() {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-service-auth".to_owned(),
            "--face-template".to_owned(),
            r"D:\WinFaceUnlock\phase4-face-template.json".to_owned(),
            "--enable-presence-lock".to_owned(),
            "--disable-presence-lock".to_owned(),
        ];

        let result = parse_command(&args);

        assert!(matches!(result, Err(InstallerError::InvalidArguments(_))));
    }

    #[test]
    fn parse_configure_presence_lock_updates_presence_without_face_template()
    -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-presence-lock".to_owned(),
            "--enable-presence-lock".to_owned(),
            "--presence-detector-kind".to_owned(),
            "opencv-dnn-person".to_owned(),
            "--presence-tracking-mode".to_owned(),
            "continuous-low-fps".to_owned(),
            "--presence-person-detector-model".to_owned(),
            "yolov8-onnx".to_owned(),
            "--presence-person-debug-output-dir".to_owned(),
            r"D:\WinFaceUnlock\presence-debug".to_owned(),
        ];

        let InstallerCommand::ConfigurePresenceLock { patch, dry_run } = parse_command(&args)?
        else {
            return Err(InstallerError::InvalidArguments(
                "unexpected command".to_owned(),
            ));
        };

        assert!(!dry_run);
        assert_eq!(patch.presence_lock_enabled, Some(true));
        assert_eq!(
            patch.presence_detector_kind.as_deref(),
            Some("opencv-dnn-person")
        );
        assert_eq!(
            patch.presence_tracking_mode.as_deref(),
            Some("continuous-low-fps")
        );
        assert_eq!(
            patch.presence_person_detector_model.as_deref(),
            Some("yolov8-onnx")
        );
        assert_eq!(
            patch.presence_person_model_path,
            Some(PathBuf::from(r"models\yolov8n.onnx"))
        );
        assert_eq!(patch.presence_person_model_config_path, Some(None));
        assert_eq!(
            patch.presence_person_debug_output_dir,
            Some(Some(PathBuf::from(r"D:\WinFaceUnlock\presence-debug")))
        );
        Ok(())
    }

    #[test]
    fn parse_configure_presence_lock_requires_a_presence_option() {
        let args = vec![
            "installer_cli".to_owned(),
            "configure-presence-lock".to_owned(),
        ];

        let result = parse_command(&args);

        assert!(matches!(result, Err(InstallerError::InvalidArguments(_))));
    }
}
