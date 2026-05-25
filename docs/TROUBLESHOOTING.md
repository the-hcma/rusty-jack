# Troubleshooting

Common issues when using Rusty Jack on macOS with HDMI/DP monitors and eqMac.

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

## `disable` vs `pause`

| Command | Use when |
|---------|----------|
| `pause` | Temporarily stop auto-routing; you will `resume` later |
| `disable` | Removing rusty-jack from launchd entirely |

Neither stops eqMac — manage eqMac separately in System Settings or its app.

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

File issues: [github.com/thehcma/rusty-jack](https://github.com/thehcma/rusty-jack/issues).
