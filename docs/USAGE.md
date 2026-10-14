# Wayleaf 4D User Guide

Wayleaf 4D captures a selected Wayland display through the XDG Desktop Portal and PipeWire, samples colors near the picture edges, and sends per-segment RGB values to a Nanoleaf 4D controller using extControl v2. Frames are processed locally; the program does not save screen images.

## Requirements

- A Linux Wayland session with a working XDG Desktop Portal ScreenCast implementation and PipeWire.
- A Nanoleaf 4D reachable on the local network. Avahi mDNS discovery is optional; manual IPv4 entry remains available. API pairing must be enabled on the controller.
- Plasma 6 and a systemd user manager for the widget's service toggle. Build dependencies are listed in [Development](DEVELOPMENT.md).

## Install and first run

Build and install for the current user:

```sh
make
make install-user
```

`make install-user` installs the worker, Plasma applet and QML plugin, user service, and hidden portal desktop identity. It only reloads the systemd user unit files; it does not enable, start, stop, or restart the worker. Existing config and credentials are not replaced.

Find **Wayleaf 4D** in the System Tray under **Hardware**, or add the widget to a panel/desktop through Plasma's **Add Widgets**. The widget starts stopped.

The status dot is green while the worker reports `running`, red on an error, and muted otherwise. Since extControl uses one-way UDP, green means the worker successfully sent a frame locally; it does not confirm receipt by the panels.

Open its **Device** tab. Search the local network through Avahi or enter the controller's IPv4 address manually. Enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within the controller's 30-second pairing window. Turn sync off before pairing or disconnecting a device.

The widget stores the API token in the owner-only config file and displays device name, model, serial number, firmware, and address. **Disconnect** removes only the saved address and token from this computer; it does not revoke API access on the controller.

Terminal pairing is also available:

```sh
nanoleaf-sync --pair --ip 192.0.2.10
```

Replace `192.0.2.10` with the controller's address.

Turn on the widget and approve the portal's display selection. Closing the widget popup does not stop synchronization.

When the sync switch is on, use **Main → Change Capture…** to reopen the desktop portal and select a display again. The worker restarts with its saved restore token cleared; after approval, the portal saves a token for the new selection. Canceling leaves synchronization stopped with an error; turn the switch off and on to try again.

The System Tray entry can be shown or hidden from **Configure System Tray**. This does not change capture permission or stop the worker. KDE's separate **Screen Casting** indicator represents active portal capture; it should not be removed or hidden in a way that removes its consent/stop controls. To reduce clutter, set **Configure System Tray → Entries → Screen Casting → Show only in popup**.

## Start, stop, and login behavior

The switch controls the user service and remembered login state:

- **On** enables and starts `nanoleaf-sync.service`.
- **Off** stops the worker and disables the service.
- Logging out ends the graphical session. When enabled, the service starts at the next graphical login; when disabled, it stays off.

The service retries only failures the worker classifies as temporary: HTTP transport failures, HTTP 408/425/429 or 5xx responses, transient UDP network errors, and GStreamer stream failures after capture starts. Those exit with status 75 (`EX_TEMPFAIL`) and systemd retries after 15 seconds. Retries continue every 15 seconds without an attempt limit.

Portal denial/cancellation or closure, invalid configuration or API data, other HTTP statuses, missing GStreamer elements, non-network socket errors, and unexpected signals/crashes are not restarted. Fix the cause, then toggle sync off and on. Closing the KDE Screen Casting indicator ends the worker without reopening the consent flow. Prefer the widget switch to manual `systemctl enable/disable` so displayed and systemd state remain synchronized. To stream directly from a terminal instead, run `nanoleaf-sync` and stop it with Ctrl-C; do not run it while the service is active, since only one worker can own the Nanoleaf D-Bus service.

Check the service and current-boot logs with:

```sh
systemctl --user status nanoleaf-sync.service
journalctl --user -u nanoleaf-sync.service -b
systemctl --user is-enabled nanoleaf-sync.service
```

The last command reports whether the service is configured to start at login.

## Controls and defaults

The **Device** page searches local Nanoleaf mDNS records (Avahi) or accepts a manual IPv4 address, pairs, shows device details, and locally disconnects saved credentials. The **Main** page contains brightness, saturation, and response. **Tuning** contains sampling and image-handling controls. **Alignment** maps screen zones to the controller's segment order. Sliders also accept numeric entry.

| Control | Config key | Range | Default | Effect |
|---|---|---:|---:|---|
| Brightness | `brightness` | 0–2 | 1.5 | Scales final LED brightness; 1.0 is neutral. |
| Saturation boost | `saturation_boost` | 0–3 | 1.5 | Emphasizes saturated pixels. |
| Response | `smooth` | 0.05–1 | 0.35 | Higher values follow changes faster; lower values smooth more but add lag. |
| Capture/send rate | `fps` | 1–10 Hz | 10 Hz | The implementation caps the requested rate at 10 Hz; PipeWire may deliver fewer frames. |
| Maximum step | `max_step` | 1–255 | 12 | Caps each frame's color change; smaller steps respond more slowly. |
| Edge sample depth | `edge_depth` | 5–20% | 8% | Distance inward from the screen edge sampled by each zone. |
| Center color blend | `center_bleed` | 0–50% | 0% | Mixes the central picture's average RGB into each segment. |
| Brightest-pixel blend | `peak_mix` | 0–35% | 15% | Mixes the zone's brightest-luma pixel; reduce it if tiny highlights dominate. |
| Inward feather depth | `feather_depth` | 0–20% | 5% | Extends edge zones inward with weights tapering to zero; 0 disables feathering. |
| Black threshold | `black_threshold` | 0–0.1 | 0.01 | Suppresses near-black capture noise. |
| Letterbox detection | `letterbox` | On/off | On | Detects black bars and samples the active picture. |
| Midtone lift | `midtone_lift` | On/off | Off | Optional legacy lift for dim and midtone content. |
| Strict black | `strict_black` | On/off | Off | Sends exact black immediately instead of smoothing dark transitions. |

