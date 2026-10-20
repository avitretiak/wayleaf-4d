# Nanoleaf 4D and Wayleaf API parameters

## Scope and evidence

Read-only inspection on 2026-10-16: Nanoleaf NL69, firmware 12.4.1, hardware 1.2-1, reporting 32 panels. Controller state and limits were read through HTTP GET requests; Wayleaf's settings were enumerated through D-Bus `GetSettings`. Separately, at the user's request, global brightness was set to 100% via `PUT /state` and verified with a subsequent GET; no other state-setting requests were sent.

Three independent control layers:

- **Nanoleaf HTTP:** device power, global brightness, standalone colors, scenes, and native camera mirroring.
- **Nanoleaf UDP:** per-panel RGB targets and transition timing, used by Wayleaf.
- **Wayleaf D-Bus:** software sampling, color processing, and strip alignment.

Controller-reported ranges below are verified on this NL69. Scene/plugin limits and native camera commands are from the published OpenAPI; their write behavior was not tested on this controller.

## Nanoleaf HTTP lighting controls

All resource paths are relative to `http://<controller>:16021/api/v1/<token>`. Do not put actual tokens in documentation, logs, or shared command examples.

| Parameter | Resource | Controller-reported range | Function |
|---|---|---|---|
| Power | `/state/on` | Boolean | Device on/off |
| Global brightness | `/state/brightness` | 0–100% | Controller-level dimmer |
| Hue | `/state/hue` | 0–360 degrees | Standalone hue/saturation color mode |
| Saturation | `/state/sat` | 0–100% | Standalone color saturation |
| Color temperature | `/state/ct` | 1200–6500 K | Warm-to-cool white in color-temperature mode |
| Color mode | `/state/colorMode` | `hs`, `ct`, `effect` | Read-only active mode |
| Global orientation | `/panelLayout/globalOrientation` | 0–360 degrees | Controller layout orientation, separate from Wayleaf alignment |
| Selected scene | `/effects/select` | Name from `/effects/effectsList` | Stored effect selection |
| Panel layout | `/panelLayout/layout` | Read-only geometry | Reported panel count, IDs, and positions |

The inspected controller listed 12 stored scenes and selected `*ExtControl*`. State hue, saturation, and color temperature are not evidence of processing applied to the active RGB stream.

State changes use `PUT /state`, for example `{"sat":{"value":50}}`. Brightness, hue, saturation, and color temperature also support `increment`. Brightness supports optional `duration` in seconds; the published specification does not provide a universal maximum duration or independent increment range.

Hue, saturation, and color temperature are standalone color-mode controls, not documented stream-processing filters. Treat writes to them as potentially disruptive to external control. Mode-switching writes were not tested while Wayleaf was running.

### Global brightness

Global brightness is the controller's universal dimmer, separate from Wayleaf's software RGB multiplier. The initial read-only inspection reported **25%**. At the user's request, it was then set to **100%**; a fresh API read confirmed `{"value":100,"max":100,"min":0}`. This is live device state, not a default or permanent property.

Read:

```http
GET /api/v1/<token>/state/brightness
```

Response shape:

```json
{"value":100,"max":100,"min":0}
```

Set an absolute brightness, illustrated here at 100%:

```http
PUT /api/v1/<token>/state
Content-Type: application/json

{"brightness":{"value":100}}
```

An optional fade can be requested with `{"brightness":{"value":100,"duration":2}}`. Relative changes use, for example, `{"brightness":{"increment":-10}}`.

The published API describes global brightness as a universal multiplier. Setting it to 100% while Wayleaf was running returned HTTP 204; a subsequent read reported 100%. This verifies the write and readback, not the perceptual dimming curve or how hardware brightness combines with streamed RGB.

Wayleaf's applet brightness slider changes only the software `brightness` setting. It does not call the controller's `/state/brightness` setter. Controller-level 100% is maximum brightness, while Wayleaf `brightness=1.0` is a neutral software RGB multiplier; Wayleaf's UI maps its 0–2 range to 0–100%.

