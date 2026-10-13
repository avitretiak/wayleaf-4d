# Nanoleaf Sync

The desktop tray is the interactive entry point. `nanoleaf-sync` remains available for scripted or terminal use.

## Build and install

```sh
make
make install-user
```

Launch **Nanoleaf Sync** from the desktop application menu, or run `nanoleaf-sync-tray`. The tray starts stopped; choose **Start** to begin screen sync and **Stop** to end it. Closing the settings window leaves the tray available. **Quit** stops a worker started by that tray and leaves a separately launched CLI worker alone.

The settings window separates general picture controls from a **4D alignment** tab for start corner, perimeter offset, and direction. General settings include **Edge sample depth** (5–20%, default 8%), **Center color blend** (0–50%, default 0%), **Brightest-pixel blend** (0–35%, default 15%), and **Inward feather depth** (0–20%, default 5%). The tray applies changes to a running worker; while stopped, settings are saved for its next start. Settings live at `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`). Older configs missing these keys use the defaults.

## How sampling works

The capture is reduced to at most 320 pixels wide. Each Nanoleaf segment samples a fixed rectangle along its corresponding screen edge; **Edge sample depth** controls how far that rectangle reaches inward. A 34-segment layout uses the controller-reported `positionData` coordinates to place each segment along its edge; a 40-segment layout uses the calibrated NL69 perimeter geometry with dedicated corner regions 1.5 times as deep as the edge depth. **Inward feather depth** extends each region inward by that percentage of the active picture and smoothly tapers its pixel weights to zero; corner zones combine the horizontal and vertical tapers. Set it to 0 for the original hard boundary.

Pixels inside each zone are averaged with extra weight for saturation. The local average is blended with the zone's brightest-luma pixel using **Brightest-pixel blend**; feather weights also attenuate peak pixels in the fade area. Then **Center color blend** mixes in that share of the average RGB from the central half of the active picture (sampled every fourth pixel). At the default 0%, each zone uses its full local result; the center average still drives scene-cut detection. Black gating and brightness/color controls run after these blends; temporal smoothing and maximum step limit how quickly each segment follows the target.

## Pairing and CLI

Put the Nanoleaf 4D controller into API pairing mode, then run:

```sh
nanoleaf-sync --pair --ip DEVICE_IP
```

The saved token is used by both launch paths. Run `nanoleaf-sync --help` for command-line overrides and `nanoleaf-sync --self-test` for the local geometry, sampler, and UDP packet check.

Use `--edge-depth 5..20` to override the percentage from the CLI; 8% is the default.

## Capture

The worker requests a Wayland screen stream through the XDG Desktop Portal and consumes its PipeWire stream with GStreamer. The portal may request screen-sharing approval. It discovers the Nanoleaf segment count and IDs from the controller; this device reports 34, so each UDP frame carries 34 segments. Capture is reduced to an RGBA frame up to 320 pixels wide before sampling.

`nanoleaf-sync --capture-debug` samples for 10 seconds without enabling UDP streaming, saving screenshots, or writing settings. It prints per-frame GStreamer buffer PTS and arrival intervals, active crop, scene-cut flags, and each segment's processed target versus smoothed output, then timing/gap summaries. It uses the saved Nanoleaf credentials to read segment IDs; the XDG Desktop Portal may ask for screen-sharing approval. Redirect the text output when reporting an intermittent issue.
