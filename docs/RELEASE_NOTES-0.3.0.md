# Wayleaf 4D 0.3.0

Four-way Plasma controls, a mode-specific edge preview, and 34-panel edge mapping.

## Changes

- A native four-position mode slider supports clicking, dragging, and keyboard input. Brightness is available across modes.
- The Look slider provides Subtle, Balanced, Vivid, and Custom. Presets hide fine tuning; Custom restores its saved values and opens the controls.
- Custom settings persist across preset changes and popup reopen, including when Custom matches a preset. Resetting fine tuning keeps Custom selected.
- The edge preview uses equal-length 6 px segment pills, 4 px gaps, and a 4 px inset. Shared zone colors drive an antialiased 6 px rim and a soft Gaussian halo extending to 16 logical pixels. Preview updates at up to 10 Hz while the popup is open.
- The 3D preview shows 3 top, 2 right, 3 bottom, and 2 left coarse screen zones.
- The 34-panel layout fits its two bottom-edge runs to adjacent halves. Panel IDs are retained; the preview has 9 top, 6 right, 13 bottom, and 6 left segments.

## Requirements and privacy

- Screen-capture modes (2D–4D) require `/usr/bin/gsr-kms-server` from GPU Screen Recorder 6.1.4 and a working DRM/KMS, EGL, OpenGL ES 3, GBM, and libdrm stack. The package-managed helper carries `CAP_SYS_ADMIN`; the Wayleaf worker remains unprivileged.
- Direct KMS capture does not request portal consent and does not show KDE's Screen Casting indicator. Start synchronization only when you intend to send sampled display colors to the controller. 1D uses no screen capture.
- Building requires Rust and Cargo 1.89 or newer, GNU Make, and system libcurl/libdrm development support. Plasma 6 and a systemd user manager are required for the widget controls. Prebuilt shader packages are bundled; end users do not need a shader compiler.
- Existing settings and controller credentials remain in `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`).
