use crate::capture::Capture;
use crate::device::{
    connect_udp, enable_stream, encode_packet, panel_layout, restore_effect, selected_effect,
    send_packet,
};
use crate::sampling::{
    colors_for_mode, make_zones, preview_zones_for_mode, zones_for_mode, Color, Sampler, Zone,
};
use crate::{AppError, Config, Result};
use serde::Serialize;
use serde_json::{Map, Number, Value};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::{OwnedValue, Value as DbusValue};
use zbus::{interface, DBusError};

const BUS_NAME: &str = "io.github.avitretiak.Wayleaf4D";
const OBJECT_PATH: &str = "/io/github/avitretiak/Wayleaf4D";
const MAX_CAPTURE_TEST: Duration = Duration::from_secs(5);
const CAPTURE_TEST_FRAMES: usize = 30;
const DEBUG_DURATION: Duration = Duration::from_secs(10);
const SIGNAL_POLL: Duration = Duration::from_millis(50);

struct WorkerState {
    config: Config,
    revision: u64,
    stopping: bool,
}

#[derive(Clone, Copy, Serialize)]
struct PreviewSegment {
    id: u16,
    edge: &'static str,
    position: f32,
    span: f32,
    r: u8,
    g: u8,
    b: u8,
}

fn preview_segment(
    zone: &Zone,
    color: crate::sampling::Color,
    edge_hint: Option<&'static str>,
) -> PreviewSegment {
    let width = zone.x1 - zone.x0;
    let height = zone.y1 - zone.y0;
    let center_x = (zone.x0 + zone.x1) * 0.5;
    let center_y = (zone.y0 + zone.y1) * 0.5;
    let (edge, position, span) = if let Some(edge) = edge_hint {
        match edge {
            "top" | "bottom" => (edge, center_x, width),
            "left" | "right" => (edge, center_y, height),
            _ => unreachable!(),
        }
    } else {
        let mut edge = ("top", center_x, width);
        let mut edge_distance = zone.y0;
        for (distance, candidate) in [
            (1.0 - zone.x1, ("right", center_y, height)),
            (1.0 - zone.y1, ("bottom", center_x, width)),
            (zone.x0, ("left", center_y, height)),
        ] {
            if distance < edge_distance {
                edge = candidate;
                edge_distance = distance;
            }
        }
        edge
    };
    let byte = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    PreviewSegment {
        id: zone.id,
        edge,
        position,
        span,
        r: byte(color.r),
        g: byte(color.g),
        b: byte(color.b),
    }
}

struct Shared {
    state: Mutex<WorkerState>,
    changed: Condvar,
    stop: Arc<AtomicBool>,
    sent: AtomicBool,
    preview_enabled: AtomicBool,
    panel_count: AtomicUsize,
    preview: Mutex<Vec<PreviewSegment>>,
}

impl Shared {
    fn new(config: Config) -> Self {
        Self {
            state: Mutex::new(WorkerState {
                config,
                revision: 0,
                stopping: false,
            }),
            changed: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            sent: AtomicBool::new(false),
            preview_enabled: AtomicBool::new(false),
            panel_count: AtomicUsize::new(0),
            preview: Mutex::new(Vec::new()),
        }
    }

    fn preview_json(&self) -> String {
        if !self.preview_enabled.load(Ordering::Acquire) {
            return "[]".into();
        }
        let preview = self
            .preview
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        serde_json::to_string(&*preview).unwrap_or_else(|_| "[]".into())
    }

    fn update_preview(&self, mode: &str, zones: &[Zone], colors: &[crate::sampling::Color]) {
        if !self.preview_enabled.load(Ordering::Acquire) {
            return;
        }
        let mut preview = self
            .preview
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        preview.clear();
        if zones.len() != colors.len() {
            return;
        }
        preview.reserve(zones.len());
        preview.extend(
            zones
                .iter()
                .zip(colors)
                .enumerate()
                .map(|(index, (zone, color))| {
                    // Coarse zones have explicit edge ownership; their corner
                    // bounds also touch adjacent edges, so distance ties misclassify them.
                    let edge_hint = if mode == "3D" && zones.len() == 10 {
                        Some(match index {
                            0..=2 => "top",
                            3..=4 => "right",
                            5..=7 => "bottom",
                            8..=9 => "left",
                            _ => unreachable!(),
                        })
                    } else if mode == "4D" {
                        zone.edge
                    } else {
                        None
                    };
                    preview_segment(zone, *color, edge_hint)
                }),
        );
    }

