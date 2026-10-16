# Wayleaf 4D

Rust worker and KDE Plasma 6 applet for screen-derived color synchronization to Nanoleaf 4D panels.

## Commands

- Host checks: `cargo test --locked --target-dir build/cargo`
- Release build: `cargo build --release --locked --target-dir build/cargo`
- Project build and shader validation: `make` (runs `check-shaders` and builds the worker)
- Worker self-tests: `build/wayleaf-4d --self-test` (after `make`) or `build/cargo/release/wayleaf-4d --self-test` (after a direct Cargo release build).
- Plasma QML tests: `QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_QUICK_CONTROLS_STYLE=org.kde.desktop QT_STYLE_OVERRIDE=Breeze qmltestrunner -input tests/qml/`
- User install: `make install-user`; refuses to replace an active worker. Stop the worker first if installation is necessary.
- Package build: `makepkg --force --noconfirm` (Arch Linux; do not use `apt`, `dpkg`, `snap`, or `flatpak`).
- Release/package metadata must agree across `Cargo.toml`, `Cargo.lock`, `PKGBUILD`, `.SRCINFO`, applet metadata, README, and consolidated release notes.

Run the narrowest relevant checks during development and the applicable full project checks before delivery. For behavior or UI changes, exercise the actual path in addition to tests. Do not claim physical lighting verification unless observed on the hardware.

## Rules

- Preserve unrelated user changes. Do not stage, commit, push, publish, install, or alter hardware unless explicitly requested.
- Never expose Nanoleaf tokens, populated configuration, or other credentials in source, logs, docs, or support output. Use redacted examples such as `<token>`.
- Never issue a Nanoleaf state or streaming write unless the user explicitly requests the hardware action. Read-only API inspection is safe; changing light output is not implied by a build or test request.
- Keep the control process, worker, applet, and Nanoleaf device responsibilities distinct. The worker owns capture, sampling, packet output, and runtime settings; the control process owns pairing/device management; QML communicates through session D-Bus.
- Preserve API-reported panel IDs and layout. Do not assume 34 panels, pad short layouts, or infer physical calibration corrections without evidence.
- Keep strip alignment separate from color-look presets and custom-look data.
- Keep diagnostics and generated artifacts out of shipped packages unless explicitly requested.
- Do not add dependencies without approval. Follow existing Rust, QML/Kirigami, and configuration patterns; avoid unrelated refactors.
- Do not create branches, commits, or releases unless requested. Do not add co-authorship trailers.

## Device and API safety

- The Nanoleaf LAN API uses HTTP and authentication tokens; keep the controller on a trusted local network and never publish its token.
- Treat saturation/color-temperature HTTP writes and native Screen Mirror commands as potentially disruptive to external-control streaming.
- Wayleaf brightness is software RGB scaling. Nanoleaf global brightness is a separate controller setting; do not conflate them.
- Hardware output, service start/stop, installation, Plasma restart, and configuration changes are separate actions. Perform only the action requested and verify its result using readback where available.

## Pull-request review

- Reject changes that expose credentials, populated config files, or unredacted support output.
- Require Rust tests for behavior changes, QML tests for UI behavior changes, and worker self-tests where applicable.
- Mapping changes must preserve every reported panel ID exactly once, valid non-zero geometry, and output IDs under alignment permutations and panel-count changes.
- Verify configuration validation, serialization/migration, D-Bus transport, and that alignment remains independent of presets when those contracts change.
- Require package/release metadata and archive checks for packaging changes. A passing build alone does not establish safe physical output.

## Detailed guidance

- [User guide and supported behavior](docs/USAGE.md)
- [Implementation architecture and configuration](docs/DEVELOPMENT.md)
- [Consolidated release notes](docs/RELEASE_NOTES.md)
- [Nanoleaf API and parameter reference](.agents/nanoleaf-api.md)
