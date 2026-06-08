# Releasing

Rusty Jack releases are managed by Release Please and published through the Homebrew tap `the-hcma/tap`.

## Overview

There are two release workflows:

- `Release Please` (`.github/workflows/release-please.yml`) runs on pushes to `main`. It opens or updates a release PR automatically, then waits for owner approval before creating the GitHub release and tag.
- `Release` (`.github/workflows/release.yml`) is the manual repair/backfill path for an existing tag.

Both workflows publish Homebrew changes through a pull request in `the-hcma/homebrew-tap`; they do not push directly to protected `main`.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Create GitHub environments in `the-hcma/rusty-jack`:
   - `release-automation` — no required reviewers.
   - `release` — required reviewer: `thehcma` only.
3. Add `RELEASE_PLEASE_TOKEN` to **both** environments. This must be a fine-grained token with write access to `the-hcma/rusty-jack`; a real token is needed so Release Please PRs trigger required CI.
4. Add `HOMEBREW_TAP_TOKEN` to the `release` environment only. It must have write access to `the-hcma/homebrew-tap`.
5. Ensure the tap allows auto-merge and has CI protecting `main`; tap formula updates are merged only after `Tap CI` passes.
6. Optional when collaborators exist: add a repository ruleset that restricts creation of `v*` tags. Because Release Please publishes with the owner PAT, configure bypass actors carefully so routine publish still works.

Keep release tokens in environments rather than repository secrets. Workflow changes require owner review through `CODEOWNERS`.

## Security Model

Release automation is split into prepare and publish:

| Stage | Job | Environment | Approval |
|-------|-----|-------------|----------|
| Prepare release PR | `release-pr` | `release-automation` | Automatic on pushes to `main` |
| Publish tag/release | `publish-release` | `release` | **Required reviewer: `thehcma`** |
| Update Homebrew tap | `update-homebrew-tap` | `release` | Same workflow approval as publish |

Additional controls:

- The Release Please PR (`release-please` label) is the content review gate for version bumps and changelog text.
- `publish-release` only runs when `Cargo.toml` is ahead of the latest published GitHub release.
- Direct `v*` tag pushes no longer trigger `release.yml`.
- Manual repair uses `Release` workflow dispatch only, which also requires the protected `release` environment.
- Release files in `CODEOWNERS` require `@thehcma` review.
- Dependabot auto-merge skips PRs labeled `release-please`.
- Do **not** add `merge-it` to Release Please PRs; merge them manually after review.

Pull requests from forks do not receive release environment secrets.

## Normal Release

1. Merge feature and fix PRs using conventional commit messages, for example `feat:`, `fix:`, or `docs:`.
2. Release Please runs automatically on the resulting push to `main` and opens or updates a release PR labeled `release-please`.
3. Review the Release Please PR. It updates:
   - `Cargo.toml`
   - `Cargo.lock`
   - `CHANGELOG.md`
   - `.release-please-manifest.json`
4. Merge the Release Please PR after CI passes.

After the release PR merges:

1. The workflow detects an unpublished `Cargo.toml` version.
2. GitHub prompts for approval of the `Publish release` job in the protected `release` environment.
3. After you approve, Release Please creates the GitHub release and tag.
4. The `Update Homebrew tap` job publishes the formula PR.

## Backfill Or Repair

Use the manual `Release` workflow when a tag already exists or when tap publication needs to be retried.

From the Actions UI, run `Release` with:

```text
tag = v0.1.1
```

Approve the pending `release` environment deployment when prompted.

The workflow is idempotent: if the GitHub release exists and the formula is already current, it exits cleanly.

Do not publish new versions by pushing tags locally or by editing release files on `main` outside the Release Please PR flow.

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

If the `release-pr` job fails with `token` missing, confirm `RELEASE_PLEASE_TOKEN` is set on the `release-automation` environment.

If publish waits forever, approve the pending deployment for the `release` environment in GitHub Actions.

If tap publication fails, fix the cause in `the-hcma/homebrew-tap` or the `HOMEBREW_TAP_TOKEN`, then rerun the `Release` workflow with the same tag.
