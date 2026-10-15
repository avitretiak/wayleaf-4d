use crate::capture::Capture;
use crate::device::{connect_udp, enable_stream, encode_packet, panel_layout, send_packet};
use crate::sampling::{make_zones, Sampler, Zone};
use crate::{AppError, Config, Result};
use serde_json::{Map, Number, Value};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
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

struct Shared {
    state: Mutex<WorkerState>,
    changed: Condvar,
    stop: Arc<AtomicBool>,
    sent: AtomicBool,
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
        }
    }

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
        let patch = settings_from_dbus(settings)?;
        let mut state = self.shared.lock();
        if state.stopping || self.shared.is_stopping() {
            return Err(WorkerError::InvalidSettings(
                "worker is stopping".to_owned(),
            ));
        }
        let mut next = state.config.clone();
        next.apply_settings(&Value::Object(patch))
            .map_err(|error| WorkerError::InvalidSettings(error.message))?;
        if next.monitor != state.config.monitor {
            return Err(WorkerError::InvalidSettings(
                "monitor changes require a worker restart".to_owned(),
            ));
        }
        next.save()
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
            "fps" | "max_step" | "edge_depth" | "center_bleed" | "peak_mix" | "feather_depth" => {
                let number = value
                    .as_i64()
                    .and_then(|number| i32::try_from(number).ok())
                    .ok_or_else(|| {
                        WorkerError::Failed("worker setting has an invalid integer".to_owned())
                    })?;
                OwnedValue::from(number)
            }
            "letterbox" | "midtone_lift" | "strict_black" => {
                OwnedValue::from(value.as_bool().ok_or_else(|| {
                    WorkerError::Failed("worker setting has an invalid boolean".to_owned())
                })?)
            }
            "monitor" => OwnedValue::try_from(DbusValue::from(
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
            "fps" | "max_step" | "edge_depth" | "center_bleed" | "peak_mix" | "feather_depth" => {
                Value::from(i32::try_from(value).map_err(|_| invalid())?)
            }
            "letterbox" | "midtone_lift" | "strict_black" => {
                Value::from(bool::try_from(value).map_err(|_| invalid())?)
            }
            "monitor" => Value::String(String::try_from(value).map_err(|_| invalid())?),
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
    if shared.is_stopping() {
        return Ok(());
    }
    let mut zones = make_zones(&panels, &config)?;
    let mut sampler = Sampler::new(zones.clone());
    let mut capture = Capture::new(&config.monitor)?;
    if shared.is_stopping() {
        return Ok(());
    }
    let port = enable_stream(&config)?;
    if shared.is_stopping() {
        return Ok(());
    }
    let socket = connect_udp(&config.host, port)?;
    let mut packet = [0_u8; 322];
    let mut revision = 0;
    let mut interval = frame_interval(config.fps);
    let mut next_frame = Instant::now();
    let mut previous_frame = None;
    eprintln!(
        "Streaming {} Nanoleaf 4D segments to {}:{}",
        zones.len(),
        config.host,
        port
    );

    while !shared.is_stopping() {
        if refresh_config(
            &shared,
            &panels,
            &mut config,
            &mut zones,
            &mut sampler,
            &mut revision,
        )? {
            interval = frame_interval(config.fps);
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
            &mut sampler,
            &mut revision,
        )? {
            interval = frame_interval(config.fps);
            next_frame = Instant::now();
            continue;
        }

        let frame = capture.frame()?;
        let sampled_at = Instant::now();
        if shared.is_stopping() {
            break;
        }
        let elapsed = previous_frame
            .map(|previous: Instant| sampled_at.saturating_duration_since(previous))
            .unwrap_or_else(|| Duration::from_secs_f64(1.0 / 30.0));
        let colors = sampler.sample(
            frame.data,
            frame.stride,
            frame.width,
            frame.height,
            &config,
            elapsed,
        )?;
        let packet_len = encode_packet(&zones, colors, &mut packet)?;
        if shared.is_stopping() {
            break;
        }
        send_packet(&socket, &packet[..packet_len])?;
        shared.sent.store(true, Ordering::Release);
        previous_frame = Some(sampled_at);
        next_frame = next_deadline(next_frame, Instant::now(), interval);
    }

    // Synchronize with any SetSettings transaction that began before Stop.
    refresh_config(
        &shared,
        &panels,
        &mut config,
        &mut zones,
        &mut sampler,
        &mut revision,
    )?;
    Ok(())
}

fn refresh_config(
    shared: &Shared,
    panels: &[crate::sampling::PanelPosition],
    config: &mut Config,
    zones: &mut Vec<Zone>,
    sampler: &mut Sampler,
    revision: &mut u64,
) -> Result<bool> {
    let Some((next_revision, next_config)) = shared.updated_config(*revision) else {
        return Ok(false);
    };
    let next_zones = make_zones(panels, &next_config)?;
    sampler.remap(next_zones.clone());
    *zones = next_zones;
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
    let mut frames = 0_usize;
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::new(stdout.lock());
    writeln!(
        output,
        "# capture-debug: no UDP frames sent; each zone is controller-ID:target-RGB>output-RGB"
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
            "frame={frames:04} arrival_ms={:.1} cut={} active={:.3},{:.3},{:.3},{:.3} zones=",
            started.elapsed().as_secs_f64() * 1000.0,
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
            write!(
                output,
                "{}:{},{},{}>{},{},{}",
                zone.id,
                target.r.round() as i32,
                target.g.round() as i32,
                target.b.round() as i32,
                output_color.r.round() as i32,
                output_color.g.round() as i32,
                output_color.b.round() as i32
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

    shutdown_self_test()?;
    settings_transaction_self_test()?;
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

    let valid = HashMap::from([
        ("brightness".to_owned(), OwnedValue::from(0.73_f64)),
        ("smooth".to_owned(), OwnedValue::from(0.62_f64)),
    ]);
    interface
        .set_settings(valid)
        .map_err(|error| AppError::new(format!("runtime self-test: {error:?}")))?;
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
