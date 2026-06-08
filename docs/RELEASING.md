# Releasing

Rusty Jack releases are managed by Release Please and published locally through `make publish-release`.

## Overview

| Stage | Where | Credentials |
|-------|--------|-------------|
| Open/update release PR | GitHub Actions (`Release Please` workflow) | Built-in `GITHUB_TOKEN` |
| Review and merge release PR | GitHub UI | — |
| Create tag, GitHub release, tap PR | **Local** `make publish-release` | Your `gh auth` session (`GH_TOKEN`) |

Release Please runs on pushes to `main` and opens or updates a release PR. After you merge that PR, run `make publish-release` on your machine to create the GitHub release and open the Homebrew tap update PR.

No GitHub Actions secrets or environments are required for releases.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Install [GitHub CLI](https://cli.github.com/) and authenticate with write access to `the-hcma/rusty-jack` and `the-hcma/homebrew-tap`:

   ```bash
   gh auth login
   gh auth status
   ```

3. Install Node.js (for `npx release-please` used during publish).
4. Ensure the tap allows auto-merge and has CI protecting `main`; tap formula updates merge only after `Tap CI` passes.

Workflow changes to release files require owner review through `CODEOWNERS`.

## Security Model

- The Release Please PR (`release-please` label) is the content review gate for version bumps and changelog text.
- Publishing is an explicit local action using your own GitHub credentials — nothing in CI can create releases or push to the tap.
- Dependabot auto-merge skips PRs labeled `release-please`.
- Do **not** add `merge-it` to Release Please PRs; merge them manually after review.

Release Please PRs opened by `GITHUB_TOKEN` do not re-trigger other GitHub Actions workflows on the release branch. Review the release PR diff and CI on `main` before merging.

## Normal Release

1. Merge feature and fix PRs using conventional commit messages, for example `feat:`, `fix:`, or `docs:`.
2. Release Please runs automatically on the resulting push to `main` and opens or updates a release PR labeled `release-please`.
3. Review the Release Please PR. It updates:
   - `Cargo.toml`
   - `Cargo.lock`
   - `CHANGELOG.md`
   - `.release-please-manifest.json`
4. Merge the Release Please PR after review.
5. On your machine:

   ```bash
   git checkout main
   git pull --ff-only
   make publish-release
   ```

   This creates the GitHub release and tag (via release-please), then opens a tap PR in `the-hcma/homebrew-tap` with auto-merge enabled.

## Backfill Or Repair

Re-run publish for the current `Cargo.toml` version (idempotent):

```bash
git checkout main
git pull --ff-only
make publish-release
```

If the GitHub release already exists but the tap formula needs updating:

```bash
make publish-release -- --tap-only
```

Preview actions without changing anything:

```bash
make publish-release -- --dry-run
```

Do not publish new versions by editing release files on `main` outside the Release Please PR flow.

## Verify

Check the release:

```bash
gh release view v0.2.0 --repo the-hcma/rusty-jack
```

Check the tap formula:

```bash
brew tap the-hcma/tap
brew info the-hcma/tap/rusty-jack
```

Install from Brew:

```bash
brew install rusty-jack
```

## Troubleshooting

If no release PR appears, check that recent commits use releasable conventional commit prefixes such as `feat:` or `fix:`.

If `make publish-release` fails with authentication errors, confirm `gh auth status` shows write access to both repositories:

```bash
GH_TOKEN="$(gh auth token)" gh api repos/the-hcma/rusty-jack --jq .full_name
GH_TOKEN="$(gh auth token)" gh api repos/the-hcma/homebrew-tap --jq .full_name
```

If tap publication fails, fix the cause in `the-hcma/homebrew-tap`, then rerun `make publish-release -- --tap-only`.

If `npx` is missing, install Node.js (`brew install node`).

## Cleanup (legacy CI secrets)

If you previously configured GitHub environments for release automation, delete them after merging this flow:

```bash
gh secret delete RELEASE_PLEASE_TOKEN --env release --repo the-hcma/rusty-jack || true
gh secret delete RELEASE_PLEASE_TOKEN --env release-automation --repo the-hcma/rusty-jack || true
gh secret delete HOMEBREW_TAP_TOKEN --env release --repo the-hcma/rusty-jack || true
gh api -X DELETE "repos/the-hcma/rusty-jack/environments/release" || true
gh api -X DELETE "repos/the-hcma/rusty-jack/environments/release-automation" || true
```

These commands are safe to rerun; they no-op when the secret or environment is already gone.
