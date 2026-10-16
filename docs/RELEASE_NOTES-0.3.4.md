# Wayleaf 4D 0.3.4

Variable-length strip mapping and independent strip alignment controls.

## Strip mapping

- The controller's reported count and `positionData` define the mapping for 1–40 panels. Each reported panel ID appears exactly once in the output.
- Sampling follows the strip's open route. Coordinate runs determine edge ownership and sampling bounds; separate runs on the same edge occupy adjacent spans.
- Matching 40-panel NL69 layouts use calibrated sampling rectangles and dedicated corner regions.
- Worker startup reads the controller layout. Restart the worker after cutting or recalibrating a strip.

## Strip alignment

- The 4D popup includes a read-only detected count, start-corner selection, reverse direction, signed perimeter offset, and group reset.
- Alignment is independent of Look presets and saved Custom settings.
- `start_corner`, `reverse_direction`, and `perimeter_offset` persist through the shared configuration and D-Bus settings API. Valid legacy alignment settings are converted on load.
- Offsets wrap modulo the detected count, including after a strip is shortened.

## Preview

- Physical panel zones carry explicit edge ownership for the preview.
- Alignment changes panel-ID assignments while sampling rectangles and their preview positions remain fixed.
