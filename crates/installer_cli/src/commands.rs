use std::{ffi::OsString, path::PathBuf};

use crate::{
    provider_registry::{ProviderInstallPlan, ProviderRegistry, default_provider_binary_path},
    service_manager::{
        InstallerError, ServiceInstallPlan, ServiceLifecycleCommand, ServiceManagerFacade,
    },
    service_registry::{ServiceAuthRegistry, ServiceAuthRegistryConfig},
};
use windows_provider::{
    TILE_VISIBILITY_HIDDEN_UNTIL_READY, TILE_VISIBILITY_VISIBLE, WAKE_AUTH_SOURCE_LOCAL_CAMERA,
    WAKE_AUTH_SOURCE_MANUAL_TEST,
};

#[derive(Clone, Debug, PartialEq)]
pub enum InstallerCommand {
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
        config: ServiceAuthRegistryConfig,
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
            config: parse_service_auth_config(args)?,
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
}

fn print_usage() {
    println!("WinFaceUnlock installer");
    println!("Usage:");
    println!("  installer_cli install-service [--service-binary <path>] [--start] [--dry-run]");
    println!("  installer_cli uninstall-service [--no-stop] [--dry-run]");
    println!("  installer_cli start-service");
    println!("  installer_cli stop-service");
    println!("  installer_cli service-status");
    println!("  installer_cli repair-service [--service-binary <path>] [--dry-run]");
    println!(
        "  installer_cli configure-service-auth --face-template <path> [--camera-id opencv-index:0] [--yunet-model <path>] [--sface-model <path>] [--minifasnet-model <path>] [--minifasnet-crop-scale 2.7] [--minifasnet-min-live-score 0.80] [--minifasnet-min-spoof-score 0.70] [--minifasnet-max-spoof-frame-ratio 0.40] [--match-threshold 0.75] [--dry-run]"
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
        assert_eq!(
            config.minifasnet_model_path,
            PathBuf::from(r"D:\WinFaceUnlock\minifasnet.onnx")
        );
        assert_eq!(config.minifasnet_crop_scale, 1.3);
        assert_eq!(config.minifasnet_max_spoof_frame_ratio, 0.45);
        Ok(())
    }
}
