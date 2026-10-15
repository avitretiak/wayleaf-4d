use crate::{config, device, kms, AppError, Config, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::process::{Child, Command, Output, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::{connection::Builder, Connection, MessageIterator, Proxy};
use zbus::fdo;
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str};
use zbus::MatchRule;

const CONTROL_NAME: &str = "io.github.avitretiak.Wayleaf4D.Control";
const CONTROL_PATH: &str = "/io/github/avitretiak/Wayleaf4D/Control";
const WORKER_NAME: &str = "io.github.avitretiak.Wayleaf4D";
const WORKER_PATH: &str = "/io/github/avitretiak/Wayleaf4D";
const WORKER_INTERFACE: &str = "io.github.avitretiak.Wayleaf4D";
const SYSTEMD_UNIT: &str = "wayleaf-4d.service";
const AVAHI_NAME: &str = "org.freedesktop.Avahi";
const AVAHI_SERVER_PATH: &str = "/";
const AVAHI_SERVER_INTERFACE: &str = "org.freedesktop.Avahi.Server";
const AVAHI_BROWSER_INTERFACE: &str = "org.freedesktop.Avahi.ServiceBrowser";
const NANOLEAF_SERVICE: &str = "_nanoleafapi._tcp";
const STATE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const BUS_CALL_TIMEOUT: Duration = Duration::from_secs(3);
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(15);
const DISCOVERY_WINDOW: Duration = Duration::from_secs(5);
const MAX_SETTINGS_BYTES: usize = 4 * 1024;
type AvahiResolveServiceArgs = (i32, i32, String, String, String, i32, u32);
type AvahiResolvedService = (
    i32,
    i32,
    String,
    String,
    String,
    String,
    i32,
    String,
    u16,
    Vec<Vec<u8>>,
    u32,
);
struct Shared {
    state: Mutex<State>,
}

struct State {
    config: Config,
    status: String,
    error: String,
    running: bool,
    enabled: bool,
    busy: bool,
    worker_present: bool,
    device_connected: bool,
    device_busy: bool,
    device_info: Value,
    device_info_stale: bool,
    discovered_devices: Vec<Value>,
    device_message: String,
    preview_enabled: bool,
    monitors: Vec<kms::Monitor>,
    monitor_error: String,
    revision: u64,
}

struct WorkerSnapshot {
    present: Option<bool>,
    status: Option<std::result::Result<String, String>>,
    settings: Option<std::result::Result<HashMap<String, OwnedValue>, String>>,
    error: Option<String>,
}

impl State {
    fn new(config: Config, monitors: Vec<kms::Monitor>) -> Self {
        let device_connected = !config.host.is_empty() && !config.token.is_empty();
        Self {
            config,
            status: "stopped".into(),
            error: String::new(),
            running: false,
            enabled: false,
            busy: false,
            worker_present: false,
            device_connected,
            device_busy: false,
            device_info: json!({}),
            device_info_stale: false,
            discovered_devices: Vec::new(),
            device_message: String::new(),
            monitors,
            monitor_error: String::new(),
            preview_enabled: false,
            revision: 0,
        }
    }

    fn json(&self) -> Value {
        json!({
            "settings": self.config.settings_json(),
            "status": self.status,
            "error": self.error,
            "running": self.running,
            "enabled": self.enabled,
            "busy": self.busy,
            "deviceConnected": self.device_connected,
            "deviceBusy": self.device_busy,
            "deviceHost": self.config.host,
            "deviceInfo": self.device_info,
            "deviceInfoStale": self.device_info_stale,
            "discoveredDevices": self.discovered_devices,
            "deviceMessage": self.device_message,
            "monitors": self.monitors,
            "monitorError": self.monitor_error,
            "selectedMonitor": self.config.monitor,
        })
    }
}

fn state_lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn run(config: Config) -> Result<()> {
    let (monitors, monitor_error) = match kms::monitors() {
        Ok(monitors) => (monitors, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let mut initial = State::new(config, monitors);
    if let Some(error) = monitor_error {
        initial.monitor_error = error;
    }
    let shared = Arc::new(Shared {
        state: Mutex::new(initial),
    });
    let api = ControlApi {
        shared: Arc::clone(&shared),
    };
    let connection = Connection::session().map_err(|error| {
        AppError::new(format!(
            "cannot connect control service to session bus: {error}"
        ))
    })?;
    connection
        .object_server()
        .at(CONTROL_PATH, api)
        .map_err(|error| AppError::new(format!("cannot export control D-Bus object: {error}")))?;
    let name = connection
        .request_name_with_flags(CONTROL_NAME, fdo::RequestNameFlags::DoNotQueue.into())
        .map_err(|error| AppError::new(format!("cannot claim control D-Bus name: {error}")))?;
    if name != fdo::RequestNameReply::PrimaryOwner {
        return Err(AppError::new("control D-Bus name is already owned"));
    }
    let _connection = connection;

    thread::Builder::new()
        .name("wayleaf-control-state".into())
        .spawn(move || loop {
            refresh_state(&shared);
            thread::sleep(STATE_POLL_INTERVAL);
        })
        .map_err(|_| AppError::new("cannot start control state monitor"))?;
    loop {
        thread::park();
    }
}

struct ControlApi {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "io.github.avitretiak.Wayleaf4D.Control")]
impl ControlApi {
    fn get_state(&self) -> String {
        serde_json::to_string(&state_lock(&self.shared).json()).unwrap_or_else(|_| "{}".into())
    }

    #[zbus(name = "GetPreview")]
    fn get_preview(&self) -> String {
        if !state_lock(&self.shared).preview_enabled {
            return "[]".into();
        }
        let Ok(connection) = session_connection() else {
            return "[]".into();
        };
        worker_preview_on(&connection)
    }

    #[zbus(name = "SetPreviewEnabled")]
    fn set_preview_enabled(&self, enabled: bool) {
        state_lock(&self.shared).preview_enabled = enabled;
        let Ok(connection) = session_connection() else {
            return;
        };
        if !worker_present_on(&connection).unwrap_or(false) {
            return;
        }
        if let Ok(proxy) = worker_proxy(&connection) {
            let _: std::result::Result<(), zbus::Error> = proxy.call("SetPreviewEnabled", &enabled);
        };
    }

    fn set_settings(&self, settings_json: String) -> fdo::Result<()> {
        if settings_json.len() > MAX_SETTINGS_BYTES {
            return Err(fdo::Error::InvalidArgs(
                "settings JSON exceeds the 4 KiB limit".into(),
            ));
        }
        let updates: Value = serde_json::from_str(&settings_json)
            .map_err(|_| fdo::Error::InvalidArgs("settings must be valid JSON".into()))?;
        if !updates.is_object() {
            return Err(fdo::Error::InvalidArgs(
                "settings must be a JSON object".into(),
            ));
        }
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            let mut candidate = state.config.clone();
            candidate
                .apply_settings(&updates)
                .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
            if candidate.monitor != state.config.monitor {
                return Err(fdo::Error::InvalidArgs(
                    "use SelectMonitor to change the capture monitor".into(),
                ));
            }
            if updates.as_object().is_some_and(|object| object.is_empty()) {
                return Ok(());
            }
            state.busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.error.clear();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-settings",
            BusyKind::Control,
            move |shared| {
                finish_control(&shared, apply_settings(&shared, &updates), None, None, None);
            },
        )
    }

    #[zbus(name = "Run")]
    fn run(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            state.busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.error.clear();
            state.status = "starting".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-run",
            BusyKind::Control,
            move |shared| {
                finish_control(&shared, run_worker(), Some(true), None, Some("starting"));
            },
        )
    }

    #[zbus(name = "Stop")]
    fn stop(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            state.busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.error.clear();
            state.status = "stopping".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-stop",
            BusyKind::Control,
            move |shared| match stop_worker_and_power_off(&shared) {
                Ok(warning) => finish_stop(&shared, warning),
                Err(error) => finish_control(&shared, Err(error), None, None, None),
            },
        )
    }

    #[zbus(name = "SetStartAtLogin")]
    fn set_start_at_login(&self, enabled: bool) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            state.busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.error.clear();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-login-setting",
            BusyKind::Control,
            move |shared| {
                finish_control(
                    &shared,
                    set_start_at_login(enabled),
                    None,
                    Some(enabled),
                    None,
                );
            },
        )
    }

    fn select_monitor(&self, name: String) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            let available = if name.is_empty() {
                state.monitors.len() == 1
            } else {
                state.monitors.iter().any(|monitor| monitor.name == name)
            };
            if !available {
                return Err(fdo::Error::InvalidArgs(
                    "selected monitor is unavailable or ambiguous".into(),
                ));
            }
            if state.config.monitor == name {
                return Ok(());
            }
            state.busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.error.clear();
            if state.running {
                state.status = "stopping".into();
            }
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-monitor",
            BusyKind::Control,
            move |shared| finish_control(&shared, select_monitor(&shared, &name), None, None, None),
        )
    }

    fn discover_devices(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            state.device_busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.discovered_devices.clear();
            state.device_message = "Searching the local network for Nanoleaf devices…".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-discovery",
            BusyKind::Device,
            move |shared| match discover_devices() {
                Ok(devices) => {
                    let mut state = state_lock(&shared);
                    state.discovered_devices = devices;
                    state.device_message = if state.discovered_devices.is_empty() {
                        "No Nanoleaf devices found. Enter the device IP manually.".into()
                    } else {
                        "Select your Nanoleaf 4D from the results.".into()
                    };
                    state.device_busy = false;
                    state.revision = state.revision.wrapping_add(1);
                }
                Err(message) => finish_device(&shared, Err(message)),
            },
        )
    }

    fn pair_device(&self, host: String) -> fdo::Result<()> {
        let address = host.trim().to_owned();
        if Ipv4Addr::from_str(&address).is_err() {
            return Err(fdo::Error::InvalidArgs("enter a valid IPv4 address".into()));
        }
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            if state.running || matches!(state.status.as_str(), "starting" | "running" | "stopping")
            {
                state.device_message =
                    "Turn sync off before pairing or forgetting a device.".into();
                return Err(fdo::Error::Failed(state.device_message.clone()));
            }
            state.device_busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.device_message = "Pairing. Hold the 4D power button for 5 seconds first…".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-pair",
            BusyKind::Device,
            move |shared| {
                let result = pair_device(&shared, &address);
                finish_device(&shared, result);
            },
        )
    }

    fn refresh_device_info(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            if !state.device_connected {
                return Err(fdo::Error::Failed(
                    "no paired Nanoleaf device is saved".into(),
                ));
            }
            state.device_busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.device_info_stale = true;
            state.device_message = "Loading device details…".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-device-info",
            BusyKind::Device,
            move |shared| {
                let config = state_lock(&shared).config.clone();
                let result = device::device_info(&config);
                match result {
                    Ok(info) => {
                        let mut state = state_lock(&shared);
                        state.device_info = info;
                        state.device_info_stale = false;
                        state.device_message = "Device details updated.".into();
                        state.device_busy = false;
                        state.revision = state.revision.wrapping_add(1);
                    }
                    Err(_) => finish_device(
                        &shared,
                        Err("Device is saved, but its details could not be refreshed.".into()),
                    ),
                }
            },
        )
    }

    fn forget_device(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            if state.running || matches!(state.status.as_str(), "starting" | "running" | "stopping")
            {
                state.device_message =
                    "Turn sync off before pairing or forgetting a device.".into();
                return Err(fdo::Error::Failed(state.device_message.clone()));
            }
            state.device_busy = true;
            state.revision = state.revision.wrapping_add(1);
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-forget",
            BusyKind::Device,
            move |shared| {
                let result = forget_device(&shared);
                finish_device(&shared, result);
            },
        )
    }

    #[zbus(name = "StopAndForgetDevice")]
    fn stop_and_forget_device_action(&self) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            if !state.device_connected {
                return Err(fdo::Error::Failed("no Nanoleaf device is paired".into()));
            }
            state.device_busy = true;
            state.revision = state.revision.wrapping_add(1);
            state.device_message = "Stopping sync and disconnecting…".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-stop-and-forget",
            BusyKind::Device,
            move |shared| {
                let result = stop_worker_and_forget_device(&shared);
                finish_device(&shared, result);
            },
        )
    }

    fn clear_error(&self) {
        let mut state = state_lock(&self.shared);
        state.error.clear();
        state.revision = state.revision.wrapping_add(1);
    }
}

