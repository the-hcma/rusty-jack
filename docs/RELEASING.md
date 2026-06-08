# Releasing

Rusty Jack releases are managed by Release Please and published through the Homebrew tap `the-hcma/tap`.

## Overview

There are two release workflows:

- `Release Please` (`.github/workflows/release-please.yml`) runs on pushes to `main`. It opens or updates a release PR, then creates the GitHub release and tag when that release PR is merged.
- `Release` (`.github/workflows/release.yml`) runs on `v*` tags or manual dispatch. It is the repair/backfill path for an existing tag.

Both workflows publish Homebrew changes through a pull request in `the-hcma/homebrew-tap`; they do not push directly to protected `main`.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Create a GitHub environment named `release` in `the-hcma/rusty-jack`.
3. Add `RELEASE_PLEASE_TOKEN` as a `release` environment secret. This must be a fine-grained token with write access to `the-hcma/rusty-jack`; a real token is needed so Release Please PRs trigger required CI.
4. Add `HOMEBREW_TAP_TOKEN` as a `release` environment secret. This must have write access to `the-hcma/homebrew-tap`.
5. Do **not** add required reviewers to the `release` environment. Release Please runs automatically on pushes to `main`; the Release Please PR is the human approval gate.
6. Ensure the tap allows auto-merge and has CI protecting `main`; tap formula updates are merged only after `Tap CI` passes.

Keep release tokens in the `release` environment rather than repository secrets. Workflow changes still have to pass branch protection and CODEOWNERS review before reaching `main`.

## Security Model

Release tokens live in the `release` environment without deployment protection rules:

- `Release Please` and `Release` jobs declare `environment: release` so they can read the PATs.
- Routine pushes to `main` do **not** wait for a separate workflow deployment approval.
- The Release Please PR (version bump, `CHANGELOG.md`, and release notes) is the human gate before a tag is created.
- Pull requests from forks do not receive these secrets.
- Tap updates go through a protected pull request in `the-hcma/homebrew-tap`; the token cannot push directly to tap `main`.

If an unexpected release workflow change reaches `main`, inspect the workflow diff before the next Release Please PR merges.

## Normal Release

1. Merge feature and fix PRs using conventional commit messages, for example `feat:`, `fix:`, or `docs:`.
2. Release Please runs automatically on the resulting push to `main` and opens or updates a release PR.
3. Review the Release Please PR. It updates:
   - `Cargo.toml`
   - `Cargo.lock`
   - `CHANGELOG.md`
   - `.release-please-manifest.json`
4. Merge the Release Please PR after CI passes.

After the release PR merges, Release Please creates the GitHub release and tag. The `Update Homebrew tap` job in the same workflow then:

- validates that the release version matches `Cargo.toml`
- downloads the tag tarball and computes its SHA-256
- opens or updates a tap PR for `Formula/rusty-jack.rb` from `make render-homebrew-formula`
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

If Release Please fails with `token` missing, confirm `RELEASE_PLEASE_TOKEN` is set on the `release` environment.

If tap publication fails, fix the cause in `the-hcma/homebrew-tap` or the `HOMEBREW_TAP_TOKEN`, then rerun the `Release` workflow with the same tag.
