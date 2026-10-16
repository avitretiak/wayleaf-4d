# Wayleaf 4D 0.3.3

System-themed control icons, branded app/tray artwork, and saturation presets.

## Changes

- Brightness uses the active system theme's symbolic glyph, rendered in the theme foreground.
- The device-details button uses the system theme's symbolic wired-network glyph.
- The bundled `ambient-light.svg` supplies the app/tray artwork. Metadata icon lookup uses the unthemed `pixmaps` fallback.
- Nanoleaf API requests use local HTTP. The Rust curl binding has TLS disabled; signal-hook uses its signal-flag support.
- Saturation is **0.8** for Subtle, **1.0** for Balanced, and **1.9** for Vivid. New-install and reset defaults match Balanced; saved settings and Custom values remain independent.