#[derive(Clone, Copy)]
enum BusyKind {
    Control,
    Device,
}

fn spawn_task<F>(shared: &Arc<Shared>, name: &str, kind: BusyKind, task: F) -> fdo::Result<()>
where
    F: FnOnce(Arc<Shared>) + Send + 'static,
{
    let task_shared = Arc::clone(shared);
    let error_shared = Arc::clone(shared);
    thread::Builder::new()
        .name(name.into())
        .spawn(move || task(task_shared))
        .map(|_| ())
        .map_err(move |_| {
            let mut state = state_lock(&error_shared);
            match kind {
                BusyKind::Control => {
                    state.busy = false;
                    state.status = "error".into();
                    state.error = "cannot start control operation".into();
                }
                BusyKind::Device => {
                    state.device_busy = false;
                    state.device_message = "Cannot start device operation.".into();
                }
            }
            state.revision = state.revision.wrapping_add(1);
            fdo::Error::Failed("cannot start background operation".into())
        })
}

fn finish_control(
    shared: &Shared,
    result: std::result::Result<(), String>,
    running: Option<bool>,
    enabled: Option<bool>,
    success_status: Option<&str>,
) {
    let mut state = state_lock(shared);
    state.busy = false;
    match result {
        Ok(()) => {
            if let Some(running) = running {
                state.running = running;
            }
            if let Some(enabled) = enabled {
                state.enabled = enabled;
            }
            if let Some(status) = success_status {
                state.status = status.into();
            }
        }
        Err(error) => {
            state.status = "error".into();
            state.error = error;
        }
    }
    state.revision = state.revision.wrapping_add(1);
}

