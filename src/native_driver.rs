//! Lifecycle helpers for the Rusty Jack CoreAudio HAL driver bundle.

use crate::eqmac::{
    eqmac_app_path, eqmac_driver_backup_info, eqmac_driver_backup_path,
    remove_eqmac_driver_backup_info, write_eqmac_driver_backup_info, EqMacDriverBackupInfo,
    EQMAC_EMBEDDED_DRIVER_PATH, EQMAC_HAL_DRIVER_PATH,
};
use crate::hdmi_displayport_volume_control::{
    connected_hdmi_displayport_output_present, native_driver_info, native_driver_scope_note,
    native_driver_user_install_offered, RUSTY_JACK_DRIVER_BUNDLE_ID,
};
use crate::output_device::OutputDevice;
use crate::system_default::HalDriverInfo;
use crate::RustyJackError;
use dialoguer::console::style;
use dialoguer::Confirm;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RUSTY_JACK_DRIVER_BUNDLE_NAME: &str = "RustyJack.driver";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NativeDriverInstallResult {
    NotNeededNoHdmiDisplayPort,
    /// User install prompts are disabled until a signed native driver release ships.
    NotOffered,
    AlreadyInstalled {
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Installed {
        source_path: String,
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Skipped {
        reason: String,
    },
    BundleUnavailable {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NativeDriverUninstallResult {
    NotInstalled,
    Removed {
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Skipped {
        install_path: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NativeDriverUpgradeResult {
    NotInstalled,
    UpToDate {
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Upgraded {
        source_path: String,
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        from_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        to_version: Option<String>,
    },
    Skipped {
        install_path: String,
        reason: String,
    },
    BundleUnavailable {
        install_path: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DriverSwapInResult {
    SwappedIn {
        #[serde(skip_serializing_if = "Option::is_none")]
        eqmac_backup: Option<EqMacDriverBackupInfo>,
        native_driver: NativeDriverInstallResult,
    },
    Skipped {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
}

/// Outcome of restoring eqMac's system HAL driver after native-driver testing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EqMacHalRestoreResult {
    NotNeeded,
    RestoredFromBackup {
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    ReinstalledFromAppBundle {
        install_path: String,
        source_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    AlreadyPresent {
        install_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DriverSwapOutResult {
    SwappedOut {
        #[serde(skip_serializing_if = "Option::is_none")]
        restored_eqmac: Option<EqMacDriverBackupInfo>,
        native_driver: NativeDriverUninstallResult,
    },
    UpToDate {
        message: String,
    },
    Skipped {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwapInEqMacAction {
    BackUpSystemDriver,
    AlreadyBackedUp,
    NoEqMacDriver,
    Conflict,
}

#[must_use]
pub fn bundled_native_driver_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUSTY_JACK_DRIVER_BUNDLE").map(PathBuf::from) {
        if path.is_dir() {
            return Some(path);
        }
    }

    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let candidates = [
        exe_dir.join(RUSTY_JACK_DRIVER_BUNDLE_NAME),
        exe_dir
            .parent()
            .map(|prefix| {
                prefix
                    .join("share")
                    .join("rusty-jack")
                    .join(RUSTY_JACK_DRIVER_BUNDLE_NAME)
            })
            .unwrap_or_default(),
    ];

    candidates.into_iter().find(|path| path.is_dir())
}

#[must_use]
pub fn native_driver_install_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library/Audio/Plug-Ins/HAL")
            .join(RUSTY_JACK_DRIVER_BUNDLE_NAME)
    })
}

pub fn install_for_connected_hdmi_displayport(
    devices: &[OutputDevice],
    interactive: bool,
) -> Result<NativeDriverInstallResult, RustyJackError> {
    if !connected_hdmi_displayport_output_present(devices) {
        return Ok(NativeDriverInstallResult::NotNeededNoHdmiDisplayPort);
    }

    if let Some(driver) = native_driver_info() {
        return Ok(NativeDriverInstallResult::AlreadyInstalled {
            install_path: driver.install_path,
            version: driver.version,
        });
    }

    if !native_driver_user_install_offered() {
        return Ok(NativeDriverInstallResult::NotOffered);
    }

    let Some(source) = bundled_native_driver_path() else {
        return Ok(NativeDriverInstallResult::BundleUnavailable {
            message: bundle_unavailable_message(),
        });
    };

    if !interactive {
        return Ok(NativeDriverInstallResult::Skipped {
            reason: "run `rusty-jack install` interactively to install the native driver".into(),
        });
    }

    if !Confirm::new()
        .with_prompt(
            style(concat!(
                "Install the Rusty Jack user-scoped audio driver for HDMI/DisplayPort volume keys?\n",
                "No sudo is needed, but CoreAudio may not load it; use `rusty-jack driver swap-in` for a system HAL test."
            ))
            .cyan()
            .to_string(),
        )
        .default(true)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("driver install prompt failed: {err}")))?
    {
        return Ok(NativeDriverInstallResult::Skipped {
            reason: "user declined native driver install".into(),
        });
    }

    let info = crate::hal_plugin::driver_bundle_info(&source).ok_or_else(|| {
        RustyJackError::Config(format!(
            "driver bundle {} is missing a readable Info.plist",
            source.display()
        ))
    })?;
    let install_path = install_path_or_err()?;
    install_driver_bundle(&source, &install_path)?;

    Ok(NativeDriverInstallResult::Installed {
        source_path: source.to_string_lossy().into_owned(),
        install_path: install_path.to_string_lossy().into_owned(),
        version: info.version,
    })
}

pub fn uninstall_if_installed(
    interactive: bool,
) -> Result<NativeDriverUninstallResult, RustyJackError> {
    let Some(driver) = native_driver_info() else {
        return Ok(NativeDriverUninstallResult::NotInstalled);
    };

    if !interactive {
        return Ok(NativeDriverUninstallResult::Skipped {
            install_path: driver.install_path,
            reason: "run `rusty-jack uninstall` interactively to remove the native driver".into(),
        });
    }

    if !Confirm::new()
        .with_prompt(format!(
            "Remove Rusty Jack native audio driver at {}? {}.",
            driver.install_path,
            native_driver_scope_note(&driver.install_path)
        ))
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("driver uninstall prompt failed: {err}")))?
    {
        return Ok(NativeDriverUninstallResult::Skipped {
            install_path: driver.install_path,
            reason: "user declined native driver removal".into(),
        });
    }

    std::fs::remove_dir_all(&driver.install_path).map_err(RustyJackError::Io)?;
    Ok(NativeDriverUninstallResult::Removed {
        install_path: driver.install_path,
        version: driver.version,
    })
}

pub fn upgrade_if_materially_changed(
    interactive: bool,
) -> Result<NativeDriverUpgradeResult, RustyJackError> {
    let Some(installed) = native_driver_info() else {
        return Ok(NativeDriverUpgradeResult::NotInstalled);
    };

    let Some(source) = bundled_native_driver_path() else {
        return Ok(NativeDriverUpgradeResult::BundleUnavailable {
            install_path: installed.install_path,
            message: bundle_unavailable_message(),
        });
    };

    let source_info = crate::hal_plugin::driver_bundle_info(&source).ok_or_else(|| {
        RustyJackError::Config(format!(
            "driver bundle {} is missing a readable Info.plist",
            source.display()
        ))
    })?;

    if !driver_materially_changed(&source, &source_info, &installed)? {
        return Ok(NativeDriverUpgradeResult::UpToDate {
            install_path: installed.install_path,
            version: installed.version,
        });
    }

    if !native_driver_user_install_offered() {
        return Ok(NativeDriverUpgradeResult::UpToDate {
            install_path: installed.install_path,
            version: installed.version,
        });
    }

    if !interactive {
        return Ok(NativeDriverUpgradeResult::Skipped {
            install_path: installed.install_path,
            reason: "run `rusty-jack upgrade` interactively to upgrade the native driver".into(),
        });
    }

    if !Confirm::new()
        .with_prompt(format!(
            "Upgrade Rusty Jack native audio driver at {}?",
            installed.install_path
        ))
        .default(true)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("driver upgrade prompt failed: {err}")))?
    {
        return Ok(NativeDriverUpgradeResult::Skipped {
            install_path: installed.install_path,
            reason: "user declined native driver upgrade".into(),
        });
    }

    let install_path = PathBuf::from(&installed.install_path);
    install_driver_bundle(&source, &install_path)?;
    Ok(NativeDriverUpgradeResult::Upgraded {
        source_path: source.to_string_lossy().into_owned(),
        install_path: installed.install_path,
        from_version: installed.version,
        to_version: source_info.version,
    })
}

/// Install the bundled driver under `/Library/Audio/Plug-Ins/HAL` (requires sudo).
pub fn install_bundled_native_driver_to_system_hal(
) -> Result<NativeDriverInstallResult, RustyJackError> {
    let Some(source) = bundled_native_driver_path() else {
        return Ok(NativeDriverInstallResult::BundleUnavailable {
            message: bundle_unavailable_message(),
        });
    };
    let source_info = crate::hal_plugin::driver_bundle_info(&source).ok_or_else(|| {
        RustyJackError::Config(format!(
            "driver bundle {} is missing a readable Info.plist",
            source.display()
        ))
    })?;
    let install_path = system_native_driver_install_path();

    if let Some(installed) = native_driver_info() {
        if installed.install_path == install_path.to_string_lossy()
            && !driver_materially_changed(&source, &source_info, &installed)?
        {
            return Ok(NativeDriverInstallResult::AlreadyInstalled {
                install_path: installed.install_path,
                version: installed.version,
            });
        }
    }

    sudo_install_driver_bundle(&source, &install_path)?;
    remove_user_scoped_native_driver_if_present()?;
    Ok(NativeDriverInstallResult::Installed {
        source_path: source.to_string_lossy().into_owned(),
        install_path: install_path.to_string_lossy().into_owned(),
        version: source_info.version,
    })
}

pub fn install_bundled_native_driver() -> Result<NativeDriverInstallResult, RustyJackError> {
    let Some(source) = bundled_native_driver_path() else {
        return Ok(NativeDriverInstallResult::BundleUnavailable {
            message: bundle_unavailable_message(),
        });
    };
    let source_info = crate::hal_plugin::driver_bundle_info(&source).ok_or_else(|| {
        RustyJackError::Config(format!(
            "driver bundle {} is missing a readable Info.plist",
            source.display()
        ))
    })?;
    let install_path = install_path_or_err()?;

    if let Some(installed) = native_driver_info() {
        if !driver_materially_changed(&source, &source_info, &installed)? {
            return Ok(NativeDriverInstallResult::AlreadyInstalled {
                install_path: installed.install_path,
                version: installed.version,
            });
        }
    }

    install_driver_bundle(&source, &install_path)?;
    Ok(NativeDriverInstallResult::Installed {
        source_path: source.to_string_lossy().into_owned(),
        install_path: install_path.to_string_lossy().into_owned(),
        version: source_info.version,
    })
}

pub fn remove_native_driver_if_installed() -> Result<NativeDriverUninstallResult, RustyJackError> {
    let mut removed_path = None;
    let mut version = None;
    let mut candidates = vec![system_native_driver_install_path()];
    if let Some(user_path) = native_driver_install_path() {
        if !candidates.contains(&user_path) {
            candidates.push(user_path);
        }
    }
    for path in candidates {
        if !path.exists() {
            continue;
        }
        version = crate::hal_plugin::driver_bundle_info(&path).and_then(|info| info.version);
        remove_native_driver_at_path(&path)?;
        removed_path = Some(path.to_string_lossy().into_owned());
    }
    let Some(install_path) = removed_path else {
        return Ok(NativeDriverUninstallResult::NotInstalled);
    };
    Ok(NativeDriverUninstallResult::Removed {
        install_path,
        version,
    })
}

pub fn swap_in_for_testing(interactive: bool) -> Result<DriverSwapInResult, RustyJackError> {
    let eqmac_path = Path::new(EQMAC_HAL_DRIVER_PATH);
    let backup_path = eqmac_backup_path_or_err()?;
    let action = plan_swap_in(eqmac_path.exists(), backup_path.exists());

    let eqmac_backup = match action {
        SwapInEqMacAction::BackUpSystemDriver => {
            let allow_moves =
                crate::native_driver_hal_smoke::system_driver_moves_allowed(interactive);
            if !allow_moves {
                return Ok(DriverSwapInResult::Skipped {
                    reason: "moving the system eqMac HAL driver requires interactive confirmation (or set RUSTY_JACK_HAL_DRIVER_SMOKE=1)".into(),
                    command: Some("rusty-jack driver swap-in".into()),
                });
            }
            if interactive
                && !confirm_system_driver_move(
                    "Back up the system eqMac HAL driver and swap in Rusty Jack for testing?",
                )?
            {
                return Ok(DriverSwapInResult::Skipped {
                    reason: "user declined driver swap-in".into(),
                    command: None,
                });
            }
            backup_eqmac_driver(eqmac_path, &backup_path)?
        }
        SwapInEqMacAction::AlreadyBackedUp => eqmac_driver_backup_info(),
        SwapInEqMacAction::NoEqMacDriver => None,
        SwapInEqMacAction::Conflict => {
            return Ok(DriverSwapInResult::Skipped {
                reason: format!(
                    "eqMac driver exists at {EQMAC_HAL_DRIVER_PATH} and a managed backup also exists at {}; inspect before swapping",
                    backup_path.display()
                ),
                command: None,
            });
        }
    };

    let native_driver = install_bundled_native_driver_to_system_hal()?;
    Ok(DriverSwapInResult::SwappedIn {
        eqmac_backup,
        native_driver,
    })
}

/// Remove Rusty Jack's HAL driver and restore eqMac when the app is installed.
///
/// Uses the managed backup when present; otherwise reinstalls from the copy embedded in
/// `eqMac.app`. Always restarts `coreaudiod` when eqMac is installed so the HAL reloads.
pub fn restore_eqmac_hal_driver(allow_sudo: bool) -> Result<EqMacHalRestoreResult, RustyJackError> {
    if eqmac_app_path().is_none() {
        return Ok(EqMacHalRestoreResult::NotNeeded);
    }

    let eqmac_path = Path::new(EQMAC_HAL_DRIVER_PATH);
    let backup_path = eqmac_driver_backup_path();

    if !allow_sudo && restore_eqmac_hal_driver_needs_sudo(backup_path.as_deref()) {
        return Err(RustyJackError::Config(
            "restoring eqMac's HAL driver requires sudo (or set RUSTY_JACK_HAL_DRIVER_SMOKE=1)"
                .into(),
        ));
    }

    let _ = remove_native_driver_if_installed();

    if let Some(backup) = backup_path.as_ref().filter(|path| path.exists()) {
        if eqmac_path.exists() {
            sudo_rm_rf(eqmac_path)?;
        }
        let version = crate::hal_plugin::driver_bundle_info(backup).and_then(|info| info.version);
        restore_eqmac_driver(backup, eqmac_path)?;
        remove_eqmac_driver_backup_info()?;
        sudo_restart_coreaudiod()?;
        return Ok(EqMacHalRestoreResult::RestoredFromBackup {
            install_path: EQMAC_HAL_DRIVER_PATH.into(),
            version,
        });
    }

    if eqmac_hal_driver_bundle_valid(eqmac_path) {
        let version =
            crate::hal_plugin::driver_bundle_info(eqmac_path).and_then(|info| info.version);
        sudo_restart_coreaudiod()?;
        return Ok(EqMacHalRestoreResult::AlreadyPresent {
            install_path: EQMAC_HAL_DRIVER_PATH.into(),
            version,
        });
    }

    let embedded = Path::new(EQMAC_EMBEDDED_DRIVER_PATH);
    if !embedded.is_dir() {
        return Err(RustyJackError::Config(format!(
            "eqMac is installed but its HAL driver is missing at {EQMAC_HAL_DRIVER_PATH} and no managed backup or embedded copy exists at {}",
            embedded.display()
        )));
    }

    if eqmac_path.exists() {
        sudo_rm_rf(eqmac_path)?;
    }
    sudo_install_eqmac_driver_from_embedded(embedded, eqmac_path)?;
    let version = crate::hal_plugin::driver_bundle_info(eqmac_path).and_then(|info| info.version);
    sudo_restart_coreaudiod()?;
    Ok(EqMacHalRestoreResult::ReinstalledFromAppBundle {
        install_path: EQMAC_HAL_DRIVER_PATH.into(),
        source_path: EQMAC_EMBEDDED_DRIVER_PATH.into(),
        version,
    })
}

pub fn swap_out_for_testing(interactive: bool) -> Result<DriverSwapOutResult, RustyJackError> {
    let allow_moves = crate::native_driver_hal_smoke::system_driver_moves_allowed(interactive);
    if !allow_moves && eqmac_app_path().is_some() {
        return Ok(DriverSwapOutResult::Skipped {
            reason:
                "restoring the system eqMac HAL driver requires interactive confirmation (or set RUSTY_JACK_HAL_DRIVER_SMOKE=1)".into(),
            command: Some("rusty-jack driver swap-out".into()),
        });
    }
    if interactive
        && eqmac_app_path().is_some()
        && !confirm_system_driver_move(
            "Restore eqMac's HAL driver and remove Rusty Jack's test driver?",
        )?
    {
        return Ok(DriverSwapOutResult::Skipped {
            reason: "user declined driver swap-out".into(),
            command: None,
        });
    }

    let native_driver = remove_native_driver_if_installed()?;
    let eqmac_restore = restore_eqmac_hal_driver(allow_moves)?;
    let restored_eqmac = eqmac_restore_backup_info(&eqmac_restore);

    if matches!(eqmac_restore, EqMacHalRestoreResult::NotNeeded)
        && native_driver == NativeDriverUninstallResult::NotInstalled
    {
        return Ok(DriverSwapOutResult::UpToDate {
            message: "Rusty Jack driver is absent and eqMac is not installed".into(),
        });
    }

    Ok(DriverSwapOutResult::SwappedOut {
        restored_eqmac,
        native_driver,
    })
}

pub fn print_install_result(result: &NativeDriverInstallResult) {
    match result {
        NativeDriverInstallResult::NotNeededNoHdmiDisplayPort
        | NativeDriverInstallResult::NotOffered => {}
        NativeDriverInstallResult::AlreadyInstalled {
            install_path,
            version,
        } => {
            println!("Rusty Jack native audio driver already installed");
            print_driver_path(install_path, version.as_deref());
        }
        NativeDriverInstallResult::Installed {
            install_path,
            version,
            ..
        } => {
            println!("Installed Rusty Jack native audio driver");
            print_driver_path(install_path, version.as_deref());
            println!(
                "  note: restart audio apps if volume keys do not pick up the driver immediately"
            );
        }
        NativeDriverInstallResult::Skipped { reason } => {
            println!("Skipped native audio driver install");
            println!("  reason: {reason}");
        }
        NativeDriverInstallResult::BundleUnavailable { message } => {
            println!("Rusty Jack native audio driver bundle not available");
            println!("  note: {message}");
        }
    }
}

pub fn print_uninstall_result(result: &NativeDriverUninstallResult) {
    match result {
        NativeDriverUninstallResult::NotInstalled => {}
        NativeDriverUninstallResult::Removed {
            install_path,
            version,
        } => {
            println!("Removed Rusty Jack native audio driver");
            print_driver_path(install_path, version.as_deref());
        }
        NativeDriverUninstallResult::Skipped {
            install_path,
            reason,
        } => {
            println!("Kept Rusty Jack native audio driver");
            println!("  path:   {install_path}");
            println!("  reason: {reason}");
        }
    }
}

pub fn print_upgrade_result(result: &NativeDriverUpgradeResult) {
    match result {
        NativeDriverUpgradeResult::NotInstalled => {}
        NativeDriverUpgradeResult::UpToDate {
            install_path,
            version,
        } => {
            println!("Rusty Jack native audio driver is up to date");
            print_driver_path(install_path, version.as_deref());
        }
        NativeDriverUpgradeResult::Upgraded {
            install_path,
            from_version,
            to_version,
            ..
        } => {
            println!("Upgraded Rusty Jack native audio driver");
            println!(
                "  version: {} -> {}",
                from_version.as_deref().unwrap_or("(unknown)"),
                to_version.as_deref().unwrap_or("(unknown)")
            );
            println!("  path:    {install_path}");
        }
        NativeDriverUpgradeResult::Skipped {
            install_path,
            reason,
        } => {
            println!("Skipped native audio driver upgrade");
            println!("  path:   {install_path}");
            println!("  reason: {reason}");
        }
        NativeDriverUpgradeResult::BundleUnavailable {
            install_path,
            message,
        } => {
            println!("Rusty Jack native audio driver bundle not available");
            println!("  installed: {install_path}");
            println!("  note:      {message}");
        }
    }
}

pub fn print_driver_swap_in_result(result: &DriverSwapInResult) {
    match result {
        DriverSwapInResult::SwappedIn {
            eqmac_backup,
            native_driver,
        } => {
            println!("Swapped in Rusty Jack native audio driver for testing");
            if let Some(info) = eqmac_backup {
                println!("  eqMac backup: {}", info.backup_path);
                println!("  eqMac original: {}", info.original_path);
            } else {
                println!("  eqMac backup: not needed; system eqMac driver was not present");
            }
            print_install_result(native_driver);
            println!("  restore: rusty-jack driver swap-out");
        }
        DriverSwapInResult::Skipped { reason, command } => {
            println!("Skipped driver swap-in");
            println!("  reason: {reason}");
            if let Some(command) = command {
                println!("  retry:  {command}");
            }
        }
    }
}

pub fn print_eqmac_hal_restore_result(result: &EqMacHalRestoreResult) {
    match result {
        EqMacHalRestoreResult::NotNeeded => {
            println!("eqMac is not installed; no HAL restore needed");
        }
        EqMacHalRestoreResult::RestoredFromBackup {
            install_path,
            version,
        } => {
            println!("Restored eqMac HAL driver from managed backup");
            print_driver_path(install_path, version.as_deref());
            println!("  coreaudiod restarted");
        }
        EqMacHalRestoreResult::ReinstalledFromAppBundle {
            install_path,
            source_path,
            version,
        } => {
            println!("Reinstalled eqMac HAL driver from app bundle");
            print_driver_path(install_path, version.as_deref());
            println!("  source:  {source_path}");
            println!("  coreaudiod restarted");
        }
        EqMacHalRestoreResult::AlreadyPresent {
            install_path,
            version,
        } => {
            println!("eqMac HAL driver already present; refreshed CoreAudio");
            print_driver_path(install_path, version.as_deref());
        }
    }
}

pub fn print_driver_swap_out_result(result: &DriverSwapOutResult) {
    match result {
        DriverSwapOutResult::SwappedOut {
            restored_eqmac,
            native_driver,
        } => {
            println!("Swapped out Rusty Jack native audio driver");
            if let Some(info) = restored_eqmac {
                println!("  restored eqMac: {}", info.original_path);
            } else {
                println!("  restored eqMac: not needed (eqMac not installed or already present)");
            }
            println!("  repair:   rusty-jack driver restore-eqmac");
            print_uninstall_result(native_driver);
        }
        DriverSwapOutResult::UpToDate { message } => {
            println!("Driver swap-out is up to date");
            println!("  note: {message}");
        }
        DriverSwapOutResult::Skipped { reason, command } => {
            println!("Skipped driver swap-out");
            println!("  reason: {reason}");
            if let Some(command) = command {
                println!("  retry:  {command}");
            }
        }
    }
}

fn install_path_or_err() -> Result<PathBuf, RustyJackError> {
    native_driver_install_path().ok_or_else(|| {
        RustyJackError::Config("HOME is not set; cannot locate HAL Plug-Ins directory".into())
    })
}

fn eqmac_backup_path_or_err() -> Result<PathBuf, RustyJackError> {
    eqmac_driver_backup_path().ok_or_else(|| {
        RustyJackError::Config("HOME is not set; cannot locate eqMac driver backup path".into())
    })
}

fn bundle_unavailable_message() -> String {
    format!(
        "no bundled {RUSTY_JACK_DRIVER_BUNDLE_NAME} found; install from a package that includes the driver bundle or set RUSTY_JACK_DRIVER_BUNDLE"
    )
}

fn print_driver_path(path: &str, version: Option<&str>) {
    println!("  path:    {path}");
    println!("  scope:   {}", native_driver_scope_note(path));
    if let Some(version) = version {
        println!("  version: {version}");
    }
}

fn install_driver_bundle(source: &Path, destination: &Path) -> Result<(), RustyJackError> {
    validate_driver_source(source)?;

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(RustyJackError::Io)?;
    }
    copy_dir_all(source, destination)?;
    codesign_driver_bundle(destination);
    Ok(())
}

fn system_native_driver_install_path() -> PathBuf {
    PathBuf::from("/Library/Audio/Plug-Ins/HAL").join(RUSTY_JACK_DRIVER_BUNDLE_NAME)
}

fn remove_user_scoped_native_driver_if_present() -> Result<(), RustyJackError> {
    let Some(user_path) = native_driver_install_path() else {
        return Ok(());
    };
    if user_path.exists() {
        std::fs::remove_dir_all(&user_path).map_err(RustyJackError::Io)?;
    }
    Ok(())
}

fn remove_native_driver_at_path(path: &Path) -> Result<(), RustyJackError> {
    if !path.exists() {
        return Ok(());
    }
    if path.starts_with("/Library/") {
        sudo_rm_rf(path)?;
    } else {
        std::fs::remove_dir_all(path).map_err(RustyJackError::Io)?;
    }
    Ok(())
}

fn sudo_install_driver_bundle(source: &Path, destination: &Path) -> Result<(), RustyJackError> {
    validate_driver_source(source)?;
    let source = source
        .canonicalize()
        .map_err(RustyJackError::Io)?
        .to_string_lossy()
        .into_owned();
    let destination = destination.to_string_lossy().into_owned();
    let ring_dir = crate::passthrough::PASSTHROUGH_RING_PATH
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or(crate::passthrough::PASSTHROUGH_RING_PATH);
    let script = format!(
        "set -euo pipefail\n\
         mkdir -p '{ring_dir}'\n\
         chmod 1777 '{ring_dir}'\n\
         rm -rf '{destination}'\n\
         cp -R '{source}' '{destination}'\n\
         rm -f '{destination}/.built'\n\
         chown -R root:wheel '{destination}'\n\
         codesign -s - --force --deep '{destination}' 2>/dev/null || true\n"
    );
    let status = Command::new("sudo")
        .args(["sh", "-ec", &script])
        .status()
        .map_err(RustyJackError::Io)?;
    if status.success() {
        sudo_restart_coreaudiod()
    } else {
        Err(RustyJackError::Config(format!(
            "sudo install of native driver to {destination} failed with status {status}"
        )))
    }
}

fn validate_driver_source(source: &Path) -> Result<(), RustyJackError> {
    if source.file_name().and_then(|name| name.to_str()) != Some(RUSTY_JACK_DRIVER_BUNDLE_NAME) {
        return Err(RustyJackError::Config(format!(
            "driver source must be named {RUSTY_JACK_DRIVER_BUNDLE_NAME}: {}",
            source.display()
        )));
    }
    let info = crate::hal_plugin::driver_bundle_info(source).ok_or_else(|| {
        RustyJackError::Config(format!(
            "driver bundle {} is missing a readable Info.plist",
            source.display()
        ))
    })?;
    if info.bundle_id != RUSTY_JACK_DRIVER_BUNDLE_ID {
        return Err(RustyJackError::Config(format!(
            "driver bundle id {} does not match expected {RUSTY_JACK_DRIVER_BUNDLE_ID}",
            info.bundle_id
        )));
    }
    Ok(())
}

fn codesign_driver_bundle(destination: &Path) {
    let _ = Command::new("codesign")
        .args(["-s", "-", "--force", "--deep"])
        .arg(destination)
        .status();
}

fn backup_eqmac_driver(
    eqmac_path: &Path,
    backup_path: &Path,
) -> Result<Option<EqMacDriverBackupInfo>, RustyJackError> {
    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    let version = crate::hal_plugin::driver_bundle_info(eqmac_path).and_then(|info| info.version);
    sudo_mv(eqmac_path, backup_path)?;
    write_eqmac_driver_backup_info(backup_path, version).map(Some)
}

fn restore_eqmac_driver(backup_path: &Path, eqmac_path: &Path) -> Result<(), RustyJackError> {
    sudo_mv(backup_path, eqmac_path)
}

fn restore_eqmac_hal_driver_needs_sudo(backup_path: Option<&Path>) -> bool {
    backup_path.is_some_and(|path| path.exists())
        || !eqmac_hal_driver_bundle_valid(Path::new(EQMAC_HAL_DRIVER_PATH))
}

#[must_use]
fn eqmac_hal_driver_bundle_valid(path: &Path) -> bool {
    let executable = path.join("Contents/MacOS/eqMac");
    crate::hal_plugin::driver_bundle_info(path)
        .is_some_and(|info| info.bundle_id == "com.bitgapp.eqmac.driver")
        && executable.is_file()
}

fn eqmac_restore_backup_info(result: &EqMacHalRestoreResult) -> Option<EqMacDriverBackupInfo> {
    match result {
        EqMacHalRestoreResult::NotNeeded | EqMacHalRestoreResult::AlreadyPresent { .. } => None,
        EqMacHalRestoreResult::RestoredFromBackup { version, .. } => Some(EqMacDriverBackupInfo {
            original_path: EQMAC_HAL_DRIVER_PATH.into(),
            backup_path: eqmac_driver_backup_path()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            version: version.clone(),
            backed_up_at_unix: None,
        }),
        EqMacHalRestoreResult::ReinstalledFromAppBundle { version, .. } => {
            Some(EqMacDriverBackupInfo {
                original_path: EQMAC_HAL_DRIVER_PATH.into(),
                backup_path: EQMAC_EMBEDDED_DRIVER_PATH.into(),
                version: version.clone(),
                backed_up_at_unix: None,
            })
        }
    }
}

fn sudo_install_eqmac_driver_from_embedded(
    embedded: &Path,
    destination: &Path,
) -> Result<(), RustyJackError> {
    let embedded = embedded
        .canonicalize()
        .map_err(RustyJackError::Io)?
        .to_string_lossy()
        .into_owned();
    let destination = destination.to_string_lossy().into_owned();
    let script = format!(
        "set -euo pipefail\n\
         rm -rf '{destination}'\n\
         cp -R '{embedded}' '{destination}'\n\
         chown -R root:wheel '{destination}'\n"
    );
    let status = Command::new("sudo")
        .args(["sh", "-ec", &script])
        .status()
        .map_err(RustyJackError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(RustyJackError::Config(format!(
            "sudo reinstall of eqMac driver from {embedded} failed with status {status}"
        )))
    }
}

fn sudo_restart_coreaudiod() -> Result<(), RustyJackError> {
    let status = Command::new("sudo")
        .args(["sh", "-ec", "killall -9 coreaudiod 2>/dev/null || true"])
        .status()
        .map_err(RustyJackError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(RustyJackError::Config(format!(
            "sudo restart of coreaudiod failed with status {status}"
        )))
    }
}

fn confirm_system_driver_move(prompt: &str) -> Result<bool, RustyJackError> {
    Confirm::new()
        .with_prompt(format!("{prompt} This uses sudo for /Library."))
        .default(false)
        .interact()
        .map_err(|err| RustyJackError::Config(format!("driver swap prompt failed: {err}")))
}

fn sudo_mv(source: &Path, destination: &Path) -> Result<(), RustyJackError> {
    let status = Command::new("sudo")
        .arg("mv")
        .arg(source)
        .arg(destination)
        .status()
        .map_err(RustyJackError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(RustyJackError::Config(format!(
            "sudo mv {} {} failed with status {status}",
            source.display(),
            destination.display()
        )))
    }
}

fn sudo_rm_rf(path: &Path) -> Result<(), RustyJackError> {
    let status = Command::new("sudo")
        .arg("rm")
        .arg("-rf")
        .arg(path)
        .status()
        .map_err(RustyJackError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(RustyJackError::Config(format!(
            "sudo rm -rf {} failed with status {status}",
            path.display()
        )))
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), RustyJackError> {
    std::fs::create_dir_all(destination).map_err(RustyJackError::Io)?;
    for entry in std::fs::read_dir(source).map_err(RustyJackError::Io)? {
        let entry = entry.map_err(RustyJackError::Io)?;
        let name = entry.file_name();
        if name == ".built" {
            continue;
        }
        let file_type = entry.file_type().map_err(RustyJackError::Io)?;
        let target = destination.join(&name);
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(RustyJackError::Io)?;
        }
    }
    Ok(())
}

fn driver_materially_changed(
    source: &Path,
    source_info: &HalDriverInfo,
    installed: &HalDriverInfo,
) -> Result<bool, RustyJackError> {
    if source_info.version != installed.version {
        return Ok(true);
    }
    dirs_differ(source, Path::new(&installed.install_path))
}

fn plan_swap_in(eqmac_present: bool, backup_present: bool) -> SwapInEqMacAction {
    match (eqmac_present, backup_present) {
        (true, false) => SwapInEqMacAction::BackUpSystemDriver,
        (false, true) => SwapInEqMacAction::AlreadyBackedUp,
        (false, false) => SwapInEqMacAction::NoEqMacDriver,
        (true, true) => SwapInEqMacAction::Conflict,
    }
}

fn dirs_differ(left: &Path, right: &Path) -> Result<bool, RustyJackError> {
    if !right.exists() {
        return Ok(true);
    }
    for entry in std::fs::read_dir(left).map_err(RustyJackError::Io)? {
        let entry = entry.map_err(RustyJackError::Io)?;
        let left_path = entry.path();
        let right_path = right.join(entry.file_name());
        let file_type = entry.file_type().map_err(RustyJackError::Io)?;
        if file_type.is_dir() {
            if dirs_differ(&left_path, &right_path)? {
                return Ok(true);
            }
        } else if !right_path.is_file()
            || std::fs::read(&left_path).map_err(RustyJackError::Io)?
                != std::fs::read(&right_path).map_err(RustyJackError::Io)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_in_eqmac_state_plan() {
        assert_eq!(
            plan_swap_in(true, false),
            SwapInEqMacAction::BackUpSystemDriver
        );
        assert_eq!(
            plan_swap_in(false, true),
            SwapInEqMacAction::AlreadyBackedUp
        );
        assert_eq!(plan_swap_in(false, false), SwapInEqMacAction::NoEqMacDriver);
        assert_eq!(plan_swap_in(true, true), SwapInEqMacAction::Conflict);
    }

    #[test]
    fn test_driver_swap_result_serialization() {
        let result = DriverSwapInResult::Skipped {
            reason: "interactive confirmation required".into(),
            command: Some("rusty-jack driver swap-in".into()),
        };
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["status"], "skipped");
        assert_eq!(json["command"], "rusty-jack driver swap-in");
    }
}
