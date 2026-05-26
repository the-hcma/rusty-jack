# Troubleshooting

Common issues when using Rusty Jack on macOS with HDMI/DP monitors, eqMac, launchd, and ScalarWebAPI device wake support.

## Volume keys do nothing on the monitor

**Cause:** The system default is a **physical HDMI/DP device** with no software volume scalar.

**Fix (today):**

1. Install [eqMac](https://eqmac.app).
2. Run `rusty-jack apply` or pick your monitor in `picker` — rusty-jack starts eqMac if it was installed but not running.
3. Confirm eqMac is the **system default** in Sound settings (or `rusty-jack status` shows a virtual default footer routing to your monitor).

**Long-term:** Rusty Jack Phase 7 virtual driver — see [IMPLEMENTATION_PLAN.md](../IMPLEMENTATION_PLAN.md).

---

## eqMac is installed but volume still wrong

1. **Quit and restart eqMac** — or let rusty-jack launch it via `apply` / HDMI `picker`.
2. Wait a few seconds after switch — config `volume` uses retries for eqMac reset races.
3. Check `rusty-jack status` — `volume` and `config volume` should align after a successful apply.
4. In eqMac UI, confirm the **physical output** matches your monitor (not built-in).

---

## `apply` says switched but I hear nothing

- Wrong monitor in config — run `list`, fix `preferred_device.monitor_name` or `uid`.
- eqMac routing target mismatch — set output inside eqMac to your HDMI device.
- Monitor input/source — select correct HDMI input on the display.

---

## `apply` / `status`: policy “no change”

Default output already matches preferred UID. Expected when already routed correctly.

To force re-apply volume, switch away and back, or temporarily change preferred in config.

---

## Picker: ZoomAudioDevice (or similar) does nothing

**Expected.** Zoom and other **app virtual** drivers are **not speaker outputs**. They appear **dimmed** and cannot be selected. Pick your HDMI, DisplayPort, or Built-in row instead.

---

## `picker` requires a TTY

Use `--index N` from `list` IDX column for scripts:

```bash
rusty-jack list
rusty-jack picker --index 2 --json
```

---

## eqMac warning: not installed

```
warning: eqMac is not installed; volume buttons cannot control HDMI/DisplayPort output.
  Download eqMac from https://eqmac.app to enable software volume control.
```

Download eqMac from https://eqmac.app, or accept fixed full-level HDMI until Phase 7. Routing to HDMI still works; only **software volume** is missing.

---

## Built-in speakers work; HDMI does not

Normal. Built-in has CoreAudio volume control. HDMI needs eqMac (interim) or Phase 7 driver.

---

## Daemon is not switching outputs

1. Run `rusty-jack status` and confirm the daemon block says `running`, and the config path and preferred device match what you expect.
2. Check `auto_switch`; if it is `false`, the daemon stays alive but does not enforce policy.
3. Confirm the LaunchAgent is running:

```bash
launchctl print "gui/$(id -u)/com.example.rusty-jack"
```

4. Check logs:

```bash
tail -n 100 "$HOME/Library/Logs/rusty-jack.stderr.log"
tail -n 100 "$HOME/Library/Logs/rusty-jack.stdout.log"
```

---

## ScalarWebAPI device does not wake

1. Confirm the selected Mac output matches `scalar_webapi.mac_output`.
2. Confirm the device is reachable by hostname/IP and has its network standby/wake option enabled.
3. Run `rusty-jack picker` and look for the ScalarWebAPI power-state note on the configured output.
4. Check daemon logs for wake errors or discovery warnings.

Rusty Jack uses ScalarWebAPI directly. `port` is only a fallback; SSDP discovery may find a different advertised port.

---

## Audio briefly falls back from ScalarWebAPI to internal speakers

The daemon only treats ScalarWebAPI failures as a reason to use fallback output when the Mac's network access fingerprint changed: active default interface, default gateway, or interface IP address. If that fingerprint is stable, the daemon keeps the ScalarWebAPI-backed Mac output selected and treats the failed ScalarWebAPI wake/status request as transient.

Check daemon logs for `selected ScalarWebAPI device is unreachable`. If those messages appear without a matching Wi-Fi/Ethernet or IP change, update Rusty Jack before tuning fallback configuration.

---

## `disable` vs `pause`

| Command | Use when |
|---------|----------|
| `disable` / `uninstall` | Removing rusty-jack from launchd entirely |
| `pause` | Temporarily stop auto-routing; you will `resume` later |

Neither stops eqMac — manage eqMac separately in System Settings or its app.

---

## Daemon still runs the old version after update

Pause the LaunchAgent before replacing the binary, then resume it:

```bash
rusty-jack pause
git pull
make install
rusty-jack upgrade
```

If `rusty-jack --help` shows the new version but the daemon logs do not, the plist may point at an old binary path. Run `rusty-jack upgrade` to regenerate `~/Library/LaunchAgents/com.example.rusty-jack.plist` and restart the daemon.

---

## Confirm which binary you are running

```bash
./target/release/rusty-jack --help
# rusty-jack 0.1.0 (commit 7855685)

git rev-parse --short HEAD
# should match commit in --help
```

---

## CI / development

```bash
make fmt     # must pass before push
make clippy
make test
```

Hardware-specific tests are `#[ignore]`; eqMac HAL test skips when driver not installed.

---

## Getting help

1. `rusty-jack status --json` — attach output (redact UIDs if needed).
2. `rusty-jack list --hdmi` — show monitor names.
3. Note macOS version, eqMac version, and whether eqMac app is running.

File issues: [github.com/the-hcma/rusty-jack](https://github.com/the-hcma/rusty-jack/issues).

---

Copyright (c) 2026 Henrique Andrade / thehcma.