fn finish_stop(shared: &Shared, warning: Option<String>) {
    let mut state = state_lock(shared);
    state.busy = false;
    state.running = false;
    state.worker_present = false;
    state.status = "stopped".into();
    state.error = warning.unwrap_or_default();
    state.revision = state.revision.wrapping_add(1);
}

fn finish_device(shared: &Shared, result: std::result::Result<(), String>) {
    let mut state = state_lock(shared);
    state.device_busy = false;
    if let Err(message) = result {
        state.device_message = message;
    }
    state.revision = state.revision.wrapping_add(1);
}

fn apply_settings(shared: &Shared, updates: &Value) -> std::result::Result<(), String> {
    let path = state_lock(shared).config.config_path.clone();
    if worker_present()? {
        let connection = session_connection()?;
        let proxy = worker_proxy(&connection)?;
        let live: HashMap<String, OwnedValue> = proxy
            .call("GetSettings", &())
            .map_err(|_| "cannot read live worker settings".to_owned())?;
        let mut candidate = state_lock(shared).config.clone();
        candidate
            .apply_settings(&owned_settings_json(live)?)
            .map_err(|error| error.to_string())?;
        candidate
            .apply_settings(updates)
            .map_err(|error| error.to_string())?;
        let values = settings_to_dbus(updates)?;
        let _: () = proxy
            .call("SetSettings", &values)
            .map_err(|error| format!("worker SetSettings failed: {error}"))?;
        let mut state = state_lock(shared);
        state.config = candidate;
        state.worker_present = true;
        state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
        Ok(())
    } else {
        if systemctl_active()? {
            return Err("capture worker is starting or active but not registered on D-Bus; settings were not applied".into());
        }
        let mut candidate = Config::load(&path).map_err(|error| error.to_string())?;
        candidate
            .apply_settings(updates)
            .map_err(|error| error.to_string())?;
        candidate
            .save_settings(updates)
            .map_err(|error| error.to_string())?;
        let mut state = state_lock(shared);
        state.config = candidate;
        state.worker_present = false;
        state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
        Ok(())
    }
}

