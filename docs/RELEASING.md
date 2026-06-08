# Releasing

Rusty Jack releases use [Release Please](https://github.com/googleapis/release-please) locally and publish through `make publish-release`.

**Full documentation:** this file. Quick links: [normal release](#normal-release) · [Makefile targets](#makefile-targets) · [homebrew-tap](#homebrew-tap-role) · [troubleshooting](#troubleshooting)

## Overview

| Stage | Command | Where |
|-------|---------|--------|
| Open/update release PR | `make update-release-pr` | Local (`gh auth`) |
| Review and merge release PR | GitHub UI | — |
| Create tag, GitHub release, tap PR | `make publish-release` | Local (`gh auth`) |

No GitHub Actions secrets, environments, or release workflows are required.

## Makefile targets

| Target | Script | Purpose |
|--------|--------|---------|
| `make update-release-pr` | `scripts/update-release-pr` | Open/update the version-bump + CHANGELOG PR |
| `make publish-release` | `scripts/publish-release` | Create GitHub release and open Homebrew tap PR |

Both targets automatically:

- verify you are on a **clean** `main`, `git fetch origin main`, and **fast-forward** when local `main` is behind `origin/main`
- `make update-release-pr` also runs `make publish-release` when a merged release PR exists but its GitHub tag is still missing
- use **`GH_TOKEN`** or `gh auth token`
- invoke **`npx release-please@latest`** for the release-please step

Flags for `publish-release` (pass after `--`):

```bash
make publish-release -- --dry-run    # preview only
make publish-release -- --tap-only   # skip GitHub release; update tap only
```

## One-time setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Install [GitHub CLI](https://cli.github.com/) and authenticate with write access to `the-hcma/rusty-jack` and `the-hcma/homebrew-tap`:

   ```bash
   gh auth login
   gh auth status
   ```

3. Install Node.js (`brew install node`) for `npx release-please`.
4. Ensure the tap allows auto-merge and has CI protecting `main`.

Workflow changes to release files require owner review through `CODEOWNERS`.

## Normal release

1. Merge feature and fix PRs to `main` using conventional commits (`feat:`, `fix:`, etc.).

2. Open or update the release PR:

   ```bash
   git checkout main
   make update-release-pr
   ```

   `make update-release-pr` fetches `origin/main` and fast-forwards local `main` when it is only behind the remote.

   This opens/updates a PR (label `release-please`) that bumps `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and `.release-please-manifest.json`. The script prints the open release PR URL as its last line (or a note when none exists).

3. Review and merge the release PR on GitHub.

4. Publish:

   ```bash
   make publish-release
   ```

   This creates the GitHub release/tag (`rusty-jack-vX.Y.Z`), opens a tap PR in `the-hcma/homebrew-tap` with auto-merge enabled, waits for tap CI and merge to finish, then prints the tap PR URL as its last line.

## Homebrew tap role

`the-hcma/homebrew-tap` holds `Formula/rusty-jack.rb`. Users install via:

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

You do not edit the tap by hand for routine releases. `make publish-release` renders the formula from `packaging/homebrew/rusty-jack.formula.in` (release tarball URL + SHA-256) and opens the tap PR.

## Backfill or repair

Re-run publish for the current `Cargo.toml` version:

```bash
git checkout main
make publish-release
```

If the GitHub release was deleted but the Release Please PR for that version is already merged, `make publish-release` falls back to `gh release create` using the merged release PR commit and the matching `CHANGELOG.md` section.

Tap only (GitHub release already exists):

```bash
make publish-release -- --tap-only
```

Preview without changes:

```bash
make publish-release -- --dry-run
```

## Verify

```bash
gh release view rusty-jack-v0.2.0 --repo the-hcma/rusty-jack
brew tap the-hcma/tap
brew info the-hcma/tap/rusty-jack
```

## Troubleshooting

**No release PR after `make update-release-pr`:** recent commits may not use releasable prefixes (`feat:`, `fix:`, etc.).

**`untagged, merged release PRs outstanding` from release-please:** the release PR merged but the GitHub tag was never created. `make update-release-pr` now detects this and runs `make publish-release` first. You can also publish manually:

```bash
make publish-release
```

**`main has diverged from origin/main`:** rebase or reset local `main` onto `origin/main` (for example `git pull --rebase origin main`). When local `main` is only behind the remote, `make update-release-pr` and `make publish-release` fast-forward automatically.

**Authentication errors:** confirm write access to both repos:

```bash
GH_TOKEN="$(gh auth token)" gh api repos/the-hcma/rusty-jack --jq .full_name
GH_TOKEN="$(gh auth token)" gh api repos/the-hcma/homebrew-tap --jq .full_name
```

**`npx` missing:** install Node.js (`brew install node`).

**Legacy Release Please workflow failures:** the CI workflow was removed because GitHub Actions cannot create PRs unless a repo setting is enabled. Release PR creation is local-only now (`make update-release-pr`).

## Security model

- Release Please PRs are the content review gate.
- Publishing requires your explicit local action with `gh auth` credentials.
- Dependabot auto-merge skips PRs labeled `release-please`.
- Do **not** add `merge-it` to Release Please PRs; merge manually after review.
