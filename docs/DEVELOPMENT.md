# Implementation reference

## Architecture

```mermaid
flowchart LR
    UI[Plasma 6 QML widget] -->|session D-Bus| Control[ D-Bus-activated Rust control ]
    Control -->|user systemd toggle| Worker[wayleaf-4d worker]
    CLI[CLI pairing, diagnostics, and standalone streaming] --> Worker
    Control -->|session D-Bus| Worker
    Control -->|system D-Bus mDNS browse| Avahi[optional Avahi service]
    Config[Shared config] --> Control
    Config -->|mode, static_color, shared settings| Worker
    Device[Nanoleaf 4D API layout] -->|panel IDs and layout| Worker
    Worker -->|1D static color to all panel IDs| Device
    Helper[Installed GSR KMS helper] -->|KMS planes via private socket| Capture[EGL/GLES render to small RGBA frame]
    Capture --> Sample[2D dominant color / 3D coarse zones / 4D per-panel sampling]
    Sample -->|RGB values| Worker
    Worker -->|extControl v2 RGB UDP| Device
```

The control process handles settings and device management without capturing the screen. In modes 2D–4D, the worker requests the installed GPU Screen Recorder KMS helper (GSR 6.1.4); EGL/OpenGL ES on GBM composes supported planes and reduces them before CPU sampling. Mode 1D sends its configured static color without screen capture. Only the package-managed helper carries `CAP_SYS_ADMIN`; the worker and control process remain unprivileged. Direct KMS capture does not request portal consent or display KDE's Screen Casting indicator, so synchronization must be enabled deliberately.

DRM enumeration selects only connected connectors with an active CRTC. If the configured monitor is absent, KMS returns no primary plane, or the helper temporarily returns an empty successful response, the worker reports a temporary capture failure (exit status 75); the user service retries after 15 seconds. This lets display/KMS recovery resume sync without a widget toggle. Invalid or ambiguous monitor selections and protocol incompatibilities remain permanent startup errors.

The capture shader converts top-origin KMS source coordinates to the imported texture's Y convention after applying plane rotation. `glReadPixels` rows are then flipped once into top-down RGBA for sampling.