fn run_worker() -> std::result::Result<(), String> {
    let present = worker_present()?;
    let active = systemctl_active()?;
    if present {
        return if active {
            Ok(())
        } else {
            Err("a standalone CLI worker is running; stop it before starting the service".into())
        };
    }
    systemctl_ok(
        &["start", SYSTEMD_UNIT],
        "could not start wayleaf-4d.service",
    )
}

fn set_start_at_login(enabled: bool) -> std::result::Result<(), String> {
    let action = if enabled { "enable" } else { "disable" };
    systemctl_ok(
        &[action, SYSTEMD_UNIT],
        "could not change start-at-login setting",
    )
}

fn stop_worker_and_power_off(shared: &Shared) -> std::result::Result<Option<String>, String> {
    if worker_present()? {
        worker_stop()?;
    }
    systemctl_ok(&["stop", SYSTEMD_UNIT], "could not stop wayleaf-4d.service")?;
    wait_until_stopped()?;

    {
        let mut state = state_lock(shared);
        state.running = false;
        state.worker_present = false;
        state.status = "stopped".into();
    }

    let config = state_lock(shared).config.clone();
    Ok(device::power_off(&config)
        .err()
        .map(|_| "Sync stopped, but the Nanoleaf could not be powered off.".into()))
}

fn stop_worker_and_forget_device(shared: &Shared) -> std::result::Result<(), String> {
    let worker_running = worker_present()?;
    let service_running = systemctl_active()?;
    if worker_running {
        worker_stop()?;
    }
    if service_running {
        systemctl_ok(&["stop", SYSTEMD_UNIT], "could not stop wayleaf-4d.service")?;
    }
    wait_until_stopped()?;

    {
        let mut state = state_lock(shared);
        state.running = false;
        state.worker_present = false;
        state.status = "stopped".into();
    }

    let config = state_lock(shared).config.clone();
    let warning = if worker_running || service_running {
        device::power_off(&config).err().map(|_| {
            "Warning: Sync stopped, but the Nanoleaf could not be powered off. The saved address and token were removed; controller authorization remains enabled."
                .to_owned()
        })
    } else {
        None
    };
    forget_device(shared)?;
    state_lock(shared).device_message = warning.unwrap_or_default();
    Ok(())
}

