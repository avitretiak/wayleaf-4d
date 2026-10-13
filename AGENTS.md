# Nanoleaf Sync — agent notes

## Goal

A small Linux program that mirrors Wayland screen-edge colours to Nanoleaf 4D panels. Keep the implementation local, low-latency, and minimal; the interactive tray and CLI share one worker and one settings file. The device's native ambient-mirroring effect is replaced by Nanoleaf extControl v2 streaming so panel order and screen alignment remain adjustable.

## Implementation map

- `nanoleaf-sync.cpp`: C++17 worker. Loads/saves config, pairs through the Nanoleaf LAN API, fetches the controller panel layout, requests screen capture through the XDG Desktop Portal, consumes its PipeWire stream with GStreamer, samples zones, and sends RGB UDP frames. It also owns a session D-Bus service for live tray settings and `--self-test`, `--capture-test`, and `--capture-debug`.
- `nanoleaf-sync-tray.cpp`: Qt 6 tray/settings UI. Starts and stops the worker, persists settings, and applies changes to a running worker over D-Bus.
- `docs/USAGE.md`: user-facing build, pairing, settings, capture, and diagnostics instructions.
- `Makefile`: builds both binaries and installs them to `~/.local/bin` plus the desktop entry.

Data path: controller API layout and config → portal ScreenCast session → PipeWire/GStreamer RGBA frames (scaled to at most 320 px wide) → active-picture/letterbox detection → per-panel edge zones → colour weighting, black gating, saturation/brightness, smoothing and bounded transitions → extControl v2 UDP frames (default port 60222).

`make_zones()` must preserve controller panel IDs. For 34 panels, derive each panel's edge interval from the controller's `positionData` coordinates; do not replace this with equal spacing or assume API IDs are ordered by screen position. The 40-panel NL69 path uses calibrated perimeter geometry and dedicated corner regions. Other supported counts use the generic perimeter mapping. Corner, offset, and direction affect the panel-to-zone alignment.

The sampler supports letterbox-aware active bounds, optional legacy midtone lift, OLED black gating (including strict black), saturation-weighted averaging, brightest-pixel blend, center-color blend, inward feathering, temporal smoothing, and a bounded per-frame colour step. Keep existing controls and their validation ranges consistent across config loading, D-Bus updates, tray widgets, and documentation.

## Settings and credentials

The worker's `Config` defaults and the tray's `defaults()` fallback must match. Current non-secret defaults: 30 FPS, smooth 0.35, black threshold 0.01, maximum step 12, brightness 1.5, saturation boost 1.5, edge depth 8%, center blend 0%, peak blend 15%, feather depth 5%, bottom-center corner, offset 0, forward direction, letterbox enabled, midtone lift disabled, and strict black disabled.

Config is stored at `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). It contains the controller token and portal restore token; the worker writes it with restrictive permissions. Never commit, print, or copy credential values into docs, tests, logs, or support output. Do not replace a user's existing config while changing defaults; missing keys fall back to `Config` defaults.

## Build and verification

Build both binaries with `make`; install with `make install-user`. Build dependencies are GLib/GIO, GStreamer app/video, JSON-GLib, libcurl, and Qt 6 Widgets/DBus as listed in the Makefile; do not add dependencies without need.

- Run `./nanoleaf-sync --self-test` for local sampler, geometry, alignment, and UDP packet checks; it needs no controller or screen capture.
- Run `./nanoleaf-sync --help` to verify CLI options.
- `--capture-test` exercises portal/PipeWire capture. `--capture-debug` samples for ten seconds without sending UDP frames or saving screen images; it requires controller credentials/layout and may prompt for screen-capture approval.
- Follow `docs/USAGE.md` for pairing and real-device checks. Do not restart an active stream merely to install code unless requested.

## Research references

The following local trees are ignored by Git and are research material, not build/runtime dependencies:

- `nanoleaf-research/nanoleafOpenApi/` — Nanoleaf LAN API, token pairing, and extControl endpoint notes. Upstream: <https://github.com/xuweiyi025-cloud/nanoleafOpenApi>.
- `nanoleaf-research/homebridge-nanoleaf-4d/` and `nanoleaf-research/ha-nanoleaf-4D/` — independent 4D controller API integrations. Upstream: <https://github.com/skirwan/homebridge-nanoleaf-4d> and <https://github.com/jonathanrobichaud4/ha-nanoleaf-4D>.
- `nanoleaf-research/lg-hue-sync/` — extControl v2 UDP packet structure, segment mapping, and perimeter sampling ideas. Upstream: <https://github.com/adeze/lg-hue-sync>.
- `references/Nanoleaf-Screen-Mirror-for-KDE/` — Wayland/KDE screen-mirroring reference; it targets USB strips, not the 4D network protocol. Upstream: <https://github.com/SpinGiantCRM/Nanoleaf-Screen-Mirror-for-KDE>.

Prefer this project's implementation and `docs/USAGE.md` as the source of truth. Consult references to answer a specific API/protocol question; do not copy unrelated platform or hardware assumptions into this app.