## White balance and color calibration

| Capability | Public API availability |
|---|---|
| Warm/cool white selection | `ct`, 1200–6500 K on the inspected NL69 |
| Temperature correction over arbitrary streamed RGB | No documented control found |
| Green–magenta tint correction | No documented control found |
| Independent red/green/blue calibration gains | No documented control found |
| User-adjustable gamma | No documented control found |
| Phone-app camera corner-calibration coordinates | No documented control found |

Color temperature selects a white-light color; it is not a white-balance filter applied to Wayleaf's RGB output. Absence from the public specification does not establish whether a private firmware interface exists.

The vendor describes automatic internal white balancing and an ignored white-channel field in the streaming protocol. That does not expose adjustable calibration or establish a separately controllable white LED on the NL69.

## UDP external control

Wayleaf activates `extControl` v2 through the effects API and sends RGB values to reported panel IDs.

| Parameter | Protocol range | Wayleaf behavior |
|---|---|---|
| Panel ID | 16-bit ID; only reported IDs are valid targets | Preserves API IDs |
| Red | 0–255 | Processed RGB |
| Green | 0–255 | Processed RGB |
| Blue | 0–255 | Processed RGB |
| White field | Byte present; documented as ignored | Sends zero |
| Transition time | 16-bit field in 100 ms units | Sends zero; smoothing is in software |
| Update rate | Published maximum 10 Hz | Maximum 10 Hz |
| Panel count | Device-reported layout | Supports 1–40 without padding |

The transition field represents 0–65535 at the wire level. This is field capacity, not a tested firmware acceptance limit. Multi-byte fields use big-endian encoding.

## Wayleaf D-Bus settings

The worker returns these 19 settings through `GetSettings`. Ranges are enforced by `src/config.rs`.

| Setting | Accepted range or values | Function |
|---|---|---|
| `brightness` | Float 0–2 | Software RGB multiplier; 1 is neutral |
| `saturation_boost` | Float 0–3 | Saturation-weighted sampling and additional color boost |
| `smooth` | Float 0.05–1 | Temporal response; higher follows changes faster |
| `max_step` | Integer 1–255 | Time-normalized RGB-change ceiling applied proportionally to the whole transition |
| `fps` | Integer 1–10 Hz | Capture/send rate; 1D sends at 10 Hz |
| `edge_depth` | Integer 5–20% | Screen-edge sampling depth |
| `feather_depth` | Integer 0–20% | Additional inward sampling with tapered weights |
| `center_bleed` | Integer 0–50% | Center-picture color blend |
| `peak_mix` | Integer 0–35% | Brightest sampled pixel blend |
| `black_threshold` | Float 0–0.1 | Near-black suppression threshold |
| `letterbox` | Boolean | Active-picture detection |
| `midtone_lift` | Boolean | Fixed midtone-lifting curve; no strength parameter |
| `strict_black` | Boolean | Immediate exact-black output |
| `mode` | `1D`, `2D`, `3D`, `4D` | Static, full-screen, coarse-zone, or per-panel output |
| `static_color` | `#RRGGBB` | 1D color |
| `start_corner` | `bottom_center`, `bottom_left`, `top_left`, `top_right`, `bottom_right` | Panel-ID alignment anchor |
| `reverse_direction` | Boolean | Reverse ID traversal |
| `perimeter_offset` | Integer −39–39 | ID shift modulo current panel count |
| `monitor` | Available monitor name; empty for automatic selection | Capture source; live changes use `SelectMonitor`, not `SetSettings` |

Floating-point settings must be finite. Integer settings require integer values. D-Bus transports the floating-point settings as doubles.

Mode applicability:

