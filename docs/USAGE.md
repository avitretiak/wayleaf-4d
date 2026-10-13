# Nanoleaf Sync

The desktop tray is the interactive entry point. `nanoleaf-sync` remains available for scripted or terminal use.

## Build and install

```sh
make
make install-user
```

Launch **Nanoleaf Sync** from the desktop application menu, or run `nanoleaf-sync-tray`. The tray starts stopped; choose **Start** to begin screen sync and **Stop** to end it. Closing the settings window leaves the tray available. **Quit** stops a worker started by that tray and leaves a separately launched CLI worker alone.

The settings window separates general picture controls from a **4D alignment** tab for start corner, perimeter offset, and direction; hover labels or controls for explanations. General settings include **Edge sample depth** (5–20%, default 15%), which controls how far inward from each screen edge colors are sampled. Older configs without `edge_depth` use the 15% default. Changes apply to a running worker; while stopped they are saved for its next start. Settings are stored at `$XDG_CONFIG_HOME/nanoleaf-sync/config.json` (normally `~/.config/nanoleaf-sync/config.json`).

## How sampling works

The capture is reduced to at most 320 pixels wide. Each Nanoleaf segment samples a fixed rectangle along its corresponding screen edge; **Edge sample depth** controls how far that rectangle reaches inward. A 34-segment layout divides the perimeter into edge regions; in a 40-segment layout, the dedicated corner regions extend 1.5 times as deep as the edge depth. The rectangles do not fade into the screen, and center-screen color does not currently contribute to the edge LEDs.

Within each rectangle, sampled pixels are averaged with extra weight for saturation. The result is then blended with the rectangle's brightest-luma pixel: the current peak blend is 35%. This preserves small highlights but can make tiny bright objects disproportionately visible. Black gating and brightness/color controls run after this blend; temporal smoothing and maximum step limit how quickly each segment follows the target.

## Pairing and CLI

Put the Nanoleaf 4D controller into API pairing mode, then run:

```sh
nanoleaf-sync --pair --ip DEVICE_IP
```

The saved token is used by both launch paths. Run `nanoleaf-sync --help` for command-line overrides and `nanoleaf-sync --self-test` for the local geometry, sampler, and UDP packet check.

Use `--edge-depth 5..20` to override the percentage from the CLI; 15% is the default.

## Capture

The worker requests a Wayland screen stream through the XDG Desktop Portal and consumes its PipeWire stream with GStreamer. The portal may request screen-sharing approval. It discovers the Nanoleaf segment count and IDs from the controller; this device reports 34, so each UDP frame carries 34 segments. Capture is reduced to an RGBA frame up to 320 pixels wide before sampling.

`nanoleaf-sync --capture-debug` samples for 10 seconds without enabling UDP streaming, saving screenshots, or writing settings. It prints per-frame GStreamer buffer PTS and arrival intervals, active crop, scene-cut flags, and each segment's processed target versus smoothed output, then timing/gap summaries. It uses the saved Nanoleaf credentials to read segment IDs; the XDG Desktop Portal may ask for screen-sharing approval. Redirect the text output when reporting an intermittent issue.
