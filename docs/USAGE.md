# Wayleaf 4D User Guide

Wayleaf 4D captures a selected display through direct KMS using the installed GPU Screen Recorder (GSR) KMS helper (version 6.1.4), samples reduced frames locally, and sends per-segment RGB values to a Nanoleaf 4D controller using extControl v2. It does not save screen images.

## Requirements

- Linux with DRM/KMS and working EGL, OpenGL ES 3, GBM, libdrm, and GPU drivers that support the display's scanout formats.
- The installed `/usr/bin/gsr-kms-server` helper from GPU Screen Recorder 6.1.4. The package-managed helper has `CAP_SYS_ADMIN`; Wayleaf's worker remains unprivileged. Do not set capabilities on Wayleaf or run its worker as root.
- A Nanoleaf 4D reachable on the local network. Avahi mDNS discovery is optional; manual IPv4 entry remains available. API pairing must be enabled on the controller.
- Plasma 6 and a systemd user manager for the widget's service toggle. Build dependencies are listed in [Development](DEVELOPMENT.md).

## Install and first run

Build and install for the current user:

```sh
make
make install-user
```

`make install-user` installs the worker, Plasma applet, user service, D-Bus activation entry, and hidden desktop identity. It refuses to continue while the legacy `nanoleaf-sync.service` or CLI worker is active; stop that old stream yourself, then retry. It removes only the inactive legacy service/binaries, reloads D-Bus activation and systemd's user units, and does not enable, start, stop, or restart Wayleaf 4D. The executable is replaced atomically, so installation does not interrupt a running stream; an already-running worker keeps its old code until its next start. Existing config and credentials are not replaced. Absolute `XDG_DATA_HOME` and `XDG_CONFIG_HOME` overrides are honored; absent or relative values use the standard home-directory defaults.

Open **Wayleaf 4D** from the System Tray or add the widget to a panel with Plasma's **Add Widgets**. Pair through **Pair a Nanoleaf 4D**: discover it through Avahi or enter its IPv4 address. Enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within 30 seconds.

The status dot is green while the worker reports `running`, red on an error, and muted otherwise. The Start/Stop switch controls the current sync session; **Start at login** is independent in the More options menu. Worker errors appear in the error banner. Since extControl uses one-way UDP, green means the worker successfully sent a frame locally; it does not confirm receipt by the panels.

When unpaired, use **Pair a Nanoleaf 4D** to search the local network or enter a manual IPv4 address. Device details and **Disconnect** are in **More options**. Disconnect removes the saved address and token from this computer; it does not revoke controller authorization.

Terminal pairing is also available:

```sh
wayleaf-4d --pair --ip 192.0.2.10
```

Replace `192.0.2.10` with the controller's address.

After pairing, choose a capture monitor; the selection applies immediately. Use the Start/Stop switch to begin or end synchronization. **More options → About capture** explains that direct KMS capture bypasses portal consent and KDE's Screen Casting indicator; screen images are not saved. The package-managed GSR helper can access scanout without that prompt, so start sync only when you intend to send sampled display colors to the Nanoleaf.
When the widget popup is open while sync is running, the LED-zone preview shows the sampled colors and controller IDs at up to 10 updates per second. Closing the popup disables preview updates; it does not stop synchronization.
**HDR capture:** KMS planes with supported metadata use PQ (ST 2084) or HLG conversion and Rec.2020-to-Rec.709 tone mapping before producing the 8-bit RGB stream sent to the 4D. This is HDR-aware capture, not HDR output to the panels. Missing HDR metadata is treated as SDR; malformed or unsupported HDR metadata stops capture with an error rather than silently using SDR. Actual color accuracy depends on the monitor, driver, and KMS metadata.

## Start, stop, and login behavior

**Start/Stop** controls the current user-service session; **Start at login** controls only the remembered systemd enablement:

- **Start** starts `wayleaf-4d.service` without changing the login preference.
- **Stop** stops the worker and then best-effort turns the Nanoleaf off. A power-off failure does not prevent the worker from stopping and appears as a warning. Stop does not change the login preference.
- **Start at login** can be changed without starting or stopping a currently running worker.
- When the enabled service's graphical session ends, systemd stops that run; the login preference remains in effect for the next session.

The worker exits with status 75 for failures it classifies as temporary; systemd retries that status after 15 seconds. Other errors are not retried. Fix the reported cause, then use Run again. Prefer the widget controls to manual `systemctl enable/disable` so displayed and systemd state remain synchronized. To stream directly from a terminal instead, run `wayleaf-4d --monitor NAME` (or use unambiguous auto-selection) and stop it with Ctrl-C; do not run it while the service is active, since only one worker can run at a time.

Check the service and current-boot logs with:

```sh
systemctl --user status wayleaf-4d.service
journalctl --user -u wayleaf-4d.service -b
systemctl --user is-enabled wayleaf-4d.service
```

The last command reports whether the service is configured to start at login.


## Controls and defaults

The main popup keeps the sync switch and **Brightness** visible. Brightness is shown as 0–100% of the available output range (50% is neutral; the default 1.5× setting appears as 75%). The **Main** tab contains the live preview, presets, monitor selection, pairing, and device actions. The **Fine tune** tab contains the remaining sampling and color controls in a scrollable panel. Use the reset icon beside each slider value to restore its default; click a value to enter it exactly. Keyboard arrows adjust the focused slider, Shift+wheel adjusts a slider, and normal wheel scrolling moves through the list.

**Subtle**, **Balanced**, and **Vivid** adjust only saturation, response, maximum step, edge depth, center blend, brightest-pixel blend, and capture/send rate. Balanced matches those settings' existing defaults. Fine-tuning one of them selects **Custom**; its last custom values are saved in the applet configuration and restored by **Custom**. Other controls and existing settings remain unchanged when choosing a preset.

| Control | Config key | Range | Default | Effect |
|---|---|---:|---:|---|
| Brightness | `brightness` | 0–2 (displayed 0–100%) | 1.5 (displayed 75%) | Scales final LED brightness; 1.0 is neutral. |
| Saturation boost | `saturation_boost` | 0–3 | 1.5 | Emphasizes saturated pixels. |
| Response | `smooth` | 0.05–1 | 0.35 | Higher values follow changes faster; lower values smooth more but add lag. |
| Capture/send rate | `fps` | 1–10 Hz | 10 Hz | Requested frame rate; the implementation caps it at 10 Hz. |
| Maximum step | `max_step` | 1–255 | 12 | Caps each frame's color change; smaller steps respond more slowly. |
| Edge sample depth | `edge_depth` | 5–20% | 8% | Distance inward from the screen edge sampled by each zone. |
| Center color blend | `center_bleed` | 0–50% | 0% | Mixes the central picture's average RGB into each segment. |
| Brightest-pixel blend | `peak_mix` | 0–35% | 15% | Mixes the zone's brightest-luma pixel; reduce it if tiny highlights dominate. |
| Inward feather depth | `feather_depth` | 0–20% | 5% | Extends edge zones inward with weights tapering to zero; 0 disables feathering. |
| Black threshold | `black_threshold` | 0–0.1 | 0.01 | Suppresses near-black capture noise. |
| Letterbox detection | `letterbox` | On/off | On | Detects black bars and samples the active picture. |
| Midtone lift | `midtone_lift` | On/off | Off | Optional legacy lift for dim and midtone content. |
| Strict black | `strict_black` | On/off | Off | Sends exact black immediately instead of smoothing dark transitions. |

Center blend, brightest-pixel blend, and feather depth can be set in the widget, config file, or CLI flags `--center-bleed`, `--peak-mix`, and `--feather-depth`.

## Configuration and security