fn wait_until_stopped() -> std::result::Result<(), String> {
    let deadline = Instant::now() + SYSTEMCTL_TIMEOUT;
    loop {
        if !worker_present()? && !systemctl_active()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("wayleaf-4d.service did not fully stop".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn select_monitor(shared: &Shared, name: &str) -> std::result::Result<(), String> {
    kms::select_monitor(name).map_err(|error| error.to_string())?;
    let previous = state_lock(shared).config.clone();
    let update = json!({"monitor": name});
    let mut candidate = previous.clone();
    candidate
        .apply_settings(&update)
        .map_err(|error| error.to_string())?;

    let was_enabled = systemctl_enabled()?;
    let active = systemctl_active()?;
    let present = worker_present()?;
    let was_running = present || active;
    {
        let mut state = state_lock(shared);
        state.running = was_running;
        state.enabled = was_enabled;
        state.worker_present = present;
    }
    if present && !active {
        return Err("cannot change the monitor while a standalone CLI worker is running".into());
    }

    if was_running {
        if present {
            worker_stop()?;
        }
        systemctl_ok(
            &["stop", SYSTEMD_UNIT],
            "could not stop wayleaf-4d.service before changing monitor",
        )?;
        wait_until_stopped()?;
        let mut state = state_lock(shared);
        state.running = false;
        state.worker_present = false;
        state.status = "stopped".into();
    }

    if let Err(save_error) = previous.save_settings(&update) {
        let recovery = if was_running {
            systemctl_ok(
                &["start", SYSTEMD_UNIT],
                "could not restart previous worker",
            )
        } else {
            Ok(())
        };
        let mut state = state_lock(shared);
        if recovery.is_ok() && was_running {
            state.running = true;
            state.status = "starting".into();
        }
        if let Err(recovery_error) = recovery {
            state.running = false;
            state.worker_present = false;
            return Err(format!(
                "could not save monitor setting: {save_error}; could not restore previous running state: {recovery_error}"
            ));
        }
        return Err(save_error.to_string());
    }

    let candidate = Config::load(&previous.config_path).unwrap_or(candidate);
    {
        let mut state = state_lock(shared);
        state.config = candidate;
        state.running = false;
        state.worker_present = false;
        state.status = "stopped".into();
        state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
    }
    if was_running {
        systemctl_ok(
            &["start", SYSTEMD_UNIT],
            "monitor was saved but wayleaf-4d.service could not be restarted",
        )?;
        let mut state = state_lock(shared);
        state.running = true;
        state.status = "starting".into();
    }
    Ok(())
}

fn pair_device(shared: &Shared, address: &str) -> std::result::Result<(), String> {
    if worker_present()? || systemctl_active()? {
        return Err("Turn sync off before pairing or forgetting a device.".into());
    }
    let mut config = state_lock(shared).config.clone();
    config.host = address.to_owned();
    device::pair_device(&mut config).map_err(|error| error.to_string())?;
    let info = device::device_info(&config);
    let mut state = state_lock(shared);
    state.config = config;
    state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
    match info {
        Ok(info) => {
            state.device_info = info;
            state.device_info_stale = false;
            state.device_message = "Device paired. Device details updated.".into();
        }
        Err(_) => {
            state.device_info_stale = true;
            state.device_message = "Paired, but device details could not be loaded.".into();
        }
    }
    Ok(())
}

fn forget_device(shared: &Shared) -> std::result::Result<(), String> {
    if worker_present()? || systemctl_active()? {
        return Err("Turn sync off before pairing or forgetting a device.".into());
    }
    let path = state_lock(shared).config.config_path.clone();
    config::forget_device(&path).map_err(|error| error.to_string())?;
    let mut state = state_lock(shared);
    state.config.host.clear();
    state.config.token.clear();
    state.device_connected = false;
    state.device_info = json!({});
    state.device_info_stale = false;
    state.discovered_devices.clear();
    Ok(())
}

fn refresh_state(shared: &Shared) {
    let revision = state_lock(shared).revision;
    let monitors = kms::monitors().map_err(|error| error.to_string());
    let enabled = systemctl_enabled();
    let active = systemctl_active();
    let worker = match session_connection() {
        Ok(connection) => match worker_present_on(&connection) {
            Ok(true) => WorkerSnapshot {
                present: Some(true),
                status: Some(worker_status_on(&connection)),
                settings: Some(worker_settings_on(&connection)),
                error: None,
            },
            Ok(false) => WorkerSnapshot {
                present: Some(false),
                status: None,
                settings: None,
                error: None,
            },
            Err(error) => WorkerSnapshot {
                present: None,
                status: None,
                settings: None,
                error: Some(error),
            },
        },
        Err(error) => WorkerSnapshot {
            present: None,
            status: None,
            settings: None,
            error: Some(error),
        },
    };
    apply_refresh_snapshot(shared, revision, monitors, enabled, active, worker);
}

fn apply_refresh_snapshot(
    shared: &Shared,
    revision: u64,
    monitors: std::result::Result<Vec<kms::Monitor>, String>,
    enabled: std::result::Result<bool, String>,
    active: std::result::Result<bool, String>,
    worker: WorkerSnapshot,
) {
    let WorkerSnapshot {
        present,
        status: worker_status,
        settings: worker_settings,
        error: worker_error,
    } = worker;
    let mut state = state_lock(shared);
    if state.revision != revision {
        return;
    }
    match monitors {
        Ok(monitors) => {
            state.monitors = monitors;
            state.monitor_error.clear();
        }
        Err(error) => state.monitor_error = error,
    }
    if state.busy || state.device_busy {
        return;
    }
    let enabled_error = enabled.as_ref().err().cloned();
    if let Ok(enabled) = enabled {
        state.enabled = enabled;
    }

    let mut refresh_error = worker_error.or(enabled_error);
    match present {
        Some(true) => {
            state.worker_present = true;
            state.running = true;
            match worker_status.unwrap_or_else(|| Err("could not query worker status".into())) {
                Ok(status) if matches!(status.as_str(), "starting" | "running" | "stopping") => {
                    state.status = status;
                    match worker_settings
                        .unwrap_or_else(|| Err("could not query worker settings".into()))
                    {
                        Ok(values) => match owned_settings_json(values) {
                            Ok(values) => {
                                let mut config = state.config.clone();
                                if config.apply_settings(&values).is_ok() {
                                    state.config = config;
                                    state.device_connected = !state.config.host.is_empty()
                                        && !state.config.token.is_empty();
                                } else {
                                    state.status = "error".into();
                                    refresh_error =
                                        Some("worker returned unsupported settings".into());
                                }
                            }
                            Err(error) => {
                                state.status = "error".into();
                                refresh_error = Some(error);
                            }
                        },
                        Err(error) => {
                            state.status = "error".into();
                            refresh_error = Some(error);
                        }
                    }
                }
                Ok(_) => {
                    state.status = "error".into();
                    refresh_error = Some("worker reported an unknown status".into());
                }
                Err(error) => {
                    state.status = "error".into();
                    refresh_error = Some(error);
                }
            }
        }
        Some(false) => {
            state.worker_present = false;
            match active {
                Ok(true) => {
                    state.running = true;
                    state.status = "starting".into();
                }
                Ok(false) => {
                    state.running = false;
                    state.status = "stopped".into();
                }
                Err(error) => refresh_error = Some(error),
            }
        }
        None => {
            if matches!(active, Ok(true)) {
                state.running = true;
                state.status = "starting".into();
            }
        }
    }
    if let Some(error) = refresh_error {
        state.status = "error".into();
        state.error = error;
    }
}

fn session_connection() -> std::result::Result<Connection, String> {
    Builder::session()
        .map_err(|_| "cannot connect to session D-Bus".to_owned())?
        .method_timeout(BUS_CALL_TIMEOUT)
        .build()
        .map_err(|_| "cannot connect to session D-Bus".to_owned())
}
fn worker_present() -> std::result::Result<bool, String> {
    let connection = session_connection()?;
    worker_present_on(&connection)
}

fn worker_present_on(connection: &Connection) -> std::result::Result<bool, String> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(|_| "cannot query session D-Bus".to_owned())?;
    proxy
        .call("NameHasOwner", &WORKER_NAME)
        .map_err(|_| "cannot query worker D-Bus name".to_owned())
}

fn worker_proxy<'a>(connection: &'a Connection) -> std::result::Result<Proxy<'a>, String> {
    Proxy::new(connection, WORKER_NAME, WORKER_PATH, WORKER_INTERFACE)
        .map_err(|_| "cannot connect to worker D-Bus".to_owned())
}