    fn set_preview_enabled(&self, enabled: bool) {
        self.preview_enabled.store(enabled, Ordering::Release);
        if !enabled {
            self.preview
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, WorkerState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn settings_json(&self) -> Value {
        self.lock().config.settings_json()
    }
    fn updated_config(&self, revision: u64) -> Option<(u64, Config)> {
        let state = self.lock();
        (state.revision != revision).then(|| (state.revision, state.config.clone()))
    }

    fn request_stop(&self) {
        let mut state = self.lock();
        state.stopping = true;
        self.stop.store(true, Ordering::Release);
        self.changed.notify_all();
    }

    fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn wait_until(&self, revision: u64, deadline: Instant) -> bool {
        loop {
            if self.is_stopping() {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            let state = self.lock();
            if state.stopping || self.is_stopping() {
                return false;
            }
            if state.revision != revision {
                return true;
            }
            let timeout = deadline.saturating_duration_since(now).min(SIGNAL_POLL);
            let (state, _) = self
                .changed
                .wait_timeout(state, timeout)
                .unwrap_or_else(|error| error.into_inner());
            if state.stopping || self.is_stopping() {
                return false;
            }
            if state.revision != revision {
                return true;
            }
        }
    }
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "io.github.avitretiak.Wayleaf4D")]
enum WorkerError {
    InvalidSettings(String),
    Failed(String),
}

type WorkerResult<T> = std::result::Result<T, WorkerError>;

struct WorkerInterface {
    shared: Arc<Shared>,
}

#[interface(name = "io.github.avitretiak.Wayleaf4D")]
impl WorkerInterface {
    #[zbus(name = "GetSettings")]
    fn get_settings(&self) -> std::result::Result<HashMap<String, OwnedValue>, WorkerError> {
        settings_to_dbus(&self.shared.settings_json())
    }

    #[zbus(name = "SetSettings")]
    fn set_settings(
        &self,
        settings: HashMap<String, OwnedValue>,
    ) -> std::result::Result<(), WorkerError> {
        let patch = Value::Object(settings_from_dbus(settings)?);
        let mut state = self.shared.lock();
        if state.stopping || self.shared.is_stopping() {
            return Err(WorkerError::InvalidSettings(
                "worker is stopping".to_owned(),
            ));
        }
        let mut next = state.config.clone();
        next.apply_settings(&patch)
            .map_err(|error| WorkerError::InvalidSettings(error.message))?;
        if next.monitor != state.config.monitor {
            return Err(WorkerError::InvalidSettings(
                "monitor changes require a worker restart".to_owned(),
            ));
        }
        next.save_settings(&patch)
            .map_err(|error| WorkerError::Failed(error.message))?;
        state.config = next;
        state.revision = state.revision.wrapping_add(1);
        self.shared.changed.notify_all();
        Ok(())
    }

    #[zbus(name = "GetStatus")]
    fn get_status(&self) -> &'static str {
        if self.shared.is_stopping() {
            "stopping"
        } else if self.shared.sent.load(Ordering::Acquire) {
            "running"
        } else {
            "starting"
        }
    }

    #[zbus(name = "GetPanelCount")]
    fn get_panel_count(&self) -> u32 {
        self.shared.panel_count.load(Ordering::Acquire) as u32
    }

    #[zbus(name = "GetPreview")]
    fn get_preview(&self) -> String {
        self.shared.preview_json()
    }

    #[zbus(name = "SetPreviewEnabled")]
    fn set_preview_enabled(&self, enabled: bool) {
        self.shared.set_preview_enabled(enabled);
    }

    #[zbus(name = "Stop")]
    fn stop(&self) {
        self.shared.request_stop();
    }
}

fn settings_to_dbus(settings: &Value) -> WorkerResult<HashMap<String, OwnedValue>> {
    let object = settings
        .as_object()
        .ok_or_else(|| WorkerError::Failed("worker settings are not a JSON object".to_owned()))?;
    let mut result = HashMap::with_capacity(object.len());
    for (key, value) in object {
        let owned = match key.as_str() {
            "brightness" | "saturation_boost" | "smooth" | "black_threshold" => {
                let number = value
                    .as_f64()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| {
                        WorkerError::Failed("worker setting has an invalid number".to_owned())
                    })?;
                OwnedValue::from(number)
            }
            "fps" | "max_step" | "edge_depth" | "center_bleed" | "peak_mix" | "feather_depth"
            | "perimeter_offset" => {
                let number = value
                    .as_i64()
                    .and_then(|number| i32::try_from(number).ok())
                    .ok_or_else(|| {
                        WorkerError::Failed("worker setting has an invalid integer".to_owned())
                    })?;
                OwnedValue::from(number)
            }
            "letterbox" | "midtone_lift" | "strict_black" | "spatial_blend" | "reverse_direction" => {
                OwnedValue::from(value.as_bool().ok_or_else(|| {
                    WorkerError::Failed("worker setting has an invalid boolean".to_owned())
                })?)
            }
            "monitor" | "mode" | "static_color" | "start_corner" => OwnedValue::try_from(DbusValue::from(
                value
                    .as_str()
                    .ok_or_else(|| {
                        WorkerError::Failed("worker setting has an invalid string".to_owned())
                    })?
                    .to_owned(),
            ))
            .map_err(|_| WorkerError::Failed("cannot encode worker string setting".to_owned()))?,
            _ => continue,
        };
        result.insert(key.clone(), owned);
    }
    Ok(result)
}

fn settings_from_dbus(settings: HashMap<String, OwnedValue>) -> WorkerResult<Map<String, Value>> {
    let mut result = Map::with_capacity(settings.len());
    for (key, value) in settings {
        let invalid = || WorkerError::InvalidSettings("invalid or unsupported setting".to_owned());
        let json = match key.as_str() {
            "brightness" | "saturation_boost" | "smooth" | "black_threshold" => {
                let value = f64::try_from(value).map_err(|_| invalid())?;
                Value::Number(Number::from_f64(value).ok_or_else(invalid)?)
            }
            "fps" | "max_step" | "edge_depth" | "center_bleed" | "peak_mix" | "feather_depth"
            | "perimeter_offset" => {
                Value::from(i32::try_from(value).map_err(|_| invalid())?)
            }
            "letterbox" | "midtone_lift" | "strict_black" | "spatial_blend" | "reverse_direction" => {
                Value::from(bool::try_from(value).map_err(|_| invalid())?)
            }
            "monitor" | "mode" | "static_color" | "start_corner" => {
                Value::String(String::try_from(value).map_err(|_| invalid())?)
            }
            _ => return Err(invalid()),
        };
        result.insert(key, json);
    }
    Ok(result)
}

