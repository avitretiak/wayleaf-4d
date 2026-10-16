# Wayleaf 4D 0.3.3

System-themed control icons, lighter Rust dependencies, and revised saturation presets. The bundled branded app/tray icon is preserved.

## Changes

- Use the active system theme's symbolic brightness glyph, masked to the theme foreground, instead of colored application-icon fallback artwork.
- Replace the unresolved device-button `plug` icon with the system theme's symbolic wired-network glyph.
- Register the branded metadata icon in the unthemed `pixmaps` fallback instead of hicolor, removing the direct `hicolor-icon-theme` dependency. User installation removes the previous user-local hicolor copy; pacman upgrades remove the old package-owned copy.
- Disable curl's unused default TLS feature and signal-hook's unused channel/iterator features. This removes `openssl-probe` and `openssl-sys` from the Rust dependency graph; system libcurl may still depend on OpenSSL. Zbus is unchanged.
- Set Subtle saturation to **0.8** and Balanced saturation to **1.0**. Vivid remains **1.9**. Rust defaults, QML fallback values, and reset defaults now match Balanced; existing saved settings and Custom values are preserved.

## Verification

- Full pacman-path installation, real-device HTTP and D-Bus Stop/Run, mode-specific previews, KMS capture, and graceful SIGTERM were exercised during release preparation.
- Dark and windowed light-theme appearance were checked on Plasma; symbolic brightness rendering was also checked for monochrome output.