Alignment settings:

| Control | Config key | Range | Default | Effect |
|---|---|---:|---:|---|
| Start corner | `corner` | `bottom-center`, `bottom-left`, `top-left`, `top-right`, `bottom-right` | `bottom-center` | Selects the screen corner corresponding to the first segment. |
| Offset | `offset` | 0–39 | 0 | Rotates the mapping by a number of segments. |
| Reverse direction | `reverse` | On/off | Off | Reverses the segment order. |

These settings remap the existing panel IDs; they do not change the number of segments sent. Center blend, brightest-pixel blend, and feather depth are controlled in the widget or config file; the CLI does not provide per-run flags for those three settings.

## Configuration and security

The worker, widget, and CLI share `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). It contains the controller IP, API token, portal restore token, and settings. New settings use the documented defaults when older config files omit them. Saves retain unrelated JSON keys. The config file is owner-readable/writable; do not publish it, paste it into support requests, or commit it.

The Nanoleaf LAN API uses HTTP on port 16021, and the extControl stream uses UDP on the controller-reported port (60222 is the fallback). Keep the controller and host on a trusted local network; do not expose these services to the Internet. The token is sent to the controller as part of the local API request.

The widget's **Disconnect** action removes only `ip` and `token`; the portal restore token and settings remain. It does not revoke the token on the controller. Revoke API access through the Nanoleaf controller/app if required.

The portal grants capture access and supplies a restore token stored in the config. The app samples reduced frames in memory and sends only segment colors to the Nanoleaf; it does not write captured images. `--capture-debug` prints per-segment sampled RGB values, which can reveal colors/content from the screen; review the text before sharing it.

## CLI and diagnostics

Pairing and basic checks:

```sh
nanoleaf-sync --help
nanoleaf-sync --pair --ip 192.0.2.10
nanoleaf-sync --self-test
```

The self-test is local: it does not contact the controller or request screen capture.

| Option | Purpose |
|---|---|
| `--version` | Print the Wayleaf 4D version without loading configuration. |
| `--config PATH` | Use a different config file. |
| `--pair --ip HOST` | Pair with the controller and save its API token. |
| `--device-info` | Print an allowlisted set of device details using the saved API token; no token is printed. |
| `--ip HOST --token TOKEN` | Override the saved controller address/token for a CLI run. Avoid putting a real token in shell history or support output. |
| `--corner NAME` | Set the start corner; accepted names are listed above. |
| `--offset 0..39`, `--reverse`, `--forward` | Adjust screen-zone-to-segment order. |
| `--fps 1..10`, `--port 1..65535` | Set requested frame rate and fallback UDP port. A port returned by the controller takes precedence. |
| `--brightness 0..2`, `--saturation-boost 0..3`, `--smooth 0.05..1` | Override main color-response settings. |
| `--black-threshold 0..0.1`, `--max-step 1..255`, `--edge-depth 5..20%` | Override sampling and transition settings. |
| `--letterbox`, `--no-letterbox` | Enable or disable black-bar detection. |
| `--midtone-lift`, `--no-midtone-lift` | Enable or disable the optional legacy lift. |
| `--strict-black`, `--no-strict-black` | Enable or disable immediate exact-black output. |

Diagnostic modes:

- `--capture-test` briefly captures and reports frame count, dimensions, and measured rate. It requires portal access, but not controller credentials; it sends no UDP frames. The portal may prompt, and the returned restore token is saved to config.
- `--capture-debug` requires saved controller credentials and portal access. For ten seconds it reports frame timing, active crop, scene cuts, and per-segment target/output RGB values. It sends no UDP frames, saves no images, and does not write settings or a new restore token.

If the service stops on a permanent error, inspect `journalctl --user -u nanoleaf-sync.service -b`, fix the reported cause, then toggle the widget off and on.

## Sampling and panel layouts

Captured frames are downscaled to at most 320 pixels wide. Each controller segment samples a fixed rectangle on the corresponding screen edge; edge depth controls how far inward it reaches. Letterbox detection optionally limits sampling to the active picture.

For 34 segments, mapping uses the controller's `positionData` coordinates and preserves its panel IDs. For the 40-segment NL69 layout, the worker uses calibrated perimeter geometry with dedicated corner zones 1.5 times the edge depth. Other reported counts from 4 through 40 use the generic perimeter mapping. The count reported by the controller determines the number of LEDs sent.

Each zone uses saturation-weighted averaging blended with its brightest-luma pixel. Feathering tapers weights inward; center blend optionally mixes the average RGB of the central half of the active picture (sampled every fourth pixel) into each zone. Black gating and brightness/color controls follow these mixes; smoothing and maximum step bound temporal changes.

## Remove

Turn the widget off first; this stops streaming and disables login startup. Then remove the installed files (the config and credentials are intentionally kept):

```sh
systemctl --user disable --now nanoleaf-sync.service
rm -rf "$HOME/.local/share/plasma/plasmoids/io.github.avitretiak.Wayleaf4D"
rm -f "$HOME/.local/bin/nanoleaf-sync" \
  "$HOME/.local/share/applications/io.github.avitretiak.Wayleaf4D.desktop" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/nanoleaf-sync.service"
systemctl --user daemon-reload
```

To remove all local credentials, settings, and the portal restore token, remove `${XDG_CONFIG_HOME:-$HOME/.config}/nanoleaf-sync/`. Revoke the portal's ScreenCast permission separately in the desktop's privacy settings.
