//! Embed git commit into the binary for `--version` / `--help`.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let commit = resolve_git_commit(&manifest_dir);
    let long_version = format!("{version} (commit {commit})");

    println!("cargo:rustc-env=RUSTY_JACK_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=RUSTY_JACK_VERSION={long_version}");

    register_rerun_paths(&manifest_dir);
}

fn resolve_git_commit(manifest_dir: &Path) -> String {
    if let Ok(value) = std::env::var("RUSTY_JACK_GIT_COMMIT") {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    if let Some(commit) = git_short_commit(manifest_dir) {
        return commit;
    }
    if let Some(commit) = read_commit_stamp(manifest_dir) {
        return commit;
    }
    "unknown".into()
}

fn register_rerun_paths(manifest_dir: &Path) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTY_JACK_GIT_COMMIT");

    let stamp = manifest_dir.join("target/.rusty-jack-git-commit");
    if stamp.exists() {
        println!("cargo:rerun-if-changed={}", stamp.display());
    }

    let Some(git_dir) = git_dir(manifest_dir) else {
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

fn git_dir(manifest_dir: &Path) -> Option<PathBuf> {
    let dot_git = manifest_dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir: ")?.trim();
    Some(PathBuf::from(gitdir))
}

fn git_short_commit(manifest_dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_commit_hash(String::from_utf8(output.stdout).ok()?.as_str())
}

fn read_commit_stamp(manifest_dir: &Path) -> Option<String> {
    let path = manifest_dir.join("target/.rusty-jack-git-commit");
    let content = std::fs::read_to_string(path).ok()?;
    normalize_commit_hash(content.trim())
}

fn normalize_commit_hash(commit: &str) -> Option<String> {
    let commit = commit.trim();
    if commit.is_empty() {
        return None;
    }
    if commit.len() <= 12 {
        Some(commit.to_string())
    } else {
        Some(commit[..commit.len().min(12)].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_commit_hash_short() {
        assert_eq!(normalize_commit_hash("abc1234").as_deref(), Some("abc1234"));
    }

    #[test]
    fn test_normalize_commit_hash_long() {
        assert_eq!(
            normalize_commit_hash("abcdef0123456789abcdef0123456789abcdef01").as_deref(),
            Some("abcdef012345")
        );
    }

    #[test]
    fn test_normalize_commit_hash_empty() {
        assert_eq!(normalize_commit_hash(""), None);
    }
}
