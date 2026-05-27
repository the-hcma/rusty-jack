//! Lifecycle helpers for the Rusty Jack CoreAudio HAL driver bundle.

use crate::eqmac::{
    eqmac_driver_backup_info, eqmac_driver_backup_path, remove_eqmac_driver_backup_info,
    write_eqmac_driver_backup_info, EqMacDriverBackupInfo, EQMAC_HAL_DRIVER_PATH,
};
use crate::hdmi_displayport_volume_control::{
    connected_hdmi_displayport_output_present, native_driver_info, native_driver_scope_note,
    RUSTY_JACK_DRIVER_BUNDLE_ID,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwapOutEqMacAction {
    RestoreBackup,
    AlreadyRestored,
    NoEqMacDriverState,
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
                "Install the Rusty Jack user audio driver for HDMI/DisplayPort volume keys?\n",
                "No sudo is needed."
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
    let Some(driver) = native_driver_info() else {
        return Ok(NativeDriverUninstallResult::NotInstalled);
    };
    std::fs::remove_dir_all(&driver.install_path).map_err(RustyJackError::Io)?;
    Ok(NativeDriverUninstallResult::Removed {
        install_path: driver.install_path,
        version: driver.version,
    })
}

pub fn swap_in_for_testing(interactive: bool) -> Result<DriverSwapInResult, RustyJackError> {
    let eqmac_path = Path::new(EQMAC_HAL_DRIVER_PATH);
    let backup_path = eqmac_backup_path_or_err()?;
    let action = plan_swap_in(eqmac_path.exists(), backup_path.exists());

    let eqmac_backup = match action {
        SwapInEqMacAction::BackUpSystemDriver => {
            if !interactive {
                return Ok(DriverSwapInResult::Skipped {
                    reason: "moving the system eqMac HAL driver requires interactive confirmation"
                        .into(),
                    command: Some("rusty-jack driver swap-in".into()),
                });
            }
            if !confirm_system_driver_move(
                "Back up the system eqMac HAL driver and swap in Rusty Jack for testing?",
            )? {
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

    let native_driver = install_bundled_native_driver()?;
    Ok(DriverSwapInResult::SwappedIn {
        eqmac_backup,
        native_driver,
    })
}

pub fn swap_out_for_testing(interactive: bool) -> Result<DriverSwapOutResult, RustyJackError> {
    let eqmac_path = Path::new(EQMAC_HAL_DRIVER_PATH);
    let backup_path = eqmac_backup_path_or_err()?;
    let action = plan_swap_out(eqmac_path.exists(), backup_path.exists());

    let restored_eqmac = match action {
        SwapOutEqMacAction::RestoreBackup => {
            if !interactive {
                return Ok(DriverSwapOutResult::Skipped {
                    reason:
                        "restoring the system eqMac HAL driver requires interactive confirmation"
                            .into(),
                    command: Some("rusty-jack driver swap-out".into()),
                });
            }
            let info = eqmac_driver_backup_info().unwrap_or_else(|| EqMacDriverBackupInfo {
                original_path: EQMAC_HAL_DRIVER_PATH.into(),
                backup_path: backup_path.to_string_lossy().into_owned(),
                version: crate::hal_plugin::driver_bundle_info(&backup_path)
                    .and_then(|info| info.version),
                backed_up_at_unix: None,
            });
            if !confirm_system_driver_move(
                "Restore the backed-up system eqMac HAL driver and remove Rusty Jack's test driver?",
            )? {
                return Ok(DriverSwapOutResult::Skipped {
                    reason: "user declined driver swap-out".into(),
                    command: None,
                });
            }
            restore_eqmac_driver(&backup_path, eqmac_path)?;
            remove_eqmac_driver_backup_info()?;
            Some(info)
        }
        SwapOutEqMacAction::AlreadyRestored => None,
        SwapOutEqMacAction::NoEqMacDriverState => None,
        SwapOutEqMacAction::Conflict => {
            return Ok(DriverSwapOutResult::Skipped {
                reason: format!(
                    "eqMac driver exists at {EQMAC_HAL_DRIVER_PATH} and a managed backup also exists at {}; inspect before restoring",
                    backup_path.display()
                ),
                command: None,
            });
        }
    };

    let native_driver = remove_native_driver_if_installed()?;
    if restored_eqmac.is_none() && native_driver == NativeDriverUninstallResult::NotInstalled {
        return Ok(DriverSwapOutResult::UpToDate {
            message: "Rusty Jack driver is absent and eqMac is already restored".into(),
        });
    }

    Ok(DriverSwapOutResult::SwappedOut {
        restored_eqmac,
        native_driver,
    })
}

pub fn print_install_result(result: &NativeDriverInstallResult) {
    match result {
        NativeDriverInstallResult::NotNeededNoHdmiDisplayPort => {}
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
                println!("  restored eqMac: not needed; no managed backup was present");
            }
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

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(RustyJackError::Io)?;
    }
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(RustyJackError::Io)?;
    }
    copy_dir_all(source, destination)
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

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), RustyJackError> {
    std::fs::create_dir_all(destination).map_err(RustyJackError::Io)?;
    for entry in std::fs::read_dir(source).map_err(RustyJackError::Io)? {
        let entry = entry.map_err(RustyJackError::Io)?;
        let file_type = entry.file_type().map_err(RustyJackError::Io)?;
        let target = destination.join(entry.file_name());
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

fn plan_swap_out(eqmac_present: bool, backup_present: bool) -> SwapOutEqMacAction {
    match (eqmac_present, backup_present) {
        (false, true) => SwapOutEqMacAction::RestoreBackup,
        (true, false) => SwapOutEqMacAction::AlreadyRestored,
        (false, false) => SwapOutEqMacAction::NoEqMacDriverState,
        (true, true) => SwapOutEqMacAction::Conflict,
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
    fn test_swap_out_eqmac_state_plan() {
        assert_eq!(
            plan_swap_out(false, true),
            SwapOutEqMacAction::RestoreBackup
        );
        assert_eq!(
            plan_swap_out(true, false),
            SwapOutEqMacAction::AlreadyRestored
        );
        assert_eq!(
            plan_swap_out(false, false),
            SwapOutEqMacAction::NoEqMacDriverState
        );
        assert_eq!(plan_swap_out(true, true), SwapOutEqMacAction::Conflict);
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
