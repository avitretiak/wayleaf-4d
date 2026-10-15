# Wayleaf 4D

Wayleaf 4D mirrors colors around a selected display to Nanoleaf 4D segments. Its Rust worker captures scanout through KMS using the installed GPU Screen Recorder (GSR) KMS helper (version 6.1.4), samples frames locally, and streams segment colors to the controller with Nanoleaf extControl v2 over UDP. It does not save screen images.

The Plasma 6 widget controls a user-level worker service. The `wayleaf-4d` CLI remains available for pairing, diagnostics, and standalone streaming.

## Quick start

Requirements and source-build dependencies are listed in [Development](docs/DEVELOPMENT.md). For a Plasma 6 Wayland session:

```sh
make
make install-user
```

Open **Wayleaf 4D** from the System Tray or add the widget to a panel with Plasma's **Add Widgets**. In its **Device** tab, discover the controller through Avahi or enter its IPv4 address. Enable **Settings → Connect to API** in the Nanoleaf app or hold the 4D power button for 5 seconds, then pair within 30 seconds:

```sh
wayleaf-4d --pair --ip 192.0.2.10
```

Turn on the widget and choose a display under **Main → Capture monitor**, then press **Apply monitor**. Capture uses direct KMS; it does not request portal consent or show KDE's Screen Casting indicator. The GSR helper runs with its package-managed `CAP_SYS_ADMIN` capability; the Wayleaf worker stays unprivileged. This grants capture without the portal's consent/indicator, so enable sync only when you intend to share the selected display's sampled colors with the Nanoleaf. Installation does not enable or start the worker by itself. Run `wayleaf-4d --version` to check the worker build.

See the [User Guide](docs/USAGE.md) for settings, privacy, troubleshooting, and removal instructions.

## License

Copyright (C) 2026 Avi Tretiak. Wayleaf 4D is licensed under the GNU GPL, version 3 or (at your option) any later version. See [LICENSE](LICENSE). Report issues through the [GitHub issue tracker](https://github.com/avitretiak/wayleaf-4d/issues).
