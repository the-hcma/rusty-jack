//! Embed git commit into the binary for `--version` / `--help`.

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let commit = git_short_commit().unwrap_or_else(|| "unknown".into());
    let long_version = format!("{version} (commit {commit})");

    println!("cargo:rustc-env=RUSTY_JACK_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=RUSTY_JACK_VERSION={long_version}");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
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
