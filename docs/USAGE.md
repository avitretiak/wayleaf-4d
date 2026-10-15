# Wayleaf 4D User Guide

For requirements, installation, pairing, and a first-run overview, see the [README](../README.md). This guide covers detailed controls, service behavior, configuration, CLI diagnostics, troubleshooting, and removal. For architecture and implementation details, see the [Implementation reference](DEVELOPMENT.md).

## Modes and capture behavior
Select a mode in the popup's segmented control. The mode takes effect immediately and is saved in the shared settings. In **1D**, choose a color swatch or use the custom-color picker; the worker sends that static color to every discovered controller panel ID and does not create or use a screen-capture worker. The monitor picker is replaced by **No screen capture needed**. Brightness applies in every mode.

**2D** samples one dominant color from the full selected screen and repeats it for every panel ID. **3D** samples 10 coarse zones around the screen (3 top, 2 right, 3 bottom, 2 left), then maps each zone color to the closest matching physical panel zone. **4D** retains the full per-panel edge sampling behavior, including the controller's panel IDs and layout mapping. The monitor picker applies immediately in 2D–4D.

While the popup is open, its edge glow preview displays the current output colors and panel/zone arrangement for the selected mode; when synchronization is off, it is neutral. Preview updates are limited to at most 10 per second and stop while the popup is closed; closing it does not stop synchronization. The preview is drawn inside the popup and does not extend beyond its window.

**Capture and privacy:** Direct KMS capture in 2D–4D bypasses portal consent and KDE's Screen Casting indicator. The package-managed GSR helper can access scanout without that prompt, so start sync only when you intend to send sampled display colors to the Nanoleaf. Frames are reduced and sampled in memory; screen images are not saved. 1D does not capture the screen. **HDR capture:** KMS planes with supported metadata use PQ (ST 2084) or HLG conversion and Rec.2020-to-Rec.709 tone mapping before producing the 8-bit RGB stream sent to the 4D. This is HDR-aware capture, not HDR output to the panels. Missing HDR metadata is treated as SDR; malformed or unsupported HDR metadata stops capture with an error rather than silently using SDR. Actual color accuracy depends on the monitor, driver, and KMS metadata.

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

## Modes, controls, and defaults

The popup keeps the sync switch and status pinned. **Brightness** sits immediately above the mode selector and remains available in every mode. **1D** shows static-color swatches and a multicolor custom-color picker; its center dot shows the active color and its **+** badge opens the picker. It does not show screen-capture or fine-tuning controls. **2D**, **3D**, and **4D** show the Look control (Subtle, Balanced, Vivid, Custom) and a collapsible **Fine tune** section. The section contains 3 settings in 2D, 6 in 3D, and 12 in 4D; only settings relevant to the selected mode are shown. The monitor picker appears in 2D–4D and applies immediately.

The inset live-edge glow sits directly on the popup surface around the pinned status, brightness, and mode controls, with extra room between the controls and the LEDs instead of a separate card. It shows the current color across the output in 1D and 2D, the 10 coarse zones in 3D, or the device's full panel zones in 4D. It updates only while the popup is open, at up to 10 updates per second. The **Start/Stop** switch controls output in every mode; off stops synchronization and best-effort turns the lights off. **Start at login** is independent and lives in the overflow menu.

**Subtle**, **Balanced**, and **Vivid** adjust shared color-response and sampling settings; Balanced matches the default values. The presets do not change mode, brightness, monitor, or toggles. Editing a preset-controlled setting selects **Custom**; the custom values are saved and restored when Custom is selected. Other settings remain unchanged when choosing a preset. Fine-tune settings are shared across modes and persist even when hidden in the current mode.

