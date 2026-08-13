# AGENTS.md — Ground Rules for Rusty Jack

This file defines the standards for all contributors (human or AI) working on this codebase. Every change must comply with these rules before it is considered complete.

---

## Project

Rust CLI (`rusty-jack`) for macOS audio routing.

- Binary command: `rusty-jack`.
- Main purpose: keep macOS audio routed to a preferred HDMI, DisplayPort, dock, or line-out device so external-display audio can be controlled through the keyboard volume keys when an eqMac-style virtual volume layer is available.
- ScalarWebAPI-compatible speaker support: can wake ScalarWebAPI-compatible devices attached to a Mac output when that output is selected or when daemon activity triggers fire.
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

- **`--refresh`** (first): syncs via the stacking backend in `.github/stacking-tool` (`gh-stack` — `gh stack sync` / rebase as needed), prunes merged worktrees and branches, pulls latest `main`, then exits.
- **plain / `--worktree`** (second): repeats sync/cleanup, then creates or resumes a worktree under `.worktrees/<stack-name>-wt`.
- AI agents must always pass **`--no-interactive`** and an explicit **`--worktree`** name.
- Do not manually create worktrees or run `gh stack sync` separately — `start-development` is the single entry point for new work.
- After `start-development` finishes, **`cd` into the stack worktree** (`.worktrees/<stack-name>-wt`) before any other work. Do not stay in the primary clone.

### Main worktree is off-limits (agents)

The **primary clone** (repo root — first entry in `git worktree list`, usually on branch `main`) is the **main worktree**. Treat it as **read-only** unless the user explicitly authorizes touching it in the current conversation.

**Never on the main worktree** (without explicit user authorization):

- Edit, create, or delete source files, config, or lockfiles
- Run `cargo build`, `cargo test`, or other mutating toolchain commands
- Run `dep-updater` with `--dir` pointing at the primary clone (it may fast-forward `main` and mutate git state)
- Run `gh stack …`, commits, checkouts, or other git write operations
- Leave uncommitted changes, stray branches, or detached HEAD state

**Always** do implementation, investigation that mutates state, and validation in a **stack worktree** under `.worktrees/<stack-name>-wt`. Pass that path to tools (`--dir`, `cd`, etc.).

`start-development` may update the main worktree for environment sync only; that is not permission to work there. If you need to inspect `main` without changing it, use read-only commands (`git log`, `git show`, `gh pr view`) or a **detached temporary worktree** — not the primary clone.

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

Shell helpers live in `scripts/` **without** a `.sh` extension (for example `scripts/do-release` or `scripts/publish-release`, not `scripts/publish-release.sh`). Match existing script style: `#!/usr/bin/env bash`, `set -euo pipefail`, and executable bit set. Maintainer release flow: [docs/RELEASING.md](./docs/RELEASING.md) (`make do-release`).

---

## Commits, Stacking & Pull Requests

> Stacking backend is **`gh-stack`** (see `.github/stacking-tool`). Org skills live in [repository-helpers](https://github.com/the-hcma/repository-helpers) (`.cursor/skills/gh-stack/SKILL.md`). Do **not** use Graphite (`gt`) on this repo.

- **Worktree-per-stack.** Every new stack is created via `start-development --worktree <name> --no-interactive`.
- Never work directly on `main`. Create layers with `gh stack init <branch>` / `gh stack add <branch>`, then `git add` / `git commit` as usual.
- Keep each branch focused on one logical change.
- Before publishing/submitting any PR, run the required local gates (see **Pre-Commit Checklist** below). Prefer repository-helpers **`scripts/dev/submit-stack`** (runs pre-pr checks, then `gh stack submit --auto --open`). Agents must always pass `--auto` (and prefer `--open`) — never interactive `gh stack submit` / `gh stack view` without `--json`.
- Merge path is **GitHub’s merge queue**: enable auto-merge with `gh pr merge --auto --squash` when the operator asks to merge. Do **not** use the leftover `merge-it` label. **Always ask the user before enabling auto-merge.**
- Follow **Conventional Commits** for branch commits when practical: `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`.
- PR descriptions must include **Summary** and **Test plan** at minimum.

### Stacked PRs: fix bottom-up before publish

Each PR in a gh-stack is CI-tested against its merge base. A fmt, clippy, or test failure in an early branch fails the entire stack on GitHub Actions.

Before publishing a stack:

1. Check out the **bottom** branch (closest to `main`).
2. Run the **Pre-Commit Checklist** gates.
3. Fix failures, commit on that layer, then `gh stack rebase --upstack` as needed.
4. Repeat on each subsequent branch until the stack tip passes all gates.
5. Only then run `gh stack submit --auto --open --remote origin` (or `scripts/dev/submit-stack`).

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
