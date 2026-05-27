# Signing the Rusty Jack HAL driver

CoreAudio loads HAL plugins from `/Library/Audio/Plug-Ins/HAL/`. On current macOS releases, **ad-hoc** signatures (`codesign -s -`) are often rejected by AMFI. Console shows:

```text
amfid: ... RustyJack ... signature not valid: -67050
```

The helper process `Core Audio Driver (RustyJack.driver)` may appear briefly, but the virtual output never shows up in `rusty-jack list` until the bundle is signed with a valid **Developer ID Application** certificate (and usually **notarized** for machines other than the signing Mac).

## Quick path (local iteration)

```bash
make driver-bundle
./scripts/sign-driver-bundle target/share/rusty-jack/RustyJack.driver
```

Optional: pin the identity explicitly:

```bash
export CODESIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
./scripts/sign-driver-bundle
```

Then install to the system HAL folder and restart CoreAudio:

```bash
sudo rm -rf /Library/Audio/Plug-Ins/HAL/RustyJack.driver
sudo cp -R target/share/rusty-jack/RustyJack.driver /Library/Audio/Plug-Ins/HAL/
sudo chown -R root:wheel /Library/Audio/Plug-Ins/HAL/RustyJack.driver
sudo killall -9 coreaudiod
```

Verify:

```bash
rusty-jack list
# expect UID com.the-hcma.rusty-jack.driver.output
```

## What the signing script does

1. Builds are produced by `scripts/build-driver-bundle` (ad-hoc sign for compile-only checks).
2. `scripts/sign-driver-bundle` re-signs with **Developer ID Application** + **timestamp**:
   - Inner binary `Contents/MacOS/RustyJack` first
   - Bundle root with `--deep`
3. Runs `codesign --verify` and optionally `spctl` (may still fail until notarized).

We intentionally **do not** pass `--options runtime` (hardened runtime). HAL audio server plugins are not standard app executables; hardened runtime is for notarized apps and commonly breaks plugin load.

## Notarization (distribution)

For binaries you ship to other Macs:

1. Zip the signed bundle: `ditto -c -k --keepParent RustyJack.driver RustyJack.driver.zip`
2. Submit: `xcrun notarytool submit RustyJack.driver.zip --keychain-profile <profile> --wait`
3. Staple: `xcrun stapler staple RustyJack.driver`
4. Copy the stapled bundle into your installer or `~/.cargo/share/rusty-jack/`.

Store notary credentials in the login keychain (`xcrun notarytool store-credentials`); never commit Apple IDs or app-specific passwords.

## TODO (when a Developer ID account is available)

- Add a `scripts/notarize-driver-bundle` helper (and `make notarize-driver-bundle`) that:
  - zips `RustyJack.driver` with `ditto`
  - submits via `xcrun notarytool submit --wait`
  - staples via `xcrun stapler staple`
  - verifies with `spctl` and `codesign --verify`
- Decide where the signed+stapled bundle is produced (e.g. `target/share/rusty-jack/` vs a separate `dist/` folder).
- If we want CI validation, wire a macOS job that **only verifies** signatures/notarization (no secret material in PRs) and keep signing/notarization on protected branches or via a manual release workflow.

## eqMac driver signing

eqMac ships a pre-signed `eqMac.driver` inside the app bundle. When Rusty Jack testing moves eqMac aside, restore with:

```bash
rusty-jack driver restore-eqmac
```

That copies from the managed backup when present, otherwise from `/Applications/eqMac.app/Contents/Resources/Embedded/eqMac.driver`, then restarts `coreaudiod`.

## Related

- [TROUBLESHOOTING.md](./TROUBLESHOOTING.md) — HAL smoke test, AMFI errors, eqMac restore
- `make validate-driver-bundle` — layout/plist checks (unsigned)
