# Temporary agent validation — M2 HAL ad-hoc signing (PR #44)

> **Delete this file** after validation is recorded in the PR or issue.  
> **Branch:** `native-driver-hal-fix-wt`  
> **PR:** https://github.com/the-hcma/rusty-jack/pull/44  
> **Purpose:** Confirm whether ad-hoc–signed `RustyJack.driver` loads on an Apple Silicon Mac with default SIP, and whether eqMac restore works after smoke teardown.

---

## Agent instructions

You are validating rusty-jack **on the user’s M2 Mac** (not the Intel Monterey box used earlier). Follow every step below, capture command output verbatim, and summarize pass/fail against the **Expected results** section at the end.

**Do not** disable SIP or change boot-args unless the user explicitly asks for the optional experiment in §7.

**Hardware:** HDMI or DisplayPort monitor connected and visible in Sound settings / `rusty-jack list --hdmi`.

**If eqMac is installed:** tests will move its HAL driver aside; teardown must restore it.

---

## 0. Environment setup

```bash
# Clone or use existing checkout; must be on the PR branch
git fetch origin
git checkout native-driver-hal-fix-wt
git pull origin native-driver-hal-fix-wt
git log -1 --oneline

# Record machine context (paste all output in your report)
uname -m
sw_vers
csrutil status
which rustc && rustc --version
```

Record whether eqMac is installed:

```bash
test -d /Applications/eqMac.app && echo "eqMac: installed" || echo "eqMac: not installed"
ls -la /Library/Audio/Plug-Ins/HAL/ 2>/dev/null || true
```

---

## 1. Build and install

```bash
cd "$(git rev-parse --show-toplevel)"
make install
rusty-jack --help | head -5
rusty-jack driver --help
```

**Check:** `rusty-jack driver --help` lists **`restore-eq-mac`**.

```bash
make driver-bundle
BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver"
test -d "$BUNDLE" && echo "bundle ok: $BUNDLE"
codesign -dv --verbose=4 "$BUNDLE" 2>&1 | head -30
```

**Record:** signing authority on the built bundle (expect **adhoc** or no Team ID before `make sign-driver-bundle`).

---

## 2. Baseline — before HAL mutation

```bash
rusty-jack list --hdmi
rusty-jack list | grep -E 'Rusty Jack|eqMac|rusty-jack.driver' || true
pgrep -lf 'Core Audio Driver' || true
```

**Record:** whether virtual UID `com.the-hcma.rusty-jack.driver.output` already appears (should **not** before smoke).

---

## 3. HAL smoke test (ad-hoc bundle, mutates system audio)

Requires **sudo** when prompted.

```bash
export RUSTY_JACK_DRIVER_BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver"
export RUSTY_JACK_HAL_DRIVER_SMOKE=1

cargo test --test native_driver_hal_smoke native_driver_hal_smoke_install_virtual_output_and_passthrough_ring -- --ignored --nocapture
```

**During/after test, capture:**

```bash
# Installed HAL plugins
ls -la /Library/Audio/Plug-Ins/HAL/

# Virtual output present?
rusty-jack list | grep -E 'Rusty Jack|com.the-hcma.rusty-jack.driver' || echo "no virtual output in list"

# CoreAudio helper
pgrep -lf 'RustyJack' || echo "no RustyJack helper"

# AMFI / signing (last 5 minutes; adjust if log stream unavailable)
log show --predicate 'eventMessage CONTAINS "RustyJack" OR eventMessage CONTAINS "67050"' --last 5m 2>/dev/null | tail -40 \
  || log show --last 5m 2>/dev/null | grep -iE 'rustyjack|67050|amfid' | tail -40 \
  || echo "could not query unified log; check Console.app manually"
```

---

## 4. eqMac restore (if eqMac installed)

Run even if step 3 failed (teardown should have run; this double-checks).

