# Releasing

Rusty Jack releases are managed by Release Please and published through the Homebrew tap `the-hcma/tap`.

## Overview

There are two release workflows:

- `Release Please` (`.github/workflows/release-please.yml`) runs on pushes to `main`. It opens or updates a release PR, then creates the GitHub release and tag when that release PR is merged.
- `Release` (`.github/workflows/release.yml`) runs on `v*` tags or manual dispatch. It is the repair/backfill path for an existing tag.

Both workflows publish Homebrew changes through a pull request in `the-hcma/homebrew-tap`; they do not push directly to protected `main`.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Create a protected GitHub environment named `release` in `the-hcma/rusty-jack`.
3. Restrict the `release` environment to trusted reviewers.
4. Add `RELEASE_PLEASE_TOKEN` as a `release` environment secret. This must be a fine-grained token with write access to `the-hcma/rusty-jack`; a real token is needed so Release Please PRs trigger required CI.
5. Add `HOMEBREW_TAP_TOKEN` as a `release` environment secret. This must have write access to `the-hcma/homebrew-tap`.
6. Ensure the tap allows auto-merge and has CI protecting `main`; tap formula updates are merged only after `Tap CI` passes.

Do not store release tokens as repository-level secrets. Repository secrets can be referenced by any workflow merged to `main`; environment secrets are only exposed after the protected environment is approved.

## Security Model

Release tokens are gated by the `release` environment:

- `Release Please` and `Release` jobs declare `environment: release`.
- A workflow run cannot access `RELEASE_PLEASE_TOKEN` or `HOMEBREW_TAP_TOKEN` until an allowed environment reviewer approves the job.
- Pull requests from forks do not receive these secrets.
- Changes to release workflows still have to pass branch protection and CODEOWNERS review before reaching `main`.
- Tap updates go through a protected pull request in `the-hcma/homebrew-tap`; the token cannot push directly to tap `main`.

If an unexpected release workflow is waiting for approval, reject it and inspect the workflow diff before approving any later run.

## Normal Release

1. Merge feature and fix PRs using conventional commit messages, for example `feat:`, `fix:`, or `docs:`.
2. Wait for the `Release Please` workflow on `main`.
3. Review the Release Please PR. It updates:
   - `Cargo.toml`
   - `Cargo.lock`
   - `CHANGELOG.md`
   - `.release-please-manifest.json`
4. Merge the Release Please PR after CI passes.

After the release PR merges, Release Please creates the GitHub release and tag. The same workflow then:

- validates that the release version matches `Cargo.toml`
- downloads the tag tarball and computes its SHA-256
- opens or updates a tap PR for `Formula/rusty-jack.rb`
- enables auto-merge for the tap PR after `Tap CI` passes

## Backfill Or Repair

Use the tag-driven `Release` workflow when a tag already exists or when tap publication needs to be retried.

From the Actions UI, run `Release` with:

```text
tag = v0.1.1
```

Or create a missing tag from local `main`:

```bash
git checkout main
git pull --ff-only
git tag -s v0.1.1 -m "v0.1.1"
git push origin v0.1.1
```

The workflow is idempotent: if the GitHub release exists and the formula is already current, it exits cleanly.

## Verify

Check the release:

```bash
gh release view v0.1.1 --repo the-hcma/rusty-jack
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

If tap publication fails, fix the cause in `the-hcma/homebrew-tap` or the `HOMEBREW_TAP_TOKEN`, then rerun the `Release` workflow with the same tag.