The worker, widget, and CLI share `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). Absolute `XDG_CONFIG_HOME` values are used; absent or relative values fall back to `~/.config`. The file contains the controller IP, API token, and settings. New settings use the documented defaults when older config files omit them. Settings saves discard obsolete `corner`, `offset`, and `reverse` keys while preserving unrelated JSON values, including a legacy portal restore-token key if one exists; that token is not used for capture. New writes use owner-only read/write permissions (mode `0600`); if an existing credential file is more permissive, the widget warns without silently changing its permissions. Do not publish it, paste it into support requests, or commit it.

The Nanoleaf LAN API uses HTTP on port 16021, and the extControl stream uses UDP on the controller-reported port (60222 is the fallback). Keep the controller and host on a trusted local network; do not expose these services to the Internet. The token is sent to the controller as part of the local API request.

The widget's **Disconnect** action removes only `ip` and `token`; settings and other JSON keys remain unchanged. If sync is active, the action stops it and best-effort powers off the Nanoleaf first. A power-off failure is shown as a warning, but does not prevent local credential removal. Disconnect does not revoke the token on the controller. Revoke API access through the Nanoleaf controller/app if required.

The helper can access the selected display's KMS scanout without a portal grant, consent dialog, or KDE Screen Casting indicator. Only the separately installed, package-managed GSR helper carries `CAP_SYS_ADMIN`; the worker/control process is not elevated and Wayleaf does not change capabilities. Frames are reduced and sampled in memory; only segment colors are sent to the controller, and no screen images are saved. `--capture-debug` prints sampled RGB values, which may disclose screen content in terminal logs; review that text before sharing it.

## CLI and diagnostics

Pairing and basic checks:

```sh
wayleaf-4d --help
wayleaf-4d --pair --ip 192.0.2.10
wayleaf-4d --self-test
```

The self-test is local: it does not contact the controller or request screen capture.

| Option | Purpose |
|---|---|
| `--version` | Print the Wayleaf 4D version without loading configuration. |
| `--config PATH` | Use a different config file. |
| `--pair --ip HOST` | Pair with the controller and save its API token. |
| `--device-info` | Print an allowlisted set of device details using the saved API token; no token is printed. |
| `--ip HOST --token TOKEN` | Override the saved controller address/token for a CLI run. Avoid putting a real token in shell history or support output. |
| `--list-monitors` | Print available KMS capture monitors. |
| `--monitor NAME` | Select the capture monitor by name; without it, selection succeeds only when exactly one monitor is available. |
| `--fps 1..10`, `--port 1..65535` | Set requested frame rate and fallback UDP port. A port returned by the controller takes precedence. |
| `--brightness 0..2`, `--saturation-boost 0..3`, `--smooth 0.05..1` | Override main color-response settings. |
| `--black-threshold 0..0.1`, `--max-step 1..255`, `--edge-depth 5..20` | Override sampling and transition settings (edge depth is a percentage). |
| `--center-bleed 0..50`, `--peak-mix 0..35`, `--feather-depth 0..20` | Override center blend, brightest-pixel blend, and inward feathering percentages. |
| `--letterbox`, `--no-letterbox` | Enable or disable black-bar detection. |
| `--midtone-lift`, `--no-midtone-lift` | Enable or disable the optional lift. |
| `--strict-black`, `--no-strict-black` | Enable or disable immediate exact-black output. |

Diagnostic modes:

- `--capture-test --monitor NAME` captures for up to five seconds and reports frame count, dimensions, rate, and capture latency. It requires the GSR KMS helper and GPU stack, but not controller credentials; it sends no UDP frames and saves no images. Specify a monitor when more than one is available.
- `--capture-debug --monitor NAME` requires saved controller credentials (or `--ip` and `--token`) and a valid monitor. For ten seconds it reports frame timing, active crop, scene cuts, and per-segment target/output RGB values. It sends no UDP frames, saves no images/settings, and does not use portal selection or save a restore token.

If the service stops on a permanent error, inspect `journalctl --user -u wayleaf-4d.service -b`, fix the reported cause, then toggle the widget off and on. Use `wayleaf-4d --list-monitors` to find a monitor name and `--monitor NAME` for a standalone run or diagnostic.

## Sampling and panel layouts

Captured frames are downscaled to at most 320 pixels wide. Each controller segment samples a fixed rectangle on the corresponding screen edge; edge depth controls how far inward it reaches. Letterbox detection optionally limits sampling to the active picture.

For 34 segments, mapping uses the controller's `positionData` coordinates and preserves each panel ID. The 40-segment NL69 layout uses calibrated perimeter geometry with dedicated corner zones 1.5 times the edge depth. Other reported counts from 4 through 40 use the generic perimeter mapping. There is no corner, offset, or reverse-order remapping; the count reported by the controller determines the number of LEDs sent.

Each zone uses saturation-weighted averaging blended with its brightest-luma pixel. Feathering tapers weights inward; center blend optionally mixes the average RGB of the central half of the active picture (sampled every fourth pixel) into each zone. Black gating and brightness/color controls follow these mixes; smoothing and maximum step bound temporal changes.

## Remove

Stop sync using the widget first; explicit Stop best-effort turns off the Nanoleaf. If you do not want the service to start at the next login, turn **Start at login** off separately. Then remove the installed files (the config and credentials are intentionally kept):

```sh
data_home=${XDG_DATA_HOME:-}
case $data_home in /*) ;; *) data_home="$HOME/.local/share";; esac
config_home=${XDG_CONFIG_HOME:-}
case $config_home in /*) ;; *) config_home="$HOME/.config";; esac
systemctl --user disable wayleaf-4d.service
rm -rf "$data_home/plasma/plasmoids/io.github.avitretiak.Wayleaf4D"
rm -f "$HOME/.local/bin/wayleaf-4d" \
  "$data_home/icons/hicolor/scalable/apps/io.github.avitretiak.Wayleaf4D.svg" \
  "$data_home/applications/io.github.avitretiak.Wayleaf4D.desktop" \
  "$data_home/dbus-1/services/io.github.avitretiak.Wayleaf4D.Control.service" \
  "$config_home/systemd/user/wayleaf-4d.service"
systemctl --user daemon-reload
```

To remove all local credentials and settings, resolve the config directory the same way:

```sh
config_home=${XDG_CONFIG_HOME:-}
case $config_home in /*) ;; *) config_home="$HOME/.config";; esac
rm -rf "$config_home/nanoleaf-sync/"
```

A legacy portal restore-token value may be present there but is not used by current capture. No portal ScreenCast permission is used by Wayleaf.
