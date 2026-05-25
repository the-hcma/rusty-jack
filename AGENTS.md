# AGENTS.md — Ground Rules for Rusty Jack

This file defines the standards for all contributors (human or AI) working on this codebase. Every change must comply with these rules before it is considered complete.

---

## Project

Rust CLI (`rusty-jack`) for macOS audio routing.

- Binary command: `rusty-jack`.
- Main purpose: keep macOS audio routed to a preferred HDMI, DisplayPort, dock, or line-out device so external-display audio can be controlled through the keyboard volume keys when an eqMac-style virtual volume layer is available.
- Sony-like speaker support: can wake Sony Songpal / ScalarWebAPI speakers attached to a Mac output when that output is selected or when daemon activity triggers fire.
- Config env: `RUSTY_JACK_CONFIG`; legacy alias: `HDMI_SOUND_CONTROLLER_CONFIG`.
- Default config: `~/.config/rusty-jack/config.json`.
- Do not commit local config files, private hostnames, secrets, logs, or machine-specific plist files.

---

## Session Startup

Before creating any branch or writing code, initialize the session from the repository root using [repository-helpers](https://github.com/the-hcma/repository-helpers):

```bash
~/work/ai/repository-helpers/scripts/dev/start-development --refresh
~/work/ai/repository-helpers/scripts/dev/start-development --worktree <stack-name> --no-interactive
```

- **`--refresh`** (first): syncs `main` with Graphite (`gt sync`), prunes merged worktrees and branches, pulls latest `main`, then exits.
- **plain / `--worktree`** (second): repeats sync/cleanup, then creates or resumes a worktree under `.worktrees/<stack-name>-wt`.
- AI agents must always pass **`--no-interactive`** and an explicit **`--worktree`** name.
- Do not manually create worktrees or run `gt sync` separately — `start-development` is the single entry point for new work.

---

## Language & Runtime

- Rust 1.85+ (`rust-version` in `Cargo.toml`).
- macOS 12+ only for real CoreAudio behavior.
- Build/test jobs run on macOS runners; non-macOS stubs only support editor and limited CI wiring checks.
- Keep dependencies in `Cargo.toml`; commit `Cargo.lock`.
- Avoid `unsafe`; the repo has `unsafe_code = "warn"`.

---

## Development

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Useful local commands:

```bash
make install
rusty-jack list
rusty-jack status
rusty-jack picker
rusty-jack upgrade
```

Hardware-mutating tests are ignored by default. Do not enable them unless you intend to change the Mac's current audio route.

---

## Commits, Stacking & Pull Requests

> See [GRAPHITE.md](./GRAPHITE.md) if present, and the org-wide Graphite guidance in `repository-helpers`.

- This project uses **Graphite (`gt`)** for branch stacking.
- **Worktree-per-stack.** Every new stack is created via `start-development --worktree <name> --no-interactive`.
- Never work directly on `main`.
- Keep each branch focused on one logical change.
- Submit with `gt submit --no-interactive --publish` when using Graphite.
- To merge, add the `merge-it` label. Never use `gh pr merge` directly.
- Follow **Conventional Commits** for branch commits when practical: `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`.
- PR descriptions must include **Summary** and **Test plan** at minimum.

---

## Repository Practices

Run from [repository-helpers](https://github.com/the-hcma/repository-helpers):

```bash
scripts/check-repo-practices --repo the-hcma/rusty-jack --suggest
```

---

## CI Checks (all must pass)

CI lives in `.github/workflows/ci.yml`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
```

No PR may be merged with a failing CI check.

---

## Pre-Commit Checklist

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `git diff --check`
- [ ] No secrets, local config, logs, or private hostnames in the diff
- [ ] User-facing behavior is documented in `README.md` or `docs/USAGE.md`