fn register_worker(shared: Arc<Shared>) -> Result<Connection> {
    let connection = Connection::session().map_err(dbus_error)?;
    let registered = connection
        .object_server()
        .at(OBJECT_PATH, WorkerInterface { shared })
        .map_err(dbus_error)?;
    if !registered {
        return Err(AppError::new("cannot register worker D-Bus object"));
    }
    let reply = connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .map_err(dbus_error)?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(connection),
        RequestNameReply::Exists | RequestNameReply::InQueue => Err(AppError::new(
            "another Wayleaf 4D worker already owns the D-Bus service",
        )),
    }
}

fn dbus_error(error: zbus::Error) -> AppError {
    if matches!(&error, zbus::Error::NameTaken) {
        AppError::new("another Wayleaf 4D worker already owns the D-Bus service")
    } else {
        AppError::temporary(format!("session D-Bus error: {error}"))
    }
}

fn register_signals(stop: Arc<AtomicBool>) -> Result<()> {
    flag::register(SIGINT, stop.clone())
        .map_err(|error| AppError::new(format!("cannot register SIGINT handler: {error}")))?;
    flag::register(SIGTERM, stop)
        .map_err(|error| AppError::new(format!("cannot register SIGTERM handler: {error}")))?;
    Ok(())
}

fn mode_requires_capture(mode: &str) -> bool {
    mode != "1D"
}

fn capture_for_mode(config: &Config) -> Result<Option<Capture>> {
    if mode_requires_capture(&config.mode) {
        Capture::new(&config.monitor).map(Some)
    } else {
        Ok(None)
    }
}

fn mode_sampler_config(config: &Config) -> Config {
    let mut sampling = config.clone();
    match config.mode.as_str() {
        "2D" => {
            sampling.letterbox = false;
            sampling.center_bleed = 0;
            sampling.peak_mix = 0;
            sampling.feather_depth = 0;
            sampling.black_threshold = 0.0;
            sampling.midtone_lift = false;
            sampling.strict_black = false;
            sampling.spatial_blend = false;
        }
        "3D" => {
            sampling.letterbox = false;
            sampling.center_bleed = 0;
            sampling.feather_depth = 0;
            sampling.midtone_lift = false;
            sampling.strict_black = false;
        }
        _ => {}
    }
    sampling
}

fn stream_frame_interval(mode: &str, fps: i32) -> Duration {
    frame_interval(if mode == "1D" { 10 } else { fps })
}

pub fn run(mut config: Config) -> Result<()> {
    config.validate()?;
    if config.host.is_empty() || config.token.is_empty() {
        return Err(AppError::new("config or --ip and --token are required"));
    }
    let shared = Arc::new(Shared::new(config.clone()));
    register_signals(shared.stop.clone())?;
    if shared.is_stopping() {
        return Ok(());
    }
    let _connection = register_worker(shared.clone())?;
    if shared.is_stopping() {
        return Ok(());
    }

    let panels = panel_layout(&config)?;
    shared.panel_count.store(panels.len(), Ordering::Release);
    if shared.is_stopping() {
        return Ok(());
    }
    let mut zones = make_zones(&panels, &config)?;
    let mut sample_zones = zones_for_mode(&config.mode, &zones, config.edge_depth)?;
    let mut preview_zones = preview_zones_for_mode(&config.mode, &zones, &sample_zones)?;
    let mut sampler_config = mode_sampler_config(&config);
    let mut sampler = Sampler::new(sample_zones.clone());
    let mut mode_colors = vec![Color::default(); zones.len()];
    let mut capture = capture_for_mode(&config)?;
    if shared.is_stopping() {
        return Ok(());
    }

    let previous_effect = selected_effect(&config);
    if shared.is_stopping() {
        return Ok(());
    }
    let port = match enable_stream(&config) {
        Ok(port) => port,
        Err(error) => {
            restore_after_stream(&config, previous_effect.as_deref());
            return Err(error);
        }
    };

    let result = (|| {
        if shared.is_stopping() {
            return Ok(());
        }
        let socket = connect_udp(&config.host, port)?;
        let mut packet = [0_u8; 322];
        let mut revision = 0;
        let mut interval = stream_frame_interval(&config.mode, config.fps);
        let mut next_frame = Instant::now();
        let mut previous_frame = None;
        eprintln!(
            "Streaming {} Nanoleaf panels in {} mode to {}:{}",
            zones.len(),
            config.mode,
            config.host,
            port
        );

        while !shared.is_stopping() {
            if refresh_config(
                &shared,
                &panels,
                &mut config,
                &mut zones,
                &mut sample_zones,
                &mut preview_zones,
                &mut sampler_config,
                &mut sampler,
                &mut capture,
                &mut revision,
            )? {
                interval = stream_frame_interval(&config.mode, config.fps);
                next_frame = Instant::now();
            }
            if !shared.wait_until(revision, next_frame) {
                break;
            }
            if shared.is_stopping() {
                break;
            }
            if refresh_config(
                &shared,
                &panels,
                &mut config,
                &mut zones,
                &mut sample_zones,
                &mut preview_zones,
                &mut sampler_config,
                &mut sampler,
                &mut capture,
                &mut revision,
            )? {
                interval = stream_frame_interval(&config.mode, config.fps);
                next_frame = Instant::now();
                continue;
            }

            let (colors, preview_colors): (&[Color], &[Color]) = if config.mode == "1D" {
                colors_for_mode(
                    &config.mode,
                    &zones,
                    &sample_zones,
                    &[],
                    &config.static_color,
                    config.brightness,
                    &mut mode_colors,
                )?;
                (&mode_colors, &mode_colors[..1])
            } else {
                let capture = capture
                    .as_mut()
                    .ok_or_else(|| AppError::new("capture missing for screen-sampling mode"))?;
                let frame = capture.frame()?;
                let sampled_at = Instant::now();
                if shared.is_stopping() {
                    break;
                }
                let elapsed = previous_frame
                    .map(|previous: Instant| sampled_at.saturating_duration_since(previous))
                    .unwrap_or_else(|| Duration::from_secs_f64(1.0 / 30.0));
                let sampled = if config.mode == "2D" {
                    sampler.sample_full_screen(
                        frame.data,
                        frame.stride,
                        frame.width,
                        frame.height,
                        &sampler_config,
                        elapsed,
                    )?
                } else {
                    sampler.sample(
                        frame.data,
                        frame.stride,
                        frame.width,
                        frame.height,
                        &sampler_config,
                        elapsed,
                    )?
                };
                previous_frame = Some(sampled_at);
                if config.mode == "4D" {
                    (sampled, sampled)
                } else {
                    colors_for_mode(
                        &config.mode,
                        &zones,
                        &sample_zones,
                        sampled,
                        &config.static_color,
                        config.brightness,
                        &mut mode_colors,
                    )?;
                    (&mode_colors, sampled)
                }
            };
            shared.update_preview(&config.mode, &preview_zones, preview_colors);
            let packet_len = encode_packet(&zones, colors, &mut packet)?;
            if shared.is_stopping() {
                break;
            }
            send_packet(&socket, &packet[..packet_len])?;
            shared.sent.store(true, Ordering::Release);
            next_frame = next_deadline(next_frame, Instant::now(), interval);
        }

        // Synchronize with any SetSettings transaction that began before Stop.
        refresh_config(
            &shared,
            &panels,
            &mut config,
            &mut zones,
            &mut sample_zones,
            &mut preview_zones,
            &mut sampler_config,
            &mut sampler,
            &mut capture,
            &mut revision,
        )?;
        Ok(())
    })();
    restore_after_stream(&config, previous_effect.as_deref());
    result
}