```bash
rusty-jack driver restore-eq-mac
ls -la /Library/Audio/Plug-Ins/HAL/eqMac.driver/Contents/MacOS/ 2>&1
test ! -d /Library/Audio/Plug-Ins/HAL/RustyJack.driver && echo "RustyJack.driver removed" || echo "RustyJack.driver still present"
```

**Ask user** to open eqMac briefly (or report if app says driver missing).

---

## 5. Optional second test — passthrough engine (only if step 3 passed)

Skip if step 3 failed on virtual output timeout.

```bash
export RUSTY_JACK_DRIVER_BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver"
export RUSTY_JACK_HAL_DRIVER_SMOKE=1

cargo test --test native_driver_hal_smoke native_driver_hal_smoke_passthrough_engine_starts_on_physical_hdmi -- --ignored --nocapture
```

---

## 6. Developer ID comparison (only if cert exists on this Mac)

Skip entire section if `security find-identity -v -p codesigning` shows no `Developer ID Application`.

```bash
make sign-driver-bundle
codesign -dv --verbose=4 target/share/rusty-jack/RustyJack.driver 2>&1 | head -20

# Re-run install smoke only if user approves another HAL mutation:
# export RUSTY_JACK_DRIVER_BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver"
# export RUSTY_JACK_HAL_DRIVER_SMOKE=1
# cargo test --test native_driver_hal_smoke native_driver_hal_smoke_install_virtual_output_and_passthrough_ring -- --ignored --nocapture
```

---

## 7. Optional experiment — SIP / ad-hoc (user must opt in)

**Do not run** unless the user explicitly requests relaxing security on this test machine.

If they do:

1. Reboot to Recovery → `csrutil disable` (or user’s chosen policy) → reboot.
2. Repeat §3 only.
3. Report whether virtual output appeared vs §3 on default SIP.

---

## Expected results (default SIP, ad-hoc bundle)

| Check | Expected on stock M2 |
|-------|----------------------|
| `make install` / tests in CI sense | Build succeeds |
| `driver restore-eq-mac` in help | Present |
| Built bundle `codesign -dv` | Ad-hoc (`Signature=adhoc` or no TeamIdentifier) |
| Smoke test `native_driver_hal_smoke_install_...` | **Likely FAIL** — timeout waiting for virtual output |
| `rusty-jack list` after swap-in | **No** `com.the-hcma.rusty-jack.driver.output` |
| Console / `log show` | May show `amfid` / `signature not valid: -67050` for RustyJack |
| `pgrep` helper | May appear briefly or not at all |
| After teardown / `restore-eq-mac` | `eqMac.driver` present; RustyJack removed; eqMac app works if installed |
| Developer ID signed (§6) | Virtual output **may** appear — report actual outcome |

**Hypothesis under test:** Ad-hoc HAL signing is **not** sufficient on Apple Silicon with default SIP (same class of failure as Intel Monterey). Developer ID signing is required for real validation.

---

## Report template (paste into PR #44 comment)

```markdown
### M2 HAL validation (branch native-driver-hal-fix-wt @ <commit>)

**Machine:** <chip> / macOS <version> / `csrutil status` output
**eqMac:** installed | not installed

#### Ad-hoc bundle
- codesign authority: …
- smoke test: PASS | FAIL (error: …)
- virtual UID in list: yes | no
- AMFI / -67050 in logs: yes | no | not checked
- RustyJack.helper: seen | not seen

#### eqMac restore
- restore-eq-mac: …
- eqMac app OK: yes | no | not tested

#### Developer ID (if run)
- signed smoke: …

#### Conclusion
Ad-hoc sufficient on this M2 with default SIP: yes | no
Notes: …
```

---

## Cleanup

- Ensure `/Library/Audio/Plug-Ins/HAL/RustyJack.driver` is absent unless intentionally testing.
- Run `rusty-jack driver restore-eq-mac` if eqMac is installed.
- Delete this file in a follow-up commit after results are captured.
