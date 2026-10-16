# Wayleaf 4D 0.3.4

Variable-length strip mapping and independent strip alignment controls.

## Changes

- Map the controller's reported strip length and `positionData` instead of nominal 34-panel or count-ratio geometry. Retain calibrated 40-panel rectangles only for the matching live NL69 layout; never pad a shortened strip.
- Add 4D **Strip alignment** controls for start corner, reverse direction, and signed panel offset, plus detected count and group reset. Look presets and Custom data do not include alignment.
- Persist canonical `start_corner`, `reverse_direction`, and `perimeter_offset` settings across JSON and D-Bus. Migrate valid legacy alignment values and retain them during unrelated partial saves.
- Publish preview edges from explicit physical geometry ownership rather than 34-panel index ranges. Refresh the API layout on worker startup; saved offsets wrap safely after a cut.
- Treat the strip as an open route: the first panel uses its outgoing segment, never an invented last-to-first connection.

## Verification

- Rust: 35 tests passed, release build and all module self-tests passed.
- Plasma QML: 10 checks passed, including keyboard alignment changes, preset independence, Custom-data exclusion, and group reset. Native rendering was inspected at 320-pixel width.
- An isolated D-Bus session with a loopback HTTP/UDP controller exercised the worker and control processes at 32, 3, and 1 panels, including fresh startup reads, count forwarding, settings persistence, reversal, and saved offsets.
- A read-only API capture confirmed the current 32-panel layout. An offline diagnostic generated from actual Rust rectangles was exercised in a browser, including ID wraparound and fullscreen; diagnostic artifacts are not shipped.
- The local `0.3.4-1` pacman package passed all 35 package tests and installed self-tests. Plasma and the worker were restarted; the user confirmed that physical-device testing worked.
