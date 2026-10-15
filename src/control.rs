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
type AvahiResolvedService = (
    i32,
    i32,
    String,
    String,
    String,
    String,
    u32,
    String,
    u16,
    Vec<Vec<u8>>,
);

struct Shared {
    state: Mutex<State>,
}

struct State {
    config: Config,
    status: String,
    error: String,
    enabled: bool,
    busy: bool,
    worker_present: bool,
    device_connected: bool,
    device_busy: bool,
    device_info: Value,
    discovered_devices: Vec<Value>,
    device_message: String,
    monitors: Vec<kms::Monitor>,
}

impl State {
    fn new(config: Config, monitors: Vec<kms::Monitor>) -> Self {
        let device_connected = !config.host.is_empty() && !config.token.is_empty();
        Self {
            config,
            status: "stopped".into(),
            error: String::new(),
            enabled: false,
            busy: false,
            worker_present: false,
            device_connected,
            device_busy: false,
            device_info: json!({}),
            discovered_devices: Vec::new(),
            device_message: String::new(),
            monitors,
        }
    }

    fn json(&self) -> Value {
        json!({
            "settings": self.config.settings_json(),
            "status": self.status,
            "error": self.error,
            "enabled": self.enabled,
            "busy": self.busy,
            "deviceConnected": self.device_connected,
            "deviceBusy": self.device_busy,
            "deviceHost": self.config.host,
            "deviceInfo": self.device_info,
            "discoveredDevices": self.discovered_devices,
            "deviceMessage": self.device_message,
            "monitors": self.monitors,
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
        initial.error = error;
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

    fn set_settings(&self, settings_json: String) -> fdo::Result<()> {
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
            state.error.clear();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-settings",
            BusyKind::Control,
            move |shared| {
                finish_control(&shared, apply_settings(&shared, &updates), None, None);
            },
        )
    }

    fn set_enabled(&self, enabled: bool) -> fdo::Result<()> {
        {
            let mut state = state_lock(&self.shared);
            if state.busy || state.device_busy {
                return Err(fdo::Error::Failed(
                    "another control operation is in progress".into(),
                ));
            }
            state.busy = true;
            state.error.clear();
            state.status = if enabled { "starting" } else { "stopping" }.into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-lifecycle",
            BusyKind::Control,
            move |shared| {
                let result = set_enabled(&shared, enabled);
                finish_control(
                    &shared,
                    result,
                    Some(enabled),
                    Some(if enabled { "starting" } else { "stopped" }),
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
            state.error.clear();
            if state.worker_present || state.status == "running" || state.status == "starting" {
                state.status = "stopping".into();
            }
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-monitor",
            BusyKind::Control,
            move |shared| {
                let result = select_monitor(&shared, &name);
                let status = if state_lock(&shared).enabled {
                    "starting"
                } else {
                    "stopped"
                };
                finish_control(&shared, result, None, Some(status));
            },
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
            if state.enabled || state.status != "stopped" {
                state.device_message =
                    "Turn sync off before pairing or forgetting a device.".into();
                return Err(fdo::Error::Failed(state.device_message.clone()));
            }
            state.device_busy = true;
            state.device_info = json!({});
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
            state.device_info = json!({});
            state.device_message = "Loading device details…".into();
        }
        spawn_task(
            &self.shared,
            "wayleaf-control-device-info",
            BusyKind::Device,
            move |shared| {
                let config = state_lock(&shared).config.clone();
                let result = device::device_info(&config).map_err(|error| error.to_string());
                match result {
                    Ok(info) => {
                        let mut state = state_lock(&shared);
                        state.device_info = info;
                        state.device_message = "Device details updated.".into();
                        state.device_busy = false;
                    }
                    Err(error) => finish_device(
                        &shared,
                        Err(format!(
                            "Device is saved, but its details could not be loaded. {error}"
                        )),
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
            if state.enabled || state.status != "stopped" {
                state.device_message =
                    "Turn sync off before pairing or forgetting a device.".into();
                return Err(fdo::Error::Failed(state.device_message.clone()));
            }
            state.device_busy = true;
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

    fn clear_error(&self) {
        state_lock(&self.shared).error.clear();
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
            fdo::Error::Failed("cannot start background operation".into())
        })
}

fn finish_control(
    shared: &Shared,
    result: std::result::Result<(), String>,
    enabled: Option<bool>,
    success_status: Option<&str>,
) {
    let mut state = state_lock(shared);
    state.busy = false;
    match result {
        Ok(()) => {
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
}

fn finish_device(shared: &Shared, result: std::result::Result<(), String>) {
    let mut state = state_lock(shared);
    state.device_busy = false;
    if let Err(message) = result {
        state.device_message = message;
    }
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
        candidate.save().map_err(|error| error.to_string())?;
        let mut state = state_lock(shared);
        state.config = candidate;
        state.worker_present = false;
        state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
        Ok(())
    }
}

fn set_enabled(shared: &Shared, enabled: bool) -> std::result::Result<(), String> {
    let present = worker_present()?;
    if enabled {
        let active = systemctl_active()?;
        let args: &[&str] = if present || active {
            &["enable", SYSTEMD_UNIT]
        } else {
            &["enable", "--now", SYSTEMD_UNIT]
        };
        systemctl_ok(args, "could not enable and start wayleaf-4d.service")?;
        let mut state = state_lock(shared);
        state.worker_present = present;
        return Ok(());
    }
    if present {
        worker_stop()?;
    }
    systemctl_ok(
        &["disable", "--now", SYSTEMD_UNIT],
        "could not disable wayleaf-4d.service",
    )?;
    state_lock(shared).worker_present = false;
    Ok(())
}

fn select_monitor(shared: &Shared, name: &str) -> std::result::Result<(), String> {
    kms::select_monitor(name).map_err(|error| error.to_string())?;
    let path = state_lock(shared).config.config_path.clone();
    let present = worker_present()?;
    let enabled = systemctl_enabled()?;
    let active = systemctl_active()?;
    if present {
        worker_stop()?;
    }
    if active || enabled {
        systemctl_ok(
            &["stop", SYSTEMD_UNIT],
            "could not stop wayleaf-4d.service before changing monitor",
        )?;
    }
    let mut candidate = Config::load(&path).map_err(|error| error.to_string())?;
    candidate
        .apply_settings(&json!({"monitor": name}))
        .map_err(|error| error.to_string())?;
    candidate.save().map_err(|error| error.to_string())?;
    {
        let mut state = state_lock(shared);
        state.config = candidate;
        state.enabled = enabled;
        state.worker_present = false;
        state.device_connected = !state.config.host.is_empty() && !state.config.token.is_empty();
    }
    if enabled {
        systemctl_ok(
            &["start", SYSTEMD_UNIT],
            "could not restart wayleaf-4d.service with the selected monitor",
        )?;
    }
    Ok(())
}

fn pair_device(shared: &Shared, address: &str) -> std::result::Result<(), String> {
    if worker_present()? || systemctl_enabled()? || systemctl_active()? {
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
            state.device_message = "Device paired. Device details updated.".into();
        }
        Err(_) => {
            state.device_info = json!({});
            state.device_message = "Paired, but device details could not be loaded.".into();
        }
    }
    Ok(())
}

fn forget_device(shared: &Shared) -> std::result::Result<(), String> {
    if worker_present()? || systemctl_enabled()? || systemctl_active()? {
        return Err("Turn sync off before pairing or forgetting a device.".into());
    }
    let path = state_lock(shared).config.config_path.clone();
    config::forget_device(&path).map_err(|error| error.to_string())?;
    let mut state = state_lock(shared);
    state.config.host.clear();
    state.config.token.clear();
    state.device_connected = false;
    state.device_info = json!({});
    state.discovered_devices.clear();
    state.device_message = "Saved credentials removed from this computer. API access on the controller was not revoked.".into();
    Ok(())
}

fn refresh_state(shared: &Shared) {
    let enabled = systemctl_enabled().ok();
    let connection = match session_connection() {
        Ok(connection) => connection,
        Err(_) => return,
    };
    let present = match worker_present_on(&connection) {
        Ok(present) => present,
        Err(_) => return,
    };
    let worker_status = if present {
        Some(worker_status_on(&connection))
    } else {
        None
    };
    let worker_settings = if present {
        Some(worker_settings_on(&connection))
    } else {
        None
    };
    let service_active = if !present && enabled == Some(true) {
        systemctl_active().ok()
    } else {
        None
    };

    let mut state = state_lock(shared);
    if state.busy {
        return;
    }
    if let Some(enabled) = enabled {
        state.enabled = enabled;
    }
    state.worker_present = present;
    if present {
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
                                state.device_connected =
                                    !state.config.host.is_empty() && !state.config.token.is_empty();
                            } else {
                                state.status = "error".into();
                                state.error = "worker returned unsupported settings".into();
                            }
                        }
                        Err(error) => {
                            state.status = "error".into();
                            state.error = error;
                        }
                    },
                    Err(error) => {
                        state.status = "error".into();
                        state.error = error;
                    }
                }
            }
            Ok(_) => {
                state.status = "error".into();
                state.error = "worker reported an unknown status".into();
            }
            Err(error) => {
                state.status = "error".into();
                state.error = error;
            }
        }
    } else if !state.enabled {
        state.status = "stopped".into();
    } else {
        match service_active {
            Some(true) => state.status = "starting".into(),
            Some(false) => {
                state.status = "error".into();
                state.error = "Wayleaf 4D service is not active; inspect journalctl --user -u wayleaf-4d.service".into();
            }
            None => {}
        }
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
        let resolved: std::result::Result<AvahiResolvedService, _> = server.call(
            "ResolveService",
            &(
                interface,
                protocol,
                name.clone(),
                service,
                domain,
                0_u32,
                0_u32,
            ),
        );
        if let Ok((_, _, _, _, _, _, _, address, _, _)) = resolved {
            push_discovered(&mut devices, name, &address);
        }
    }
    let _ = timer.join();
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
}
