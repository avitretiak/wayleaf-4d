# Wayleaf 4D

Wayleaf 4D mirrors display colors to Nanoleaf 4D panels. Choose 1D static-color ambience (no screen capture), 2D single dominant screen color, 3D coarse colors from 10 edge zones, or 4D full per-panel edge mirroring. The Rust worker captures scanout through the installed GPU Screen Recorder (GSR) KMS helper (version 6.1.4) when the selected mode needs screen color, samples frames locally, and streams colors to the controller with Nanoleaf extControl v2 over UDP. It does not save screen images.

The Plasma 6 widget controls a user-level worker service. The `wayleaf-4d` CLI remains available for pairing, diagnostics, and standalone streaming.

Release notes: [0.2.0](docs/RELEASE_NOTES-0.2.0.md).

## Quick start

Requirements and source-build dependencies are listed in the [Implementation reference](docs/DEVELOPMENT.md). For a Plasma 6 Wayland session:

```sh
make
make install-user
```

Open **Wayleaf 4D** from the System Tray or add the widget to a panel with Plasma's **Add Widgets**. Pair through **Pair a Nanoleaf 4D**: discover it through Avahi or enter its IPv4 address, enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within 30 seconds:

```sh
wayleaf-4d --pair --ip 192.0.2.10
```

Choose a mode in the applet; mode and static color are saved with the shared settings. The monitor picker is not needed in 1D. In 2D–4D, the popup preview reflects the mode's output while open. Direct KMS capture in 2D–4D bypasses portal consent and KDE's Screen Casting indicator; no screen images are saved. The package-managed GSR helper accesses scanout without that prompt, so start sync only when you intend to send sampled display colors to the Nanoleaf. In 1D no screen capture is used. Installation does not enable or start the worker by itself. Run `wayleaf-4d --version` to check the worker build.

See the [User Guide](docs/USAGE.md) for settings, privacy, troubleshooting, and removal instructions.

## License

Copyright (C) 2026 Avi Tretiak. Wayleaf 4D is licensed under the GNU GPL, version 3 or (at your option) any later version. See [LICENSE](LICENSE). Report issues through the [GitHub issue tracker](https://github.com/avitretiak/wayleaf-4d/issues).
