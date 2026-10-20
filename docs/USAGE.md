# Wayleaf 4D User Guide

## Modes and capture behavior
Select a mode with the popup's four-position slider: click a label, drag the highlighted pill, or focus the slider and use Left/Right. The mode takes effect immediately and is saved in the shared settings. In **1D**, choose a color swatch or use the custom-color picker; the worker sends that static color to every controller panel ID without capturing the screen. The monitor picker shows **No screen capture needed**. Brightness applies in every mode.

**2D** samples one dominant color from the full selected screen and repeats it for every panel ID. **3D** samples 10 coarse zones around the screen (3 top, 2 right, 3 bottom, 2 left), then maps each zone color to the closest matching physical panel zone. **4D** samples each panel's screen-edge region using the controller's reported layout and panel IDs. The monitor picker applies immediately in 2D–4D.

While the popup is open, its edge preview displays the current output colors for the selected mode. When synchronization is off, the preview dims to 30% opacity; 1D keeps its static color, while modes without live data use the theme's disabled-text color. Preview updates are limited to at most 10 per second and stop while the popup is closed; closing it does not stop synchronization. The glow extends outside the hero card, but stays inside the popup window.

**Capture and privacy:** Direct KMS capture in 2D–4D bypasses portal consent and KDE's Screen Casting indicator. The package-managed GSR helper can access scanout without that prompt, so start sync only when you intend to send sampled display colors to the Nanoleaf. Frames are reduced and sampled in memory; screen images are not saved. 1D does not capture the screen. **HDR capture:** KMS planes with supported metadata use PQ (ST 2084) or HLG conversion and Rec.2020-to-Rec.709 tone mapping before producing the 8-bit RGB stream sent to the 4D. This is HDR-aware capture, not HDR output to the panels. Missing HDR metadata is treated as SDR; malformed or unsupported HDR metadata stops capture with an error rather than silently using SDR. Actual color accuracy depends on the monitor, driver, and KMS metadata.

## Start, stop, and login behavior

**Start/Stop** controls the current user-service session; **Start at login** controls only the remembered systemd enablement:

- **Start** starts `wayleaf-4d.service` without changing the login preference.
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
The popup keeps the sync switch and status pinned. **Brightness** sits immediately above the mode slider and remains available in every mode. **1D** shows static-color swatches and a multicolor custom-color picker; its center dot shows the active color and its **+** badge opens the picker. It does not show screen-capture or fine-tuning controls. **2D**, **3D**, and **4D** show a four-step Look slider (Subtle, Balanced, Vivid, Custom). The three presets hide fine tuning; selecting **Custom** makes its fine-tuning controls directly visible, without a disclosure. The section contains 4 settings in 2D, 8 in 3D, and 13 in 4D; only settings relevant to the selected mode are shown. 2D exposes Change limit; 3D exposes Change limit and Dark-scene cutoff. Ignore black bars and Instant black are exclusive to 4D. The monitor picker appears in 2D–4D and applies immediately. **Strip alignment** is in Device details, accessible from the footer or **More options**; it is independent of Looks.

The live preview frames the status, sync switch, brightness, and mode controls in a rounded card. Six-pixel segment pills with four-pixel gaps sit inside all four edges at a four-pixel inset, with equal lengths within each edge. In 4D, their counts and panel IDs follow the live mapped layout. The same zone colors blend into a continuous glow outside the card: an antialiased six-pixel bright rim and a softer, lightly color-blurred halo fading to nothing by sixteen logical pixels. A twenty-pixel popup margin leaves room for the glow without clipping its corners. Sizes scale with the desktop's Kirigami units.

**Subtle**, **Balanced**, and **Vivid** adjust shared color-response and sampling settings; Balanced matches the numeric defaults for the Look-controlled settings, including a dark-scene cutoff of **0.05**. Brightness is global, independent of Looks, and is not changed by a Look or fine-tuning reset. On the existing 0–2 brightness scale, neutral brightness **1.0** appears at 50% of the slider. The presets do not change mode, brightness, monitor, or toggles. Leaving **Custom** saves its active settings before applying a preset; returning to Custom restores them, including after the popup is reopened. Resetting all fine-tune settings restores the Balanced values while keeping **Custom** selected and fine tuning available. Fine-tune settings are shared across modes and persist even when hidden in the current mode. Existing saved named selections and settings are preserved unless reset.

