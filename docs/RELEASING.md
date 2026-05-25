# Releasing

Rusty Jack releases are managed by Release Please and publish to the Homebrew tap `the-hcma/tap`.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Create a fine-grained GitHub token with write access to `the-hcma/rusty-jack` and add it as `RELEASE_PLEASE_TOKEN`. Release Please needs a real token so the release PR it opens triggers required CI.
3. Create a fine-grained GitHub token with write access to `the-hcma/homebrew-tap`.
4. Add the tap token to `the-hcma/rusty-jack` as the repository secret `HOMEBREW_TAP_TOKEN`.

## Release A Version

1. Merge feature and fix PRs using conventional commit messages.
2. The `Release Please` workflow opens or updates a release PR on `main`.
3. Review and merge the release PR. It updates `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and `.release-please-manifest.json`.

When the release PR merges, Release Please creates the GitHub release and tag. The same workflow then:

- validates that the release version matches `Cargo.toml`
- downloads the tag tarball and computes its SHA-256
- updates `Formula/rusty-jack.rb` in `the-hcma/homebrew-tap`

## Manual Tag Release

The `Release` workflow remains available for backfills or manual repair from an existing tag:

```bash
git checkout main
git pull --ff-only
git tag -s v0.1.1 -m "v0.1.1"
git push origin v0.1.1
```

## Install From Brew

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

## Manual Tap Repair

If tap publication fails after the GitHub release is created, rerun `Release Please` from the Actions UI after fixing `HOMEBREW_TAP_TOKEN`, or rerun the tag-driven `Release` workflow with the same tag. Both tap updates are idempotent.