fn restore_after_stream(config: &Config, effect: Option<&str>) {
    if let Some(effect) = effect {
        if let Err(error) = restore_effect(config, effect) {
            eprintln!(
                "Warning: could not restore the previous Nanoleaf effect: {}",
                error.message
            );
        }
    }
}

fn refresh_config(
    shared: &Shared,
    panels: &[crate::sampling::PanelPosition],
    config: &mut Config,
    zones: &mut Vec<Zone>,
    sample_zones: &mut Vec<Zone>,
    preview_zones: &mut Vec<Zone>,
    sampler_config: &mut Config,
    sampler: &mut Sampler,
    capture: &mut Option<Capture>,
    revision: &mut u64,
) -> Result<bool> {
    let Some((next_revision, next_config)) = shared.updated_config(*revision) else {
        return Ok(false);
    };
    let next_zones = make_zones(panels, &next_config)?;
    let next_sample_zones = zones_for_mode(&next_config.mode, &next_zones, next_config.edge_depth)?;
    let next_preview_zones =
        preview_zones_for_mode(&next_config.mode, &next_zones, &next_sample_zones)?;
    let next_sampler_config = mode_sampler_config(&next_config);
    if mode_requires_capture(&next_config.mode) {
        if capture.is_none() {
            *capture = capture_for_mode(&next_config)?;
        }
    } else {
        *capture = None;
    }
    sampler.remap(next_sample_zones.clone());
    *zones = next_zones;
    *sample_zones = next_sample_zones;
    *preview_zones = next_preview_zones;
    *sampler_config = next_sampler_config;
    *config = next_config;
    *revision = next_revision;
    Ok(true)
}

fn frame_interval(fps: i32) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1) as f64)
}

fn next_deadline(previous: Instant, now: Instant, interval: Duration) -> Instant {
    let next = previous.checked_add(interval).unwrap_or(now);
    if next <= now {
        now.checked_add(interval).unwrap_or(now)
    } else {
        next
    }
}

fn wait_until(stop: &AtomicBool, deadline: Instant) {
    while !stop.load(Ordering::Acquire) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        thread::sleep(remaining.min(SIGNAL_POLL));
    }
}

fn capture_latency_summary(samples: &mut [f64]) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    samples.sort_unstable_by(f64::total_cmp);
    let median = samples[(samples.len() - 1) / 2];
    let p95 = samples[(samples.len() - 1) * 95 / 100];
    (median, p95)
}

#[derive(Default)]
struct CaptureColorStats {
    samples: u64,
    red: u64,
    green: u64,
    blue: u64,
    red_green: i64,
    blue_green: i64,
    spread: u64,
    neutral_1: u64,
    neutral_2: u64,
}

impl CaptureColorStats {
    fn sample(data: &[u8], stride: usize, width: usize, height: usize) -> Self {
        let mut stats = Self::default();
        for y in (0..height).step_by(4) {
            for x in (0..width).step_by(4) {
                let offset = y * stride + x * 4;
                let [red, green, blue] = [data[offset], data[offset + 1], data[offset + 2]];
                let high = red.max(green).max(blue);
                let low = red.min(green).min(blue);
                let spread = u64::from(high - low);
                stats.samples += 1;
                stats.red += u64::from(red);
                stats.green += u64::from(green);
                stats.blue += u64::from(blue);
                stats.red_green += i64::from(red) - i64::from(green);
                stats.blue_green += i64::from(blue) - i64::from(green);
                stats.spread += spread;
                stats.neutral_1 += if spread <= 1 { 1 } else { 0 };
                stats.neutral_2 += if spread <= 2 { 1 } else { 0 };
            }
        }
        stats
    }

    fn means(&self) -> [f64; 3] {
        let count = self.samples.max(1) as f64;
        [
            self.red as f64 / count,
            self.green as f64 / count,
            self.blue as f64 / count,
        ]
    }