The **Color enhancement** setting is **1.0** for Subtle, Balanced, and new-install/reset defaults, and **1.9** for Vivid. At or below 1, it changes the weighting of saturated pixels during sampling; above 1 it also boosts output saturation. It does not desaturate the final color below 1. The **Transition speed** default is **0.25** and the **Dark-scene cutoff** default is **0.05** for Balanced and new-install/reset values; Subtle and Vivid retain a cutoff of **0.01**. The cutoff uses low-sample confirmation and a higher wake threshold. The 0.05 default was selected after a report of red/yellow flicker on gray content with a lower cutoff; this is a user-observed tuning choice, not a universal calibration or guarantee against flicker. It does not impose a new minimum: existing saved lower values remain valid and are preserved. Saved Custom values are independent of the presets.

Screen modes use **Transition speed** and **Max change per update** to smooth and bound transitions, proportionally limiting each smoothed RGB change rather than clipping channels independently. Normal 1–10 Hz operation uses elapsed time; after a long capture gap, the first update advances gently instead of catching up in one jump. Small intensity changes use a shared 8-bit hold band to resist rounding chatter. Color differences are quantized relative to the current RGB maximum on every update, rather than held from an earlier packet, so tiny near-neutral differences do not become colored flashes around rounding boundaries.

In 3D–4D, output below the **Dark-scene cutoff** requires 150 ms of low samples before fading to exact off. Once off, it needs a signal above 1.25 times the cutoff to wake. The fade takes at least 300 ms and extends when necessary to respect **Max change per update**. **Immediate black** in 4D overrides confirmation and fading with immediate exact black. **Ignore black bars** requires repeated, consistent dark-border and contrasting-picture evidence for at least 500 ms; ambiguous dark frames retain the last confirmed crop.

**Neighbor blending** is a Custom fine-tuning toggle in 3D and 4D, independent of Look and strip alignment; 1D and 2D hide it while retaining its saved value. It defaults off (`spatial_blend: false`). When enabled, each zone receives a fixed 20% blend from geometrically neighboring zones using radius 0.35 and squared falloff `max(1 - distance/0.35, 0)^2`; a zone does not blend with itself. Exact-black receiving zones remain black, but black neighbors can darken lit zones. Blending uses an immutable snapshot of floating-point zone colors, with no temporal feedback, and quantizes only afterward. Geometry remapping is cached, so frame processing requires no additional allocations. The toggle is not affected by Look changes; its group reset and Reset all turn it off.

**Strip alignment** is available in Device details, accessed from the footer or **More options**, and is independent of Looks. The detected panel count is read-only and comes from the worker's startup layout; restart the worker after cutting or recalibrating a strip. “Unavailable” means no worker layout is available. Start corner selects the perimeter anchor, Reverse panel direction reverses ID traversal, and Perimeter offset shifts IDs by whole panels with wraparound. Bottom center with reverse off and offset zero leaves the reported ID assignment unchanged. Reset affects only alignment, not the Look or Custom data. Alignment persists across mode changes and determines physical panel ownership in 3D; 1D and 2D send uniform colors.

| Control | Config key | Range | Default | Applies to | Effect |
|---|---|---:|---:|---|---|
| Mode | `mode` | `1D`, `2D`, `3D`, `4D` | `4D` | All | Selects static color, full-screen dominant color, 10 coarse edge zones, or per-panel edge mirroring. |
| Static color | `static_color` | `#RRGGBB` | `#ffffff` | 1D | Color sent to every discovered panel ID without screen capture. |
| Brightness | `brightness` | 0–2 (displayed 0–100%) | 1.0 (displayed 50%) | All | Scales Wayleaf's output RGB; 1.0 is neutral. Independent of Looks and fine-tune resets, and separate from Nanoleaf controller global brightness (0–100%). |
| Color enhancement | `saturation_boost` | 0–3 | 1.0 | 2D–4D | Changes saturated-pixel weighting; above 1 also enhances output saturation. Values below 1 do not directly desaturate output; weighting and enhancement taper below 15% normalized luma. |
| Transition speed | `smooth` | 0.05–1 | 0.25 | 2D–4D | Higher values follow changes faster; lower values smooth more but add lag. |
| Update rate | `fps` | 1–10 Hz | 10 Hz | 2D–4D | Requested frame rate; the implementation caps it at 10 Hz. |
| Max change per update | `max_step` | 1–255 | 12 | 2D–4D | Time-normalized RGB-change ceiling; scales the whole transition together. Smaller values respond and fade to black more slowly. |
| Edge sample depth | `edge_depth` | 5–20% | 8% | 3D–4D | Distance inward from the screen edge sampled by each zone. |
| Center influence | `center_bleed` | 0–50% | 0% | 4D | Mixes the central picture's average RGB into each panel segment; higher values give it more influence. |
| Neighbor blending | `spatial_blend` | On/off | Off | 3D–4D | Fixed 20% encoded-RGB blend from neighboring zones using geometric distance; exact-black receivers stay black. |
| Highlight influence | `peak_mix` | 0–35% | 10% | 3D–4D | Mixes each sampled zone's brightest-luma pixel; higher values give that highlight more influence. |
| Edge feathering | `feather_depth` | 0–20% | 5% | 4D | Extends edge zones inward with weights tapering to zero; higher values reach farther inward; 0 disables feathering. |
| Dark-scene cutoff | `black_threshold` | 0–0.1 | 0.05 | 3D–4D | Confirms low-luma zones before fading them to black; waking requires 1.25× cutoff. Lower values allow dimmer content through; 0 disables cutoff hysteresis. The default was selected after a user report of red/yellow flicker on gray with a lower cutoff, not as a universal flicker guarantee or a new minimum; existing saved lower values remain valid. |
| Ignore black bars | `letterbox` | On/off | On | 4D | Samples the confirmed active picture; requires persistent border/content evidence and retains the crop during ambiguous dark scenes. |
| Midtone lift | `midtone_lift` | On/off | Off | 4D | Optional lift for dim and midtone content. |
| Immediate black | `strict_black` | On/off | Off | 4D | Sends exact black immediately instead of smoothing dark transitions. |
| Start corner | `start_corner` | `bottom_center`, `bottom_left`, `top_left`, `top_right`, `bottom_right` | `bottom_center` | 3D–4D | Selects the ID-permutation anchor; bottom center preserves the reported mapping. |
| Reverse panel direction | `reverse_direction` | On/off | Off | 3D–4D | Reverses ID traversal without changing sampled rectangles. |
| Perimeter offset | `perimeter_offset` | −39–39 panels | 0 | 3D–4D | Whole-panel ID shift, modulo the detected count; a saved offset remains safe after shortening. |