The worker assigns colors to discovered controller IDs according to the selected mode; sampling behavior and panel-layout geometry are detailed in [Sampling and panel layouts](#sampling-and-panel-layouts).

## Source map

| Path | Responsibility |
|---|---|
| `src/main.rs`, `src/cli.rs` | `wayleaf-4d` CLI dispatch, help/version, pairing, device info, monitor listing, diagnostics, and standalone streaming. |
| `src/config.rs` | Shared XDG config loading, validation, secure atomic saves, defaults, and device credential removal. |
| `src/device.rs` | Nanoleaf LAN API, pairing, device/layout parsing, extControl v2 UDP. |
| `src/kms.rs`, `src/capture.rs`, `src/gpu.rs` | Monitor selection, GSR KMS helper IPC, EGL/GLES/GBM plane composition and reduced RGBA capture. |
| `src/sampling.rs`, `src/runtime.rs` | Edge-zone mapping/color processing, worker D-Bus API, frame scheduling, capture diagnostics. |
| `src/control.rs` | D-Bus-activated control API, settings and service lifecycle, Avahi discovery, pairing/device actions. |
| `share/plasma/io.github.avitretiak.Wayleaf4D/` | Plasma applet metadata and QML UI. |
| `Makefile` | Builds/installs Rust binaries, checks packaged shader freshness, and generates the absolute-path D-Bus activation entry. |
| `share/systemd/user/wayleaf-4d.service` | User worker service managed by the widget toggle. |
| `share/applications/io.github.avitretiak.Wayleaf4D.desktop` | Hidden desktop identity installed with the applet. |
| `README.md` | User-facing overview, requirements, source installation, and first run. |
| `docs/USAGE.md` | Detailed controls, service behavior, configuration, CLI, diagnostics, and removal. |

QML uses Plasma's native asynchronous session-D-Bus interface. The control process owns service and device-management actions; the worker owns capture, sampling, frame scheduling, and UDP streaming. The CLI shares `Config` and can run diagnostics or a standalone stream.

The popup's hero card is 280×160 logical pixels with a 20 px outer margin, scaled by `Kirigami.Units.gridUnit / 16`. Equal-length 6 px segment pills have 4 px gaps and a 4 px inset. Live 4D pills use controller IDs and counts; without layout data, the placeholder has 9 top, 6 right, 13 bottom, and 6 left segments. Vertical positions are top-down; zone spans do not change pill lengths.

`GlowRing.qml` interpolates the sorted zone arrays into a 256×4 color texture. A rounded-rectangle distance shader draws an antialiased 6 px rim outside the card and a Gaussian-profile halo extending to 16 px. The halo uses a five-tap color kernel with a 4.5 px spread; the rim and pills use their zone colors.

Stopped previews use 30% opacity. Without live colors, 1D uses its static color and other modes use the theme's disabled-text color. Colors update without animation. Preview requests run only while the popup is open and stop without stopping synchronization.

Corner colors interpolate by angle around the rounded turn. The hero separator uses the theme text color at 20% opacity.

`SteppedSlider.qml` is the shared native Qt Quick Controls `Slider` used for mode and Look selection: values 0–3, unit steps, and `SnapAlways`, with a quarter-width selection pill and four labels. Clicks, dragging, keyboard arrows, and external value changes share the same binding. `FullRepresentation.qml` receives the backend and applet configuration explicitly from `main.qml`; it saves current Custom settings before applying a preset and persists Custom's selection identity separately. Preset matching uses displayed slider steps rather than exact `f32` equality. Only Custom exposes directly visible fine tuning; resetting numeric settings restores Balanced defaults without switching the Look back to Balanced. Strip alignment is in Device details, available from the footer or More options, and remains independent of Looks.

## Build and capture internals

User-facing prerequisites and source-install steps are in the [README](../README.md). The Makefile builds with the checked-in `Cargo.lock`; the worker links system libcurl and libdrm and dynamically loads `libEGL.so.1`, `libGLESv2.so.2`, and `libgbm.so.1`. No C++ or Qt toolchain is required for the normal worker build; precompiled `.qsb` shader packages are committed, so normal builds only check their freshness.

When editing `glow_ring.vert` or `glow_ring.frag`, regenerate both packages with `make shaders` using `/usr/lib/qt6/bin/qsb --qt6` from Qt 6 Shader Tools. `make` and `make install-user` fail with regeneration guidance if either package is missing or older than its source.

`make install-user` installs the worker, applet, user service, desktop identity, and D-Bus activation entry under user directories, then reloads systemd and D-Bus. It does not start or enable Wayleaf. It refuses to proceed while the legacy `nanoleaf-sync` worker or service is active; otherwise it disables and removes that legacy service and binaries while preserving the shared configuration. The Wayleaf binary is replaced atomically, so an already-running worker continues with its loaded code. Absolute `XDG_DATA_HOME` and `XDG_CONFIG_HOME` overrides are honored; unset or relative values use the standard home-directory defaults.

The tray renders the bundled `contents/ambient-light.svg` directly. Plasma metadata uses the application-ID icon name; installers register the artwork under `$USER_SHARE/pixmaps` or `/usr/share/pixmaps`, which KDE searches as an unthemed fallback after the selected icon theme.

Device URLs use local HTTP, and curl is built without its default TLS feature. Signal-hook provides signal flags without channel/iterator features. Zbus uses its default runtime and blocking API features.

## Verification

Run Rust unit tests, build the worker, and run its module self-tests:

```sh
cargo test --locked --target-dir build/cargo
make
build/wayleaf-4d --self-test
```

The QML regressions use isolated in-memory backend/configuration objects; they do not connect to the worker or change the lights. With Qt 6 Declarative's `qmltestrunner`, Kirigami, and the KDE desktop control style installed:

```sh
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
QT_QUICK_CONTROLS_STYLE=org.kde.desktop QT_STYLE_OVERRIDE=Breeze \
/usr/lib/qt6/bin/qmltestrunner -input tests/qml
```

These checks cover Look clicks/dragging/keyboard input, Custom-to-preset round-trips, popup recreation, `f32` preset matching, Custom-only fine-tune visibility, and alignment independence from presets/custom-look persistence. Alignment controls are exercised with keyboard input and their group reset. Balanced is the new-install and reset-all default; reset-all selects Balanced. Saturation is 1.25 for Balanced, 1.0 for Subtle, and 1.9 for Vivid. Saved legacy preset selections retain their old values unless reset. Software rendering is sufficient for state/input tests; shader appearance can be checked separately in a native Plasma/Qt Quick window.


## Sampling and panel layouts

Mode determines how sampled or static colors are assigned while controller panel IDs remain intact:

- **1D** uses `static_color` directly and sends that color to every discovered panel ID. It does not start screen capture.
- **2D** samples one dominant color from the entire selected display and repeats it for every panel ID.
- **3D** samples 10 coarse zones around the screen (3 top, 2 right, 3 bottom, 2 left); each zone color is assigned to the closest matching physical panel zone. Preview publication preserves that explicit edge ownership even when a side zone touches a top or bottom corner; nearest-border ties must not reclassify it.
- **4D** samples one screen-edge region per controller segment. Captured frames are downscaled to at most 320 pixels wide; edge depth controls how far inward each region reaches, and letterbox detection optionally limits sampling to the active picture.

`panelLayout/layout.numPanels` must match the length of `positionData`. Supported layouts contain 1–40 panels; zero panels, counts over 40, malformed entries, and duplicate IDs are rejected. The output contains exactly the reported panels. API coordinates use x rightwards and y upwards; sampling rectangles use normalized screen coordinates with x rightwards and y downwards. Panel IDs are opaque output addresses, not array indices.

The `positionData` sequence supplies traversal order. Coordinate changes determine horizontal or vertical runs and their nearest physical side. Corners belong to the incoming run; the first panel uses its outgoing step. Tangent-coordinate midpoints and extrapolated endpoint bounds determine sampling intervals. Separate runs on one edge occupy adjacent spans weighted by their coordinate extents, including when those runs contain overlapping reported coordinates. `Zone.edge` records physical side ownership for preview publication.

The calibrated 40-panel NL69 layout uses dedicated sampling rectangles, including corners 1.5 times the edge depth, when its ordered live coordinates match the reference. Other layouts use their reported geometry. Worker startup fetches the layout; setting changes rebuild zones from that worker's cached panels. A physical count or layout change requires a worker restart.

Alignment settings are `start_corner`, `reverse_direction`, and `perimeter_offset`. Default `bottom_center` selects anchor index zero and leaves the reported ID assignment unchanged. Other anchors use live corner positions; calibrated 40-panel anchors are 7, 15, 28, and 35. For rectangle index `i`, the output ID is `panels[(anchor + offset + direction*i).rem_euclid(n)].id`, where direction is +1 normally and −1 when reversed. Geometry and edge ownership stay fixed. Offsets validate in −39…39 and reduce modulo the current count when mapping. Alignment is independent of Look presets and saved Custom values. Legacy key migration and configuration compatibility rules are documented in the [User Guide](USAGE.md#configuration-and-security).

`GetPanelCount` is read-only on both worker and control D-Bus interfaces. The control method forwards the worker's cached startup count, or zero when unavailable; it is not a persisted setting and cannot be set through `SetSettings`. QML polls it independently of the mode-specific preview count.

4D zones use saturation-weighted averaging blended with their brightest-luma pixel. Feathering tapers weights inward; center blend optionally mixes the average RGB of the central half of the active picture (sampled every fourth pixel) into each zone. Black gating and brightness/color controls follow these mixes; smoothing and maximum step bound temporal changes. Controls that do not apply in the selected mode are hidden in the applet, but their saved values are retained.

## Configuration and runtime settings

The control process and worker use `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). The QML backend mirrors worker defaults and normalizes setting values; live changes cross session D-Bus. The worker validates updates and merges accepted setting keys into the latest config, preserving controller credentials and unrelated JSON. The shared schema includes `mode` (`1D`, `2D`, `3D`, `4D`) and validated `static_color` (`#RRGGBB`), defaulting to `4D` and `#ffffff`. Alignment uses the canonical `start_corner`, `reverse_direction`, and `perimeter_offset` keys; the read-only detected panel count is fetched from the worker and is not persisted.
