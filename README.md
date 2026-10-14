# Wayleaf 4D

Wayleaf 4D mirrors colors around a selected Wayland display to Nanoleaf 4D segments. It captures through the XDG Desktop Portal and PipeWire, samples frames locally, and streams segment colors to the controller with Nanoleaf extControl v2 over UDP. Screen images are not saved.

The Plasma 6 widget controls a user-level worker service. The `nanoleaf-sync` CLI remains available for pairing, diagnostics, and standalone streaming.

## Quick start

Requirements and source-build dependencies are listed in [Development](docs/DEVELOPMENT.md). For a Plasma 6 Wayland session:

```sh
make
make install-user
```

Open **Wayleaf 4D** from the System Tray or add the widget to a panel with Plasma's **Add Widgets**. In its **Device** tab, discover the controller through Avahi or enter its IPv4 address. Enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within 30 seconds:

```sh
nanoleaf-sync --pair --ip 192.0.2.10
```

Turn on the widget and approve the portal's display selection. Installation does not enable or start the worker by itself. Run `nanoleaf-sync --version` to check the worker build.

See the [User Guide](docs/USAGE.md) for settings, privacy, troubleshooting, and removal instructions.

## License

Copyright (C) 2026 Avi Tretiak. Wayleaf 4D is licensed under the GNU GPL, version 3 or (at your option) any later version. See [LICENSE](LICENSE). Report issues through the [GitHub issue tracker](https://github.com/avitretiak/wayleaf-4d/issues).
