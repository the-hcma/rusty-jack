# Releasing

Rusty Jack releases are tag-driven and publish to the Homebrew tap `the-hcma/tap`.

## One-Time Setup

1. Create the public tap repository `the-hcma/homebrew-tap`.
2. Create a fine-grained GitHub token with write access to `the-hcma/homebrew-tap`.
3. Add that token to `the-hcma/rusty-jack` as the repository secret `HOMEBREW_TAP_TOKEN`.

## Release A Version

1. Update `Cargo.toml` and `Cargo.lock` to the new version.
2. Merge the release-prep PR to `main`.
3. Tag the release from `main`:

```bash
git checkout main
git pull --ff-only
git tag -s v0.1.1 -m "v0.1.1"
git push origin v0.1.1
```

The `Release` workflow then:

- validates that the tag matches `Cargo.toml`
- creates the GitHub release if it does not already exist
- downloads the tag tarball and computes its SHA-256
- updates `Formula/rusty-jack.rb` in `the-hcma/homebrew-tap`

## Install From Brew

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

## Manual Tap Repair

If the release workflow fails after the GitHub release is created, rerun it from the Actions UI with the same tag. It is idempotent and will update the formula if needed.