| Control | Config key | Range | Default | Applies to | Effect |
|---|---|---:|---:|---|---|
| Mode | `mode` | `1D`, `2D`, `3D`, `4D` | `4D` | All | Selects static color, full-screen dominant color, 10 coarse edge zones, or per-panel edge mirroring. |
| Static color | `static_color` | `#RRGGBB` | `#ffffff` | 1D | Color sent to every discovered panel ID without screen capture. |
| Brightness | `brightness` | 0–2 (displayed 0–100%) | 1.5 (displayed 75%) | All | Scales final LED brightness; 1.0 is neutral. |
| Saturation boost | `saturation_boost` | 0–3 | 1.5 | 2D–4D | Emphasizes saturated sampled pixels. |
| Response | `smooth` | 0.05–1 | 0.35 | 2D–4D | Higher values follow changes faster; lower values smooth more but add lag. |
| Capture/send rate | `fps` | 1–10 Hz | 10 Hz | 2D–4D | Requested frame rate; the implementation caps it at 10 Hz. |
| Maximum step | `max_step` | 1–255 | 12 | 3D–4D | Caps each frame's color change; smaller steps respond more slowly. |
| Edge sample depth | `edge_depth` | 5–20% | 8% | 3D–4D | Distance inward from the screen edge sampled by each zone. |
| Center color blend | `center_bleed` | 0–50% | 0% | 4D | Mixes the central picture's average RGB into each panel segment. |
| Brightest-pixel blend | `peak_mix` | 0–35% | 15% | 3D–4D | Mixes each sampled zone's brightest-luma pixel; reduce it if tiny highlights dominate. |
| Inward feather depth | `feather_depth` | 0–20% | 5% | 4D | Extends edge zones inward with weights tapering to zero; 0 disables feathering. |
| Black threshold | `black_threshold` | 0–0.1 | 0.01 | 4D | Suppresses near-black capture noise. |
| Letterbox detection | `letterbox` | On/off | On | 4D | Detects black bars and samples the active picture. |
| Midtone lift | `midtone_lift` | On/off | Off | 4D | Optional lift for dim and midtone content. |
| Strict black | `strict_black` | On/off | Off | 4D | Sends exact black immediately instead of smoothing dark transitions. |

Center blend, brightest-pixel blend, and feather depth are also available through config or CLI flags `--center-bleed`, `--peak-mix`, and `--feather-depth`.

## Configuration and security

The worker, widget, and CLI share `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). Absolute `XDG_CONFIG_HOME` values are used; absent or relative values fall back to `~/.config`. The file contains the controller IP, API token, and settings, including the selected `mode` (`1D`, `2D`, `3D`, or `4D`) and validated `static_color` (`#RRGGBB`). Their defaults are `4D` and `#ffffff`; both are saved and restored with the other settings. Settings omitted by older config files use documented defaults. Settings saves discard obsolete `corner`, `offset`, and `reverse` keys while preserving unrelated JSON values, including a legacy portal restore-token key if one exists; that token is not used for capture. New writes use owner-only read/write permissions (mode `0600`); if an existing credential file is more permissive, the widget warns without silently changing its permissions. Do not publish it, paste it into support requests, or commit it.

The Nanoleaf LAN API uses HTTP on port 16021, and the extControl stream uses UDP on the controller-reported port (60222 is the fallback). Keep the controller and host on a trusted local network; do not expose these services to the Internet. The token is sent to the controller as part of the local API request.

The widget's **Disconnect** action removes only `ip` and `token`; settings and other JSON keys remain unchanged. If sync is active, the action stops it and best-effort powers off the Nanoleaf first. A power-off failure is shown as a warning, but does not prevent local credential removal. Disconnect does not revoke the token on the controller. Revoke API access through the Nanoleaf controller/app if required.

In 2D–4D, the helper can access the selected display's KMS scanout without a portal grant, consent dialog, or KDE Screen Casting indicator. Only the separately installed, package-managed GSR helper carries `CAP_SYS_ADMIN`; the worker/control process is not elevated and Wayleaf does not change capabilities. Frames are reduced and sampled in memory; only segment colors are sent to the controller, and no screen images are saved. 1D sends the configured static color and does not capture. `--capture-debug` prints sampled RGB values, which may disclose screen content in terminal logs; review that text before sharing it.

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

Panel mapping and sampling implementation details are in the [Implementation reference](DEVELOPMENT.md).

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