fn worker_preview_on(connection: &Connection) -> String {
    if !worker_present_on(connection).unwrap_or(false) {
        return "[]".into();
    }
    let Ok(proxy) = worker_proxy(connection) else {
        return "[]".into();
    };
    let _: std::result::Result<(), zbus::Error> = proxy.call("SetPreviewEnabled", &true);
    proxy
        .call("GetPreview", &())
        .unwrap_or_else(|_| "[]".into())
}

fn worker_status_on(connection: &Connection) -> std::result::Result<String, String> {
    worker_proxy(connection)?
        .call("GetStatus", &())
        .map_err(|_| "could not query worker status".to_owned())
}

fn worker_settings_on(
    connection: &Connection,
) -> std::result::Result<HashMap<String, OwnedValue>, String> {
    worker_proxy(connection)?
        .call("GetSettings", &())
        .map_err(|_| "could not query worker settings".to_owned())
}

fn worker_stop() -> std::result::Result<(), String> {
    let connection = session_connection()?;
    let proxy = worker_proxy(&connection)?;
    proxy
        .call("Stop", &())
        .map_err(|_| "could not stop existing capture worker".to_owned())
}

fn owned_settings_json(values: HashMap<String, OwnedValue>) -> std::result::Result<Value, String> {
    let mut settings = Map::new();
    for (key, value) in values {
        let value = owned_value_json(&value)
            .ok_or_else(|| "worker returned a setting with an invalid D-Bus type".to_owned())?;
        settings.insert(key, value);
    }
    Ok(Value::Object(settings))
}

fn owned_value_json(value: &OwnedValue) -> Option<Value> {
    if let Ok(value) = bool::try_from(value) {
        return Some(Value::Bool(value));
    }
    if let Ok(value) = i32::try_from(value) {
        return Some(json!(value));
    }
    if let Ok(value) = i64::try_from(value) {
        return Some(json!(value));
    }
    if let Ok(value) = f64::try_from(value) {
        return serde_json::Number::from_f64(value).map(Value::Number);
    }
    String::try_from(value.clone()).ok().map(Value::String)
}

fn settings_to_dbus(settings: &Value) -> std::result::Result<HashMap<String, OwnedValue>, String> {
    let object = settings
        .as_object()
        .ok_or_else(|| "settings must be a JSON object".to_owned())?;
    object
        .iter()
        .map(|(key, value)| Ok((key.clone(), value_to_owned(key, value)?)))
        .collect()
}

fn value_to_owned(key: &str, value: &Value) -> std::result::Result<OwnedValue, String> {
    let invalid = || "setting has an invalid or unsupported D-Bus value".to_owned();
    match key {
        "brightness" | "saturation_boost" | "smooth" | "black_threshold" => {
            let number = value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(invalid)?;
            Ok(OwnedValue::from(number))
        }
        "fps" | "max_step" | "edge_depth" | "center_bleed" | "peak_mix" | "feather_depth" => {
            let number = value
                .as_i64()
                .and_then(|number| i32::try_from(number).ok())
                .ok_or_else(invalid)?;
            Ok(OwnedValue::from(number))
        }
        "letterbox" | "midtone_lift" | "strict_black" => {
            value.as_bool().map(OwnedValue::from).ok_or_else(invalid)
        }
        "monitor" => value
            .as_str()
            .map(|value| OwnedValue::from(Str::from(value)))
            .ok_or_else(invalid),
        _ => Err("unknown setting key".into()),
    }
}

fn systemctl(args: &[&str]) -> std::result::Result<Output, String> {
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "could not start systemctl".to_owned())?;
    let deadline = Instant::now() + SYSTEMCTL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                terminate(&mut child);
                return Err("systemctl operation timed out".into());
            }
            Err(_) => {
                terminate(&mut child);
                return Err("could not query systemctl process".into());
            }
        }
    }
    child
        .wait_with_output()
        .map_err(|_| "could not read systemctl result".to_owned())
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn systemctl_ok(args: &[&str], error: &str) -> std::result::Result<(), String> {
    match systemctl(args) {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(error.into()),
    }
}

fn systemctl_enabled() -> std::result::Result<bool, String> {
    let output = systemctl(&["is-enabled", SYSTEMD_UNIT])?;
    let state = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        return Ok(state.trim() == "enabled");
    }
    if matches!(
        state.trim(),
        "disabled"
            | "static"
            | "masked"
            | "not-found"
            | "indirect"
            | "generated"
            | "transient"
            | "alias"
    ) {
        Ok(false)
    } else {
        Err("could not query wayleaf-4d.service enablement".into())
    }
}