    fn mean_spread(&self) -> f64 {
        self.spread as f64 / self.samples.max(1) as f64
    }

    fn neutral_percentages(&self) -> [f64; 2] {
        let count = self.samples.max(1) as f64;
        [
            self.neutral_1 as f64 * 100.0 / count,
            self.neutral_2 as f64 * 100.0 / count,
        ]
    }
}

pub fn capture_test(config: &Config) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    register_signals(stop.clone())?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    crate::capture::synthetic_render_test(&config.monitor)?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut capture = Capture::new(&config.monitor)?;
    let started = Instant::now();
    let mut latency_ms = Vec::with_capacity(CAPTURE_TEST_FRAMES);
    let mut width = 0;
    let mut height = 0;
    while latency_ms.len() < CAPTURE_TEST_FRAMES
        && started.elapsed() < MAX_CAPTURE_TEST
        && !stop.load(Ordering::Acquire)
    {
        let before = Instant::now();
        let (frame_width, frame_height) = {
            let frame = capture.frame()?;
            (frame.width, frame.height)
        };
        let latency = before.elapsed().as_secs_f64() * 1000.0;
        if started.elapsed() > MAX_CAPTURE_TEST {
            break;
        }
        width = frame_width;
        height = frame_height;
        latency_ms.push(latency);
    }
    let elapsed = started.elapsed();
    let (median_ms, p95_ms) = capture_latency_summary(&mut latency_ms);
    println!(
        "KMS capture: {} frames at {}x{} in {:.2} s ({:.1} FPS); capture_ms_median/p95={:.2}/{:.2}",
        latency_ms.len(),
        width,
        height,
        elapsed.as_secs_f64(),
        latency_ms.len() as f64 / elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
        median_ms,
        p95_ms
    );
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    if latency_ms.len() < CAPTURE_TEST_FRAMES {
        return Err(AppError::temporary(
            "capture-test did not receive 30 frames within 5 seconds",
        ));
    }
    Ok(())
}