## Configuration and security

The worker, widget, and CLI share `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). Absolute `XDG_CONFIG_HOME` values are used; absent or relative values fall back to `~/.config`. The file contains the controller IP, API token, and settings. Mode defaults to `4D`; static color defaults to `#ffffff` and accepts `#RRGGBB`. Omitted settings use documented defaults.

For compatibility, legacy `corner` values such as `bottom-right` load as snake-case `start_corner` values. In-range integer `offset` (−39–39) and boolean `reverse` load as `perimeter_offset` and `reverse_direction`. Canonical keys take precedence individually; unrecognized legacy values use defaults. Settings saves write canonical alignment values and remove the old keys, including during unrelated partial updates. Unrelated JSON values are retained.

Configuration writes use owner-only read/write permissions (mode `0600`). The widget warns if an existing credential file is more permissive; it does not silently change its permissions. Do not publish the configuration, paste it into support requests, or commit it.

The Nanoleaf LAN API uses HTTP on port 16021, and the extControl stream uses UDP on the controller-reported port (60222 is the fallback). Keep the controller and host on a trusted local network; do not expose these services to the Internet. The token is sent to the controller as part of the local API request.

The widget's **Disconnect** action removes only `ip` and `token`; settings and other JSON keys remain unchanged. If sync is active, the action stops it and best-effort powers off the Nanoleaf first. A power-off failure is shown as a warning, but does not prevent local credential removal. Disconnect does not revoke the token on the controller. Revoke API access through the Nanoleaf controller/app if required.

In 2D–4D, the helper can access the selected display's KMS scanout without a portal grant, consent dialog, or KDE Screen Casting indicator. Only the separately installed, package-managed GSR helper carries `CAP_SYS_ADMIN`; the worker/control process is not elevated and Wayleaf does not change capabilities. Frames are reduced and sampled in memory; only segment colors are sent to the controller, and no screen images are saved. 1D sends the configured static color and does not capture. `--capture-debug` prints RGB channel statistics and per-segment values, which may disclose screen content in terminal logs; review that text before sharing it.

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
- `--capture-debug --monitor NAME` requires saved controller credentials (or `--ip` and `--token`) and a valid monitor. It reports post-GPU capture RGB statistics and per-segment `target>smoothed>encoded-packet` RGB values until interrupted with Ctrl-C. It sends no UDP frames, saves no images/settings, and does not use portal selection or save a restore token.

When the selected display is disconnected, KMS returns no primary plane, or the helper temporarily returns an empty successful response, the worker exits with temporary status 75 and systemd retries it after 15 seconds. This keeps the requested sync active; capture and output may pause until the display/KMS path recovers. Explicit Stop is not retried. For permanent errors, inspect `journalctl --user -u wayleaf-4d.service -b`, fix the reported cause, then use Run again. Use `wayleaf-4d --list-monitors` to find a monitor name and `--monitor NAME` for a standalone run or diagnostic.

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
  "$data_home/pixmaps/io.github.avitretiak.Wayleaf4D.svg" \
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

Wayleaf does not use portal ScreenCast permissions or restore tokens.
