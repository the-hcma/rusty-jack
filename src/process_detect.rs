//! Process inspection via `sysinfo` (macOS).

use std::ffi::OsString;

/// Executable path for a running process, when discoverable.
#[must_use]
pub fn process_exe_path(pid: u32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
        let process = system.process(pid)?;
        process
            .exe()
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| {
                process
                    .cmd()
                    .first()
                    .and_then(|arg| arg.to_str())
                    .map(str::to_string)
            })
            .filter(|path| !path.is_empty())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// Environment block for a running process.
#[must_use]
pub fn process_environ(pid: u32) -> Option<Vec<OsString>> {
    #[cfg(target_os = "macos")]
    {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_environ(UpdateKind::Always),
        );
        system
            .process(pid)
            .map(|process| process.environ().to_vec())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// True when any process has an exact name match (case-insensitive).
#[must_use]
pub fn any_process_with_exact_name(name: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        !process_pids_with_exact_name(name).is_empty()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        false
    }
}

/// True when any process name, argv, or executable path contains `needle`.
#[must_use]
pub fn any_process_cmdline_contains(needle: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let mut system = sysinfo::System::new();
        refresh_all_processes(&mut system);
        system
            .processes()
            .values()
            .any(|process| process_fields_contain_needle(process, needle))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = needle;
        false
    }
}

/// Send SIGKILL to every process whose name matches exactly (case-insensitive).
pub fn kill_processes_with_exact_name(name: &str) {
    #[cfg(target_os = "macos")]
    {
        use sysinfo::System;

        let mut system = System::new();
        for pid in process_pids_with_exact_name_on(&mut system, name) {
            if let Some(process) = system.process(pid) {
                let _ = process.kill();
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
    }
}

#[cfg(target_os = "macos")]
fn process_pids_with_exact_name(name: &str) -> Vec<sysinfo::Pid> {
    let mut system = sysinfo::System::new();
    process_pids_with_exact_name_on(&mut system, name)
}

#[cfg(target_os = "macos")]
fn process_pids_with_exact_name_on(system: &mut sysinfo::System, name: &str) -> Vec<sysinfo::Pid> {
    refresh_all_processes(system);
    system
        .processes()
        .iter()
        .filter(|(_, process)| {
            process
                .name()
                .to_str()
                .is_some_and(|process_name| process_name_matches_exact(process_name, name))
        })
        .map(|(pid, _)| *pid)
        .collect()
}

#[cfg(target_os = "macos")]
fn refresh_all_processes(system: &mut sysinfo::System) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        false,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
}

#[cfg(target_os = "macos")]
fn process_fields_contain_needle(process: &sysinfo::Process, needle: &str) -> bool {
    if process
        .name()
        .to_str()
        .is_some_and(|name| name.contains(needle))
    {
        return true;
    }
    if process
        .cmd()
        .iter()
        .any(|arg| arg.to_str().is_some_and(|value| value.contains(needle)))
    {
        return true;
    }
    process
        .exe()
        .and_then(|path| path.to_str())
        .is_some_and(|path| path.contains(needle))
}

fn process_name_matches_exact(name: &str, expected: &str) -> bool {
    name.eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::process_name_matches_exact;

    #[test]
    fn test_process_name_matches_exact_name() {
        assert!(process_name_matches_exact("eqMac", "eqMac"));
        assert!(process_name_matches_exact("EQMAC", "eqMac"));
        assert!(!process_name_matches_exact("eqMac Helper", "eqMac"));
    }
}
