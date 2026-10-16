# Wayleaf 4D 0.3.0

This release refreshes the Plasma popup's four-way controls and edge preview, and corrects 34-panel edge mapping.

## Changes

- Reworked the hero card with a native four-position mode slider: select a mode by clicking, dragging, or using the keyboard. Brightness remains available across modes.
- Replaced the Look buttons with a four-step Subtle / Balanced / Vivid / Custom slider. Presets hide fine tuning; Custom restores its saved values and opens the controls.
- Fixed custom settings being replaced by preset values. Custom-to-preset-to-Custom round-trips now preserve the snapshot across popup reopen, including when Custom happens to match a preset. Resetting fine tuning keeps Custom selected.
- Updated the edge preview with equal-length 6 px segment pills, 4 px gaps, and a 4 px inset. A shared set of zone colors drives the preview glow: an antialiased 6 px rim and a soft Gaussian halo fading out by 16 logical pixels. Preview updates at up to 10 Hz while the popup is open.
- Made the 3D preview show the 3 top, 2 right, 3 bottom, and 2 left coarse screen zones.
- Corrected the 34-panel bottom-edge split so its two controller-order runs fit their respective halves without gaps or overlap. Panel IDs are preserved; the preview uses the layout's 9 top, 6 right, 13 bottom, and 6 left counts.
- Refreshed the README screenshot with both sliders, rounded framing, and a soft window shadow on a transparent canvas. It is an illustrative rainbow preview, not a report of sampled hardware colors.

## Requirements and privacy

- Screen-capture modes (2D–4D) require `/usr/bin/gsr-kms-server` from GPU Screen Recorder 6.1.4 and a working DRM/KMS, EGL, OpenGL ES 3, GBM, and libdrm stack. The package-managed helper carries `CAP_SYS_ADMIN`; the Wayleaf worker remains unprivileged.
- Direct KMS capture does not request portal consent and does not show KDE's Screen Casting indicator. Start synchronization only when you intend to send sampled display colors to the controller. 1D uses no screen capture.
- Building requires Rust and Cargo 1.89 or newer, GNU Make, and system libcurl/libdrm development support. Plasma 6 and a systemd user manager are required for the widget controls. Prebuilt shader packages are bundled; end users do not need a shader compiler.
- Existing settings and controller credentials remain in `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`).
