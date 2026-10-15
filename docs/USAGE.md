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

`make install-user` installs the worker, Plasma applet, user service, D-Bus activation entry, and hidden desktop identity. It refuses to continue while the legacy `nanoleaf-sync.service` or CLI worker is active; stop that old stream yourself, then retry. It removes only the inactive legacy service/binaries, reloads D-Bus activation and systemd's user units, and does not enable, start, stop, or restart Wayleaf 4D. Existing config and credentials are not replaced.

Find **Wayleaf 4D** in the System Tray under **Hardware**, or add the widget to a panel/desktop through Plasma's **Add Widgets**. The widget starts stopped.

The status dot is green while the worker reports `running`, red on an error, and muted otherwise. Worker errors appear in the error banner; rejected live settings include the worker's D-Bus error name and message. Since extControl uses one-way UDP, green means the worker successfully sent a frame locally; it does not confirm receipt by the panels.

Open its **Device** tab. Search the local network through Avahi or enter the controller's IPv4 address manually. Enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within the controller's 30-second pairing window. Turn sync off before pairing or disconnecting a device.

The widget stores the API token in the owner-only config file and displays device name, model, serial number, firmware, and address. **Disconnect** removes only the saved address and token from this computer; it does not revoke API access on the controller.

Terminal pairing is also available:

```sh
wayleaf-4d --pair --ip 192.0.2.10
```

Replace `192.0.2.10` with the controller's address.

Turn on the widget and choose a monitor in **Main → Capture monitor**, then press **Apply monitor**. If synchronization is enabled, applying a different monitor restarts the worker on that display. Capture starts directly through KMS; no portal consent is requested and KDE's Screen Casting indicator is not shown. This bypasses the portal's per-session consent and visible capture indicator. The package-managed GSR helper can access scanout without that prompt, so enable sync only when you intentionally want sampled display colors sent to the Nanoleaf. Turning the switch off stops capture.

## Start, stop, and login behavior

The switch controls the user service and remembered login state:

- **On** enables and starts `wayleaf-4d.service`.
- **Off** stops the worker and disables the service.
- Logging out ends the graphical session. When enabled, the service starts at the next graphical login; when disabled, it stays off.

The worker exits with status 75 for failures it classifies as temporary; systemd retries that status after 15 seconds. Other errors are not retried. Fix the reported cause, then toggle sync off and on. Prefer the widget switch to manual `systemctl enable/disable` so displayed and systemd state remain synchronized. To stream directly from a terminal instead, run `wayleaf-4d --monitor NAME` (or use unambiguous auto-selection) and stop it with Ctrl-C; do not run it while the service is active, since only one worker can run at a time.

Check the service and current-boot logs with:

```sh
systemctl --user status wayleaf-4d.service
journalctl --user -u wayleaf-4d.service -b
systemctl --user is-enabled wayleaf-4d.service
```

The last command reports whether the service is configured to start at login.

## Controls and defaults

The **Device** page searches local Nanoleaf mDNS records (Avahi) or accepts a manual IPv4 address, pairs, shows device details, and locally disconnects saved credentials. The **Main** page contains brightness, saturation, and response. **Tuning** contains sampling and image-handling controls. Sliders also accept numeric entry.

| Control | Config key | Range | Default | Effect |
|---|---|---:|---:|---|
| Brightness | `brightness` | 0–2 | 1.5 | Scales final LED brightness; 1.0 is neutral. |
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

The worker, widget, and CLI share `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). It contains the controller IP, API token, and settings. New settings use the documented defaults when older config files omit them. Settings saves discard obsolete `corner`, `offset`, and `reverse` keys while preserving unrelated JSON values, including a legacy portal restore-token key if one exists; that token is not used for capture. The config file is stored with owner-only read/write permissions (mode `0600`); do not publish it, paste it into support requests, or commit it.

The Nanoleaf LAN API uses HTTP on port 16021, and the extControl stream uses UDP on the controller-reported port (60222 is the fallback). Keep the controller and host on a trusted local network; do not expose these services to the Internet. The token is sent to the controller as part of the local API request.

The widget's **Disconnect** action removes only `ip` and `token`; settings and other JSON keys remain unchanged. Obsolete alignment keys are removed the next time settings are saved. Disconnect does not revoke the token on the controller. Revoke API access through the Nanoleaf controller/app if required.

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

Turn the widget off first; this stops streaming and disables login startup. Then remove the installed files (the config and credentials are intentionally kept):

```sh
systemctl --user disable --now wayleaf-4d.service
rm -rf "$HOME/.local/share/plasma/plasmoids/io.github.avitretiak.Wayleaf4D"
rm -f "$HOME/.local/bin/wayleaf-4d" \
  "$HOME/.local/share/applications/io.github.avitretiak.Wayleaf4D.desktop" \
  "$HOME/.local/share/dbus-1/services/io.github.avitretiak.Wayleaf4D.Control.service" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/wayleaf-4d.service"
systemctl --user daemon-reload
```

To remove all local credentials and settings, remove `${XDG_CONFIG_HOME:-$HOME/.config}/nanoleaf-sync/`. A legacy portal restore-token value may be present in that file but is not used by current capture. No portal ScreenCast permission is used by Wayleaf.