fn systemctl_active() -> std::result::Result<bool, String> {
    let output = systemctl(&["is-active", SYSTEMD_UNIT])?;
    let state = String::from_utf8_lossy(&output.stdout);
    match state.trim() {
        "active" | "activating" | "deactivating" => Ok(true),
        "inactive" | "failed" | "unknown" | "not-found" => Ok(false),
        _ if output.status.success() => Ok(true),
        _ => Err("could not query wayleaf-4d.service state".into()),
    }
}

fn discover_devices() -> std::result::Result<Vec<Value>, String> {
    let connection = Builder::system()
        .map_err(|_| "System mDNS is unavailable. Enter the device IP manually.".to_owned())?
        .method_timeout(BUS_CALL_TIMEOUT)
        .build()
        .map_err(|_| "System mDNS is unavailable. Enter the device IP manually.".to_owned())?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(AVAHI_NAME)
        .map_err(|_| "could not listen for mDNS results. Enter the device IP manually.".to_owned())?
        .interface(AVAHI_BROWSER_INTERFACE)
        .map_err(|_| "could not listen for mDNS results. Enter the device IP manually.".to_owned())?
        .member("ItemNew")
        .map_err(|_| "could not listen for mDNS results. Enter the device IP manually.".to_owned())?
        .build();
    let mut signals =
        MessageIterator::for_match_rule(rule, &connection, Some(64)).map_err(|_| {
            "could not listen for mDNS results. Enter the device IP manually.".to_owned()
        })?;
    let server = Proxy::new(
        &connection,
        AVAHI_NAME,
        AVAHI_SERVER_PATH,
        AVAHI_SERVER_INTERFACE,
    )
    .map_err(|_| "Avahi is unavailable. Enter the device IP manually.".to_owned())?;
    let browser: OwnedObjectPath = server
        .call(
            "ServiceBrowserNew",
            &(-1_i32, -1_i32, NANOLEAF_SERVICE, "local", 0_u32),
        )
        .map_err(|_| "Avahi discovery failed. Enter the device IP manually.".to_owned())?;
    let browser_path = browser.to_string();
    let timer_connection = connection.clone();
    let timer = thread::Builder::new()
        .name("wayleaf-avahi-deadline".into())
        .spawn(move || {
            thread::sleep(DISCOVERY_WINDOW);
            let _ = timer_connection.close();
        })
        .map_err(|_| "cannot start Avahi discovery deadline".to_owned())?;

    let deadline = Instant::now() + DISCOVERY_WINDOW;
    let mut devices = Vec::new();
    let mut resolution_error = None;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        let Some(Ok(message)) = signals.next() else {
            break;
        };
        if message.header().path().map(ToString::to_string).as_deref()
            != Some(browser_path.as_str())
        {
            continue;
        }
        let Ok((interface, protocol, name, service, domain, _flags)) = message
            .body()
            .deserialize::<(i32, i32, String, String, String, u32)>()
        else {
            continue;
        };
        if service != NANOLEAF_SERVICE {
            continue;
        }
        let args: AvahiResolveServiceArgs =
            (interface, protocol, name.clone(), service, domain, -1, 0);
        let resolved: std::result::Result<AvahiResolvedService, _> =
            server.call("ResolveService", &args);
        match resolved {
            Ok((_, _, _, _, _, _, _, address, _, _, _)) => {
                push_discovered(&mut devices, name, &address);
            }
            Err(error) => {
                resolution_error.get_or_insert_with(|| {
                    format!(
                        "Avahi could not resolve Nanoleaf service {name}: {error}. Enter the device IP manually."
                    )
                });
            }
        }
    }
    let _ = timer.join();
    if let Some(error) = resolution_error {
        return Err(error);
    }
    Ok(devices)
}

fn push_discovered(devices: &mut Vec<Value>, name: String, host: &str) -> bool {
    if Ipv4Addr::from_str(host).is_err()
        || devices
            .iter()
            .any(|device| device.get("host").and_then(Value::as_str) == Some(host))
    {
        return false;
    }
    devices.push(json!({ "name": name, "host": host }));
    true
}

