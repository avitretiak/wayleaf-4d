# Wayleaf 4D 0.3.1

Maintenance release with no changes to synchronization behavior.

## Changes

- Use the active Plasma icon theme's `video-display` icon for the applet instead of a bundled app-specific icon.
- Disable unused TLS support in the Rust curl binding; Nanoleaf API requests use local HTTP, so this removes the Rust OpenSSL dependencies without changing the supported API path.
- Consolidate legacy-setting cleanup without changing saved configuration behavior.
