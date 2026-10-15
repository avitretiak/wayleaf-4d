# Wayleaf 4D 0.2.0

Wayleaf 4D now uses a Rust worker and Plasma 6 widget to mirror display colors to Nanoleaf 4D panels.

## Changes

- Added four output modes: 1D static color, 2D full-screen dominant color, 3D ten-zone edge sampling, and 4D per-panel edge sampling.
- Replaced portal/PipeWire capture with direct KMS capture through the GPU Screen Recorder helper. Frames are reduced and sampled in memory; screen images are not saved.
- Added the Plasma controls for mode selection, static colors, sampling presets and fine tuning, monitor selection, and a live mode-specific edge preview. Run/Stop is independent of Start at login.
- Kept controller panel IDs in the output mapping. The 34-panel layout uses controller `positionData`; the 40-panel NL69 layout uses calibrated perimeter geometry.
- Kept the shared XDG config and CLI for pairing, diagnostics, and standalone streaming.

## Requirements and privacy

- Screen-capture modes (2D–4D) require `/usr/bin/gsr-kms-server` from GPU Screen Recorder 6.1.4 and a working DRM/KMS, EGL, OpenGL ES 3, GBM, and libdrm stack. The package-managed helper carries `CAP_SYS_ADMIN`; the Wayleaf worker remains unprivileged.
- Direct KMS capture does not request portal consent and does not show KDE's Screen Casting indicator. Start synchronization only when you intend to send sampled display colors to the controller. 1D uses no screen capture.
- Building requires Rust and Cargo 1.89 or newer, GNU Make, and system libcurl/libdrm development support. Plasma 6 and a systemd user manager are required for the widget controls.
- Existing settings and controller credentials remain in `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`).