pub fn self_test() -> Result<()> {
    if Ipv4Addr::from_str("192.0.2.1").is_err()
        || Ipv4Addr::from_str("localhost").is_ok()
        || Ipv4Addr::from_str("256.1.1.1").is_ok()
    {
        return Err(AppError::new("control IPv4 validation self-test failed"));
    }
    let mut devices = Vec::new();
    if !push_discovered(&mut devices, "first".into(), "192.0.2.1")
        || push_discovered(&mut devices, "duplicate".into(), "192.0.2.1")
        || push_discovered(&mut devices, "invalid".into(), "not-an-ip")
        || devices.len() != 1
    {
        return Err(AppError::new(
            "control discovery deduplication self-test failed",
        ));
    }
    let config = Config {
        host: "192.0.2.1".into(),
        token: "secret-marker".into(),
        ..Config::default()
    };
    let state = State::new(config, Vec::new());
    let json = serde_json::to_string(&state.json())
        .map_err(|_| AppError::new("control state serialization self-test failed"))?;
    if json.contains("secret-marker") || json.contains("\"token\"") {
        return Err(AppError::new("control state exposed credential fields"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_devices_are_ipv4_and_deduplicated() {
        let mut devices = Vec::new();
        assert!(push_discovered(&mut devices, "4D".into(), "192.0.2.1"));
        assert!(!push_discovered(
            &mut devices,
            "duplicate".into(),
            "192.0.2.1"
        ));
        assert!(!push_discovered(&mut devices, "IPv6".into(), "2001:db8::1"));
        assert_eq!(devices, vec![json!({"name":"4D", "host":"192.0.2.1"})]);
    }

    #[test]
    fn closed_preview_returns_no_sample_data() {
        let api = ControlApi {
            shared: Arc::new(Shared {
                state: Mutex::new(State::new(Config::default(), Vec::new())),
            }),
        };
        assert_eq!(api.get_preview(), "[]");
    }

    #[test]
    fn stop_and_forget_rejects_an_overlapping_control_operation() {
        let mut state = State::new(Config::default(), Vec::new());
        state.busy = true;
        state.device_connected = true;
        let api = ControlApi {
            shared: Arc::new(Shared {
                state: Mutex::new(state),
            }),
        };

        assert!(api.stop_and_forget_device_action().is_err());
        let state = state_lock(&api.shared);
        assert!(state.busy);
        assert!(!state.device_busy);
        assert!(state.device_connected);
    }

    #[test]
    fn oversized_settings_json_is_rejected_before_parsing() {
        let api = ControlApi {
            shared: Arc::new(Shared {
                state: Mutex::new(State::new(Config::default(), Vec::new())),
            }),
        };
        let error = api
            .set_settings("x".repeat(MAX_SETTINGS_BYTES + 1))
            .unwrap_err();
        assert!(matches!(
            error,
            fdo::Error::InvalidArgs(message) if message == "settings JSON exceeds the 4 KiB limit"
        ));
    }

    #[test]
    fn avahi_resolve_service_signatures_match_installed_xml() {
        assert_eq!(
            <AvahiResolveServiceArgs as zbus::zvariant::Type>::SIGNATURE.to_string(),
            "(iisssiu)"
        );
        assert_eq!(
            <AvahiResolvedService as zbus::zvariant::Type>::SIGNATURE.to_string(),
            "(iissssisqaayu)"
        );
    }

    #[test]
    fn run_state_and_login_preference_are_independent() {
        for running in [false, true] {
            for enabled in [false, true] {
                let mut state = State::new(Config::default(), Vec::new());
                state.running = running;
                state.enabled = enabled;
                let json = state.json();
                assert_eq!(json["running"], running);
                assert_eq!(json["enabled"], enabled);
            }
        }
    }

    #[test]
    fn state_json_never_contains_credentials() {
        let config = Config {
            host: "192.0.2.1".into(),
            token: "secret-marker".into(),
            ..Config::default()
        };
        let state = State::new(config, Vec::new());
        let json = serde_json::to_string(&state.json()).unwrap();
        assert!(json.contains("192.0.2.1"));
        assert!(!json.contains("secret-marker"));
        assert!(!json.contains("\"token\""));
    }

    #[test]
    fn integral_float_settings_stay_dbus_doubles() {
        let mut values =
            settings_to_dbus(&json!({"brightness":1,"saturation_boost":2,"fps":10})).unwrap();
        for key in ["brightness", "saturation_boost"] {
            assert!(f64::try_from(values.remove(key).unwrap()).is_ok());
        }
        assert_eq!(i32::try_from(values.remove("fps").unwrap()).unwrap(), 10);
    }

    fn test_monitor(name: &str) -> kms::Monitor {
        kms::Monitor {
            name: name.into(),
            card_path: "/dev/dri/card0".into(),
            connector_id: 1,
            width: 1920,
            height: 1080,
        }
    }

    fn stopped_worker() -> WorkerSnapshot {
        WorkerSnapshot {
            present: Some(false),
            status: None,
            settings: None,
            error: None,
        }
    }

    #[test]
    fn stale_refresh_cannot_overwrite_completed_control_state() {
        let shared = Shared {
            state: Mutex::new(State::new(Config::default(), vec![test_monitor("DP-1")])),
        };
        {
            let mut state = state_lock(&shared);
            state.revision = 1;
            state.running = true;
            state.enabled = true;
            state.worker_present = true;
            state.status = "running".into();
        }

        apply_refresh_snapshot(
            &shared,
            0,
            Ok(Vec::new()),
            Ok(false),
            Ok(false),
            stopped_worker(),
        );

        let state = state_lock(&shared);
        assert!(state.running);
        assert!(state.enabled);
        assert!(state.worker_present);
        assert_eq!(state.status, "running");
        assert_eq!(state.monitors.len(), 1);
    }

    #[test]
    fn monitor_refresh_errors_keep_last_list_and_hotplug_refresh_replaces_it() {
        let shared = Shared {
            state: Mutex::new(State::new(Config::default(), vec![test_monitor("DP-1")])),
        };

        apply_refresh_snapshot(
            &shared,
            0,
            Err("cannot enumerate DRM devices".into()),
            Ok(false),
            Ok(false),
            stopped_worker(),
        );
        {
            let state = state_lock(&shared);
            assert_eq!(state.monitors.len(), 1);
            assert_eq!(state.monitor_error, "cannot enumerate DRM devices");
        }

        apply_refresh_snapshot(
            &shared,
            0,
            Ok(Vec::new()),
            Ok(false),
            Ok(false),
            stopped_worker(),
        );
        let state = state_lock(&shared);
        assert!(state.monitors.is_empty());
        assert!(state.monitor_error.is_empty());
    }
}