- **1D:** static color and brightness; no capture processing.
- **2D:** brightness, saturation, smoothing, maximum step, and frame rate.
- **3D:** additionally edge depth, black threshold, brightest-pixel blend, and physical alignment.
- **4D:** all sampling controls.

Alignment is independent of Look presets and Custom data. White balance, tint, per-channel gains, and adjustable gamma are not Wayleaf settings.

Connection address, authentication token, and fallback UDP port are separate configuration. The fallback port range is 1–65535, default 60222; it is not a `SetSettings` key.

### Saturation semantics

Wayleaf's `saturation_boost` is not the controller's `sat` percentage:

- Saturated-pixel weighting is `1 + saturation_boost * saturation² * low_light_strength`; the smoothstep strength rises from zero to one over encoded luma 0–0.15.
- The final HSV saturation-boost stage applies only above 1.
- Values below 1 reduce saturated-pixel weighting but do not directly desaturate the final color in HSV.
- Output saturation enhancement uses `1 + (saturation_boost - 1) * low_light_strength` and remains a no-op at or below 1. Threshold confirmation/fading, vector limiting, and 8-bit output hysteresis are separate stages; they add no new D-Bus settings.

Implementation: `src/sampling.rs`, `boost_saturation()` and pixel-weighting calculation. A temperature/tint or RGB-gain correction that preserves streaming would need to process RGB before packet encoding.

## Native camera mirroring

These controls operate Nanoleaf's native camera mirroring, not Wayleaf capture.

| Parameter | Documented values |
|---|---|
| `screenMirrorMode` | 1D = 6, 2D = 2, 3D = 3, 4D = 5 |
| `screenMirrorRhythmModeType` | Pulse = 0, Fireworks = 1 |
| `screenMirrorRhythmMode` | Setter/getter documented; value range not clearly specified |

Documented effects commands: `activateScreenMirror`, `getScreenMirrorMode`, `setScreenMirrorRhythmMode`, `setScreenMirrorRhythmModeType`, and `getScreenMirrorRhythmMode`. These were not invoked during inspection because they could interrupt external control.

## Scene and plugin parameters

These are global published plugin limits, not a verified option set for every NL69 scene. Plugins may expose subsets or narrower limits.

| Parameter | Published range |
|---|---|
| Palette hue | Integer 0–359 degrees |
| Palette saturation | 0–100% |
| Palette brightness | 0–100% |
| Palette size | Recommended maximum 20 colors |
| `transTime` | Integer 1–600, in tenths of a second |
| `delayTime` | Integer 0–600, in tenths of a second |
| `loop` | Boolean |
| `linDirection` | `left`, `right`, `up`, `down` |
| `radDirection` | `in`, `out` |
| `rotDirection` | `cw`, `ccw` |
| `nColorsPerFrame` | Integer 1–50, potentially narrowed by palette/plugin |
| `mainColorProb` | Float 0–100% |
| Legacy `flowFactor` | Nonnegative; no finite maximum specified |
| Legacy `explodeFactor` | 0–1 |
| Temporary-effect `duration` | Seconds; no universal bound specified |

Scene names, plugin UUIDs, plugin-specific options, and legacy `windowSize` have no single universal numeric range. Scene controls are not filters over Wayleaf's UDP stream.

## Sources

- [Nanoleaf Light Panels OpenAPI, including 4D screen mirroring](https://nanoleaf.atlassian.net/wiki/spaces/nlapid/pages/2789310530/Nanoleaf+Light+Panels+Open+API+Documentation): sections 3.2.3 (plugin options), 3.2.6 (custom/external control), 3.4 (global brightness), 5.3 (state), 5.5 (layout), and 5.9 (screen mirror).
- Controller HTTP GET responses: root state, `/state`, `/state/brightness`, `/state/ct`, `/state/sat`, and `/panelLayout/globalOrientation`.
- Worker D-Bus `GetSettings`; repository `src/config.rs`, `src/sampling.rs`, `src/device.rs`, and `src/runtime.rs`.
