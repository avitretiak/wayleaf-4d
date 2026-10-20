# Release notes

For installation and requirements, see the [README](../README.md). Current controls and configuration are documented in the [User Guide](USAGE.md).

## 0.4.2 — 2026-10-20

- GPU capture accepts the `AB4H` half-float ABGR format produced by HDR/direct-scanout paths; its EGL DMA-BUF is imported and sampled as floating-point color, while PQ/HLG conversion remains metadata-driven.
- `--capture-debug` streams read-only capture statistics until Ctrl-C, so diagnostics can be started on demand and kept open through a display-mode transition.

## 0.4.1 — 2026-10-20

- Fine-tune slider tooltips now include the setting name, and label widths adapt to the popup, improving clarity without allowing long labels to crowd out the controls.

## 0.4.0 — 2026-10-20

- Screen-derived RGB transitions now use proportional vector limiting with the existing exponential response, actual timing at normal 1–10 Hz, and bounded advancement after long capture gaps.
- Near-black confirmation and a higher wake threshold resist threshold flicker; confirmed black fades to exact off while respecting Max change per update. Immediate black remains the instant-off override.
- Letterbox detection now requires persistent border/interior evidence, tolerates localized subtitle interference, and retains a confirmed crop through ambiguous dark frames.
- Packet output holds only a shared intensity anchor and re-quantizes current RGB differences on every update. This fixes retained tint and rounding-boundary color flashes for near-neutral as well as exactly neutral input, while retaining float response state, resolvable color, gradual intensity changes, and exact black/white.
- The `*ExtControl*` sentinel is not restored as a saved scene. Failure cleanup and service restart policy are unchanged.
- New-install brightness is now 1.0. Balanced and new-install/fine-tune reset defaults use Color enhancement 1.0, Transition speed 0.25, and Dark-scene cutoff 0.05. Brightness stays independent of Looks and fine-tune resets; existing saved values are preserved. The cutoff reflects the user's observed red/yellow flicker on gray below 0.05, not a universal flicker guarantee or a new minimum. Subtle/Vivid retain their 0.01 cutoff.
- Fine-tuning labels now describe each control's effect; configuration keys are unchanged. Confirmed-black packet output cannot brighten a channel by one RGB code while fading, including when Max change per update is lowered.

- Added an optional Custom spatial-gradient toggle for 3D–4D. It blends neighboring zones using geometry; it defaults off and is independent of Looks and strip alignment. Brightest-pixel blend slider tooltip/reset now use the 10% default.

## 0.3.5

- More options opens at its button anchor; Qt positions the menu against the available popup bounds.
- Strip alignment is available in Device details, independently of Looks.
- Custom fine-tuning controls are directly visible. Resetting all fine-tuning restores and selects Balanced defaults; Custom remains selectable.
- Saturation weighting and final saturation enhancement are smoothly attenuated below 15% normalized luma for more restrained chroma in dark content.
- Black-threshold gating is retained in 3D–4D (disabled in 2D); maximum step applies in 2D–4D, including across 2D scene cuts.
- Preset saturation is 1.0 for Subtle, 1.25 for Balanced, and 1.9 for Vivid. Existing saved named preset values retain their selection unless reset.
- Device details content now has consistent inset padding and spacing in its scroll area.
- A disconnected selected monitor or missing KMS primary plane is treated as temporary; systemd retries the worker after 15 seconds, allowing capture to resume when the display returns without toggling sync.
- Empty successful KMS responses are treated as temporary capture loss, so systemd retries synchronization after 15 seconds instead of leaving the requested sync stopped.

## 0.3.4

- Live strip mapping uses the controller's reported count and `positionData` for 1–40 panels, with each panel ID represented exactly once. Matching 40-panel NL69 layouts use calibrated geometry.
- The **Strip alignment** controls in Device details provide detected count, start corner, reverse direction, signed perimeter offset, and group reset. Device details is available from the footer or More options; alignment is independent of Look presets and Custom settings.
- Alignment settings persist through configuration and D-Bus; valid legacy values load into the canonical keys. Offsets wrap modulo the detected count, including after shortening a strip.
- Sampling follows the strip's open route. Coordinate runs determine sampling bounds and physical-edge ownership for the preview; alignment changes ID assignments without moving rectangles.
- Worker startup reads the controller layout. Restart the worker after cutting or recalibrating a strip.

## 0.3.3

- Brightness and device-details controls use symbolic system-theme icons.
- The bundled `ambient-light.svg` supplies app/tray artwork; metadata icon lookup uses the unthemed `pixmaps` fallback.
- Saturation presets are 1.0 for Subtle, 1.25 for Balanced, and 1.9 for Vivid. New-install and reset numeric defaults match Balanced.
- The Rust curl binding uses local HTTP without TLS support; signal-hook uses signal flags without channel/iterator features.

## 0.3.2

- The Plasma applet uses bundled `ambient-light.svg` artwork, with hicolor lookup for its metadata icon.
- Synchronization supports 1D static color, 2D full-screen color, 3D coarse zones, and 4D per-panel sampling.

## 0.3.1

- The applet uses the active Plasma icon theme's `video-display` icon.
- The Rust curl binding has TLS disabled for local HTTP API requests.
- Settings saves remove obsolete keys while retaining unrelated values.

## 0.3.0

- Mode and Look selection use four-position sliders with mouse and keyboard support. Brightness is available in every mode.
- Custom settings persist across preset changes and popup reopen. Resetting fine tuning keeps Custom selected.
- The edge preview uses equal-length segment pills and a shared zone-color glow, updating at up to 10 Hz while the popup is open.
- The 3D preview represents its ten coarse screen zones; the 34-panel mapping fits its bottom-edge runs to adjacent halves.
- Precompiled shader packages are bundled for normal builds.

## 0.2.0

- Rust worker and Plasma 6 controls support four output modes, static colors, Look presets, fine tuning, monitor selection, and a live preview.
- Direct KMS capture uses the GPU Screen Recorder helper in modes 2D–4D; 1D sends static color without capture. Frames are sampled in memory and are not saved.
- Output mapping retains controller panel IDs and supports coordinate-based 34-panel layouts and calibrated 40-panel NL69 geometry.
- Shared XDG configuration and CLI commands support pairing, diagnostics, and standalone streaming. Run/Stop and Start at login are independent.
