# Wayleaf 4D 0.3.1

Maintenance release with no changes to synchronization behavior.

## Changes

- The applet uses the active Plasma icon theme's `video-display` icon.
- Nanoleaf API requests use local HTTP; the Rust curl binding has TLS disabled.
- Settings saves remove obsolete configuration keys while retaining unrelated saved values.