pub fn capture_debug(config: &Config) -> Result<()> {
    config.validate()?;
    if config.host.is_empty() || config.token.is_empty() {
        return Err(AppError::new(
            "config or --ip and --token are required for --capture-debug",
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    register_signals(stop.clone())?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let panels = panel_layout(config)?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let zones = make_zones(&panels, config)?;
    let mut sampler = Sampler::new(zones.clone());
    let mut capture = Capture::new(&config.monitor)?;
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let expected = frame_interval(config.fps);
    let mut deadline = Instant::now();
    let started = Instant::now();
    let mut previous = None;
    let mut previous_active = None;
    let mut latencies = Vec::with_capacity(100);
    let mut intervals = Vec::with_capacity(100);
    let mut scene_cuts = 0;
    let mut active_changes = 0;
    let mut packet = [0u8; 322];
    let mut frames = 0_usize;
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::new(stdout.lock());
    writeln!(
        output,
        "# capture-debug: scanout-RGB statistics are post-GPU capture; each zone is controller-ID:target-RGB>smoothed-RGB>encoded-packet-RGB; no UDP frames sent"
    )
    .map_err(diagnostic_io)?;
    eprintln!("Sampling for 10 seconds; no desktop images or settings are saved");

    while started.elapsed() < DEBUG_DURATION && !stop.load(Ordering::Acquire) {
        wait_until(&stop, deadline);
        if stop.load(Ordering::Acquire) || started.elapsed() >= DEBUG_DURATION {
            break;
        }
        let before = Instant::now();
        let frame = capture.frame()?;
        let sampled_at = Instant::now();
        let latency = sampled_at.duration_since(before).as_secs_f64() * 1000.0;
        latencies.push(latency);
        if let Some(previous_at) = previous {
            intervals.push(sampled_at.duration_since(previous_at).as_secs_f64() * 1000.0);
        }
        let elapsed = previous
            .map(|previous_at: Instant| sampled_at.saturating_duration_since(previous_at))
            .unwrap_or_else(|| Duration::from_secs_f64(1.0 / 30.0));
        sampler.sample_in_place(
            frame.data,
            frame.stride,
            frame.width,
            frame.height,
            config,
            elapsed,
        )?;
        let colors = sampler.out();
        let raw = sampler.raw();
        encode_packet(&zones, colors, &mut packet)?;
        let frame_stats = CaptureColorStats::sample(frame.data, frame.stride, frame.width, frame.height);
        let [mean_r, mean_g, mean_b] = frame_stats.means();
        let mean_spread = frame_stats.mean_spread();
        let [neutral_1, neutral_2] = frame_stats.neutral_percentages();
        let cut = sampler.scene_cut();
        let active = sampler.active();
        if cut {
            scene_cuts += 1;
        }
        if previous_active.is_some_and(|old| old != active) {
            active_changes += 1;
        }
        previous_active = Some(active);
        frames += 1;
        write!(
            output,
            "frame={frames:04} arrival_ms={:.1} capture={}x{} rgb={mean_r:.1},{mean_g:.1},{mean_b:.1} rg={:+.1} bg={:+.1} spread={mean_spread:.1} neutral1={neutral_1:.1}% neutral2={neutral_2:.1}% cut={} active={:.3},{:.3},{:.3},{:.3} zones=",
            started.elapsed().as_secs_f64() * 1000.0,
            frame.width,
            frame.height,
            frame_stats.red_green as f64 / frame_stats.samples.max(1) as f64,
            frame_stats.blue_green as f64 / frame_stats.samples.max(1) as f64,
            u8::from(cut),
            active.x0,
            active.x1,
            active.y0,
            active.y1
        )
        .map_err(diagnostic_io)?;
        for (index, ((zone, target), output_color)) in zones.iter().zip(raw).zip(colors).enumerate()
        {
            if index > 0 {
                write!(output, " ").map_err(diagnostic_io)?;
            }
            let offset = 2 + index * 8;
            write!(
                output,
                "{}:{},{},{}>{},{},{}>{},{},{}",
                zone.id,
                target.r.round() as i32,
                target.g.round() as i32,
                target.b.round() as i32,
                output_color.r.round() as i32,
                output_color.g.round() as i32,
                output_color.b.round() as i32,
                packet[offset + 2],
                packet[offset + 3],
                packet[offset + 4]
            )
            .map_err(diagnostic_io)?;
        }
        writeln!(output).map_err(diagnostic_io)?;
        output.flush().map_err(diagnostic_io)?;
        previous = Some(sampled_at);
        deadline = next_deadline(deadline, Instant::now(), expected);
    }
    let elapsed = started.elapsed();
    let (capture_median, capture_p95) = capture_latency_summary(&mut latencies);
    let (interval_median, interval_p95) = capture_latency_summary(&mut intervals);
    writeln!(
        output,
        "# summary frames={frames} seconds={:.2} fps={:.1} capture_ms_median/p95={:.2}/{:.2} arrival_ms_median/p95={:.2}/{:.2} scene_cuts={scene_cuts} active_rect_changes={active_changes} smooth={:.2} max_step={} brightness={:.2} edge_depth={}% center_bleed={}% peak_mix={}% feather_depth={}%",
        elapsed.as_secs_f64(),
        frames as f64 / elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
        capture_median,
        capture_p95,
        interval_median,
        interval_p95,
        config.smooth,
        config.max_step,
        config.brightness,
        config.edge_depth,
        config.center_bleed,
        config.peak_mix,
        config.feather_depth
    )
    .map_err(diagnostic_io)?;
    output.flush().map_err(diagnostic_io)?;
    Ok(())
}

fn diagnostic_io(error: std::io::Error) -> AppError {
    AppError::new(format!("cannot write capture diagnostics: {error}"))
}

fn percentile_for_test(values: &[f64]) -> (f64, f64) {
    let mut values = values.to_vec();
    capture_latency_summary(&mut values)
}

pub fn self_test() -> Result<()> {
    let static_mode = Config {
        mode: "1D".into(),
        ..Config::default()
    };
    if capture_for_mode(&static_mode)?.is_some()
        || ["2D", "3D", "4D"]
            .iter()
            .any(|mode| !mode_requires_capture(mode))
    {
        return Err(AppError::new(
            "runtime self-test: mode capture selection failed",
        ));
    }
    let default = Config::default();
    if stream_frame_interval("1D", 1) != Duration::from_millis(100)
        || stream_frame_interval("3D", 4) != Duration::from_millis(250)
        || mode_sampler_config(&default).settings_json() != default.settings_json()
    {
        return Err(AppError::new(
            "runtime self-test: mode sampling controls changed",
        ));
    }
    let two_d = mode_sampler_config(&Config {
        mode: "2D".into(),
        max_step: 1,
        letterbox: true,
        center_bleed: 20,
        peak_mix: 25,
        feather_depth: 10,
        black_threshold: 0.05,
        midtone_lift: true,
        strict_black: true,
        spatial_blend: true,
        ..default.clone()
    });
    if two_d.max_step != 1
        || two_d.letterbox
        || two_d.center_bleed != 0
        || two_d.peak_mix != 0
        || two_d.feather_depth != 0
        || two_d.black_threshold != 0.0
        || two_d.midtone_lift
        || two_d.strict_black
        || two_d.spatial_blend
        || two_d.brightness != default.brightness
        || two_d.saturation_boost != default.saturation_boost
        || two_d.smooth != default.smooth
    {
        return Err(AppError::new(
            "runtime self-test: 2D exposed controls changed",
        ));
    }
    let three_d_input = Config {
        mode: "3D".into(),
        edge_depth: 14,
        max_step: 37,
        peak_mix: 29,
        letterbox: true,
        center_bleed: 20,
        feather_depth: 10,
        black_threshold: 0.05,
        midtone_lift: true,
        strict_black: true,
        ..default.clone()
    };
    let three_d = mode_sampler_config(&three_d_input);
    if three_d.edge_depth != 14
        || three_d.max_step != 37
        || three_d.peak_mix != 29
        || three_d.letterbox
        || three_d.center_bleed != 0
        || three_d.feather_depth != 0
        || three_d.black_threshold != three_d_input.black_threshold
        || three_d.midtone_lift
        || three_d.strict_black
        || three_d.brightness != three_d_input.brightness
        || three_d.saturation_boost != three_d_input.saturation_boost
        || three_d.smooth != three_d_input.smooth
    {
        return Err(AppError::new(
            "runtime self-test: 3D exposed controls changed",
        ));
    }
    let base = Instant::now();
    let interval = Duration::from_millis(100);
    let deadline = base + interval;
    let late = deadline + Duration::from_millis(350);
    let next = next_deadline(deadline, late, interval);
    if next <= late || next > late + interval {
        return Err(AppError::new(
            "runtime self-test: scheduler accumulated missed frames",
        ));
    }
    if percentile_for_test(&[1.0, 9.0, 4.0, 2.0]) != (2.0, 4.0) {
        return Err(AppError::new(
            "runtime self-test: capture timing percentiles",
        ));
    }
    preview_self_test()?;

    shutdown_self_test()?;
    settings_transaction_self_test()?;
    Ok(())
}

fn preview_self_test() -> Result<()> {
    use crate::sampling::{Color, Zone};

    let shared = Shared::new(Config::default());
    let zones = [
        Zone {
            id: 34,
            edge: Some("bottom"),
            x0: 0.1,
            x1: 0.2,
            y0: 0.9,
            y1: 1.0,
        },
        Zone {
            id: 3,
            edge: Some("left"),
            x0: 0.0,
            x1: 0.08,
            y0: 0.2,
            y1: 0.3,
        },
    ];
    let colors = [
        Color {
            r: 10.0,
            g: 20.0,
            b: 30.0,
        },
        Color {
            r: 40.0,
            g: 50.0,
            b: 60.0,
        },
    ];
    shared.update_preview("4D", &zones, &colors);
    if shared.preview_json() != "[]" {
        return Err(AppError::new(
            "runtime self-test: preview updated while disabled",
        ));
    }
    shared.set_preview_enabled(true);
    shared.update_preview("4D", &zones, &colors);
    let preview: Value = serde_json::from_str(&shared.preview_json())
        .map_err(|_| AppError::new("runtime self-test: preview serialization failed"))?;
    let values = preview
        .as_array()
        .filter(|values| values.len() == 2)
        .ok_or_else(|| AppError::new("runtime self-test: preview segment count"))?;
    let bottom_position = values[0]["position"].as_f64().unwrap_or(-1.0);
    let left_position = values[1]["position"].as_f64().unwrap_or(-1.0);
    if values[0]["id"] != 34
        || values[0]["edge"] != "bottom"
        || (bottom_position - 0.15).abs() > 0.001
        || values[0]["r"] != 10
        || values[1]["id"] != 3
        || values[1]["edge"] != "left"
        || (left_position - 0.25).abs() > 0.001
        || values[1]["b"] != 60
    {
        return Err(AppError::new(
            "runtime self-test: preview lost panel IDs, edge geometry, or colors",
        ));
    }
    shared.set_preview_enabled(false);
    if shared.preview_json() != "[]"
        || !shared
            .preview
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    {
        return Err(AppError::new(
            "runtime self-test: disabled preview retained a published snapshot",
        ));
    }
    let preview_shared = Shared::new(Config::default());
    preview_shared.set_preview_enabled(true);

    for depth in [5, 8, 20] {
        let coarse = zones_for_mode("3D", &zones, depth)?;
        preview_shared.update_preview("3D", &coarse, &[Color::default(); 10]);
        let published: Value = serde_json::from_str(&preview_shared.preview_json())
            .map_err(|_| AppError::new("runtime self-test: 3D preview serialization failed"))?;
        let segments = published.as_array().filter(|segments| segments.len() == 10)
            .ok_or_else(|| AppError::new("runtime self-test: 3D preview zone count"))?;
        let expected_edges = ["top", "top", "top", "right", "right",
            "bottom", "bottom", "bottom", "left", "left"];
        let expected_positions = [1.0 / 6.0, 0.5, 5.0 / 6.0, 0.25, 0.75,
            1.0 / 6.0, 0.5, 5.0 / 6.0, 0.25, 0.75];
        for (index, segment) in segments.iter().enumerate() {
            let expected_span = if expected_edges[index] == "left"
                || expected_edges[index] == "right" { 0.5 } else { 1.0 / 3.0 };
            if segment["id"] != index as u64
                || segment["edge"] != expected_edges[index]
                || (segment["position"].as_f64().unwrap_or(-1.0)
                    - expected_positions[index]).abs() > 0.0001
                || (segment["span"].as_f64().unwrap_or(-1.0)
                    - expected_span).abs() > 0.0001
            {
                return Err(AppError::new(format!(
                    "runtime self-test: 3D zone {index} has wrong edge geometry at depth {depth}"
                )));
            }
        }
    }

    Ok(())
}

fn shutdown_self_test() -> Result<()> {
    let shared = Arc::new(Shared::new(Config::default()));
    let waiting = shared.clone();
    let wait_start = Instant::now();
    let waiter =
        thread::spawn(move || waiting.wait_until(0, Instant::now() + Duration::from_secs(5)));
    thread::sleep(Duration::from_millis(5));
    shared.request_stop();
    let continued = waiter
        .join()
        .map_err(|_| AppError::new("runtime self-test: stop waiter panicked"))?;
    if continued || wait_start.elapsed() > Duration::from_millis(250) {
        return Err(AppError::new(
            "runtime self-test: Stop did not wake the scheduler",
        ));
    }
    Ok(())
}

fn settings_transaction_self_test() -> Result<()> {
    let path = std::env::temp_dir().join(format!(
        "wayleaf-runtime-self-test-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _remove = RemoveFile(path.clone());
    let config = Config {
        config_path: path.clone(),
        ..Config::default()
    };
    let shared = Arc::new(Shared::new(config));
    let interface = WorkerInterface {
        shared: shared.clone(),
    };

    let mode = OwnedValue::try_from(DbusValue::from("2D".to_owned()))
        .map_err(|_| AppError::new("runtime self-test: cannot create D-Bus mode value"))?;
    let static_color = OwnedValue::try_from(DbusValue::from("#40a0ff".to_owned()))
        .map_err(|_| AppError::new("runtime self-test: cannot create D-Bus color value"))?;
    let valid = HashMap::from([
        ("brightness".to_owned(), OwnedValue::from(0.73_f64)),
        ("smooth".to_owned(), OwnedValue::from(0.62_f64)),
        ("mode".to_owned(), mode),
        ("static_color".to_owned(), static_color),
        ("start_corner".to_owned(), OwnedValue::from(zbus::zvariant::Str::from("top_right"))),
        ("spatial_blend".to_owned(), OwnedValue::from(true)),
        ("reverse_direction".to_owned(), OwnedValue::from(true)),
        ("perimeter_offset".to_owned(), OwnedValue::from(-39_i32)),
    ]);
    interface
        .set_settings(valid)
        .map_err(|error| AppError::new(format!("runtime self-test: {error:?}")))?;
    let returned = interface
        .get_settings()
        .map_err(|error| AppError::new(format!("runtime self-test: {error:?}")))?;
    let returned = settings_from_dbus(returned)
        .map_err(|error| AppError::new(format!("runtime self-test: {error:?}")))?;
    if returned.get("mode") != Some(&Value::String("2D".into()))
        || returned.get("static_color") != Some(&Value::String("#40a0ff".into()))
        || returned.get("start_corner") != Some(&Value::String("top_right".into()))
        || returned.get("spatial_blend") != Some(&Value::Bool(true))
        || returned.get("reverse_direction") != Some(&Value::Bool(true))
        || returned.get("perimeter_offset") != Some(&Value::from(-39))
    {
        return Err(AppError::new(
            "runtime self-test: mode settings D-Bus round trip failed",
        ));
    }
    let saved = Config::load(&path)?;
    if saved.start_corner != "top_right" || !saved.reverse_direction || saved.perimeter_offset != -39 {
        return Err(AppError::new("runtime self-test: alignment was not persisted"));
    }
    for patch in [
        HashMap::from([("start_corner".to_owned(), OwnedValue::from(zbus::zvariant::Str::from("invalid")))]),
        HashMap::from([("perimeter_offset".to_owned(), OwnedValue::from(40_i32))]),
        HashMap::from([("reverse_direction".to_owned(), OwnedValue::from(1_i32))]),
    ] {
        if !matches!(interface.set_settings(patch), Err(WorkerError::InvalidSettings(_))) {
            return Err(AppError::new("runtime self-test: invalid alignment was accepted"));
        }
    }
    let invalid = HashMap::from([
        ("brightness".to_owned(), OwnedValue::from(0.1_f64)),
        ("fps".to_owned(), OwnedValue::from(0_i32)),
    ]);
    if !matches!(
        interface.set_settings(invalid),
        Err(WorkerError::InvalidSettings(_))
    ) {
        return Err(AppError::new(
            "runtime self-test: invalid settings were accepted",
        ));
    }
    let wrong_type = HashMap::from([("fps".to_owned(), OwnedValue::from(10.0_f64))]);
    if !matches!(
        interface.set_settings(wrong_type),
        Err(WorkerError::InvalidSettings(_))
    ) {
        return Err(AppError::new(
            "runtime self-test: wrong D-Bus setting type was accepted",
        ));
    }
    let unknown = HashMap::from([("port".to_owned(), OwnedValue::from(16000_i32))]);
    if !matches!(
        interface.set_settings(unknown),
        Err(WorkerError::InvalidSettings(_))
    ) {
        return Err(AppError::new(
            "runtime self-test: unknown setting was accepted",
        ));
    }
    let monitor_value = OwnedValue::try_from(DbusValue::from("test-monitor".to_owned()))
        .map_err(|_| AppError::new("runtime self-test: cannot create D-Bus monitor value"))?;
    if !matches!(
        interface.set_settings(HashMap::from([("monitor".to_owned(), monitor_value)])),
        Err(WorkerError::InvalidSettings(_))
    ) {
        return Err(AppError::new(
            "runtime self-test: live monitor change was accepted",
        ));
    }
    shared.request_stop();
    if !matches!(
        interface.set_settings(HashMap::from([(
            "brightness".to_owned(),
            OwnedValue::from(0.2_f64),
        )])),
        Err(WorkerError::InvalidSettings(_))
    ) {
        return Err(AppError::new(
            "runtime self-test: settings accepted after Stop",
        ));
    }
    let saved = Config::load(&path)?;
    if (saved.brightness - 0.73).abs() > f32::EPSILON
        || (saved.smooth - 0.62).abs() > f32::EPSILON
        || saved.fps != Config::default().fps
        || saved.mode != "2D"
        || saved.static_color != "#40a0ff"
    {
        return Err(AppError::new(
            "runtime self-test: settings transaction was lost or partially committed",
        ));
    }
    Ok(())
}

struct RemoveFile(std::path::PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod capture_debug_stats_tests {
    use super::CaptureColorStats;

    fn frame_with_color(color: [u8; 4]) -> Vec<u8> {
        let mut frame = vec![0; 8 * 8 * 4];
        for pixel in frame.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        frame
    }

    #[test]
    fn grayscale_capture_is_reported_as_neutral() {
        let frame = frame_with_color([64, 64, 64, 255]);
        let stats = CaptureColorStats::sample(&frame, 8 * 4, 8, 8);

        assert_eq!(stats.samples, 4);
        assert_eq!(stats.means(), [64.0, 64.0, 64.0]);
        assert_eq!(stats.mean_spread(), 0.0);
        assert_eq!(stats.neutral_percentages(), [100.0, 100.0]);
    }

    #[test]
    fn magenta_capture_is_reported_as_channel_skew() {
        let frame = frame_with_color([128, 64, 128, 255]);
        let stats = CaptureColorStats::sample(&frame, 8 * 4, 8, 8);

        assert_eq!(stats.means(), [128.0, 64.0, 128.0]);
        assert_eq!(stats.red_green, 4 * 64);
        assert_eq!(stats.blue_green, 4 * 64);
        assert_eq!(stats.mean_spread(), 64.0);
        assert_eq!(stats.neutral_percentages(), [0.0, 0.0]);
    }
}
