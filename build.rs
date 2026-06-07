//! Embed git commit into the binary for `--version` / `--help`.

use std::path::{Path, PathBuf};

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let commit = git_short_commit().unwrap_or_else(|| "unknown".into());
    let long_version = format!("{version} (commit {commit})");

    println!("cargo:rustc-env=RUSTY_JACK_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=RUSTY_JACK_VERSION={long_version}");

    register_git_rerun_paths();
}

fn register_git_rerun_paths() {
    println!("cargo:rerun-if-changed=build.rs");

    let Some(git_dir) = git_dir() else {
        return;
    };

    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());

    let Ok(head) = std::fs::read_to_string(&head_path) else {
        return;
    };
    let head = head.trim();
    if let Some(ref_name) = head.strip_prefix("ref: ") {
        let ref_path = git_dir.join(ref_name.trim());
        if ref_path.exists() {
            println!("cargo:rerun-if-changed={}", ref_path.display());
        }
    }

    let packed_refs = git_dir.join("packed-refs");
    if packed_refs.exists() {
        println!("cargo:rerun-if-changed={}", packed_refs.display());
    }
}

fn git_dir() -> Option<PathBuf> {
    let dot_git = Path::new(".git");
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let content = std::fs::read_to_string(dot_git).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir: ")?.trim();
    Some(PathBuf::from(gitdir))
}

fn git_short_commit() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8(output.stdout).ok()?;
    let commit = commit.trim();
    if commit.is_empty() {
        None
    } else {
        Some(commit.to_string())
    }
}
