use crate::{AppError, Result};
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{convert::TryFrom, env};

const MAX_FPS: i32 = 10;
static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub token: String,
    pub config_path: PathBuf,
    pub monitor: String,
    pub mode: String,
    pub static_color: String,
    pub fps: i32,
    pub port: i32,
    pub max_step: i32,
    pub edge_depth: i32,
    pub center_bleed: i32,
    pub peak_mix: i32,
    pub feather_depth: i32,
    pub brightness: f32,
    pub smooth: f32,
    pub black_threshold: f32,
    pub saturation_boost: f32,
    pub letterbox: bool,
    pub midtone_lift: bool,
    pub strict_black: bool,
    pub start_corner: String,
    pub reverse_direction: bool,
    pub perimeter_offset: i32,
}

impl Default for Config {
    fn default() -> Self {
        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        Self {
            host: String::new(),
            token: String::new(),
            config_path: config_dir.join("nanoleaf-sync/config.json"),
            monitor: String::new(),
            mode: "4D".into(),
            static_color: "#ffffff".into(),
            fps: MAX_FPS,
            port: 60222,
            max_step: 12,
            edge_depth: 8,
            center_bleed: 0,
            peak_mix: 15,
            feather_depth: 5,
            brightness: 1.5,
            smooth: 0.35,
            black_threshold: 0.01,
            saturation_boost: 1.0,
            letterbox: true,
            midtone_lift: false,
            strict_black: false,
            start_corner: "bottom_center".into(),
            reverse_direction: false,
            perimeter_offset: 0,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut config = Self {
            config_path: path.to_path_buf(),
            ..Self::default()
        };
        let Some(root) = read_root(path)? else {
            return Ok(config);
        };
        load_values(&mut config, &root)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !matches!(self.mode.as_str(), "1D" | "2D" | "3D" | "4D")
            || !valid_static_color(&self.static_color)
            || self.port < 1
            || self.port > 65_535
            || !(1..=MAX_FPS).contains(&self.fps)
            || !(1..=255).contains(&self.max_step)
            || !(5..=20).contains(&self.edge_depth)
            || !(0..=50).contains(&self.center_bleed)
            || !(0..=35).contains(&self.peak_mix)
            || !(0..=20).contains(&self.feather_depth)
            || !in_range(self.brightness, 0.0, 2.0)
            || !in_range(self.smooth, 0.05, 1.0)
            || !in_range(self.black_threshold, 0.0, 0.1)
            || !in_range(self.saturation_boost, 0.0, 3.0)
            || !matches!(
                self.start_corner.as_str(),
                "bottom_center" | "bottom_left" | "top_left" | "top_right" | "bottom_right"
            )
            || !(-39..=39).contains(&self.perimeter_offset)
        {
            return Err(AppError::new(
                "configuration value outside its supported range",
            ));
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        update_root(&self.config_path, true, |object, _| {
            for key in ["corner", "offset", "reverse"] {
                object.remove(key);
            }
            if !self.host.is_empty() {
                object.insert("ip".into(), Value::String(self.host.clone()));
            }
            if !self.token.is_empty() {
                object.insert("token".into(), Value::String(self.token.clone()));
            }
            write_known_settings(object, self);
            Ok(())
        })
    }

    pub fn save_settings(&self, settings: &Value) -> Result<()> {
        update_root(&self.config_path, true, |object, current| {
            let mut next = current.clone();
            next.apply_settings(settings)?;
            for key in ["corner", "offset", "reverse"] {
                object.remove(key);
            }
            let values = next.settings_json();
            for key in settings
                .as_object()
                .ok_or_else(|| AppError::new("settings must be a JSON object"))?
                .keys()
            {
                object.insert(key.clone(), values[key].clone());
            }
            for key in ["start_corner", "reverse_direction", "perimeter_offset"] {
                object.insert(key.into(), values[key].clone());
            }
            Ok(())
        })
    }

    pub fn save_credentials(&self) -> Result<()> {
        update_root(&self.config_path, true, |object, _| {
            if !self.host.is_empty() {
                object.insert("ip".into(), Value::String(self.host.clone()));
            }
            if !self.token.is_empty() {
                object.insert("token".into(), Value::String(self.token.clone()));
            }
            Ok(())
        })
    }

    pub fn settings_json(&self) -> Value {
        json!({
            "monitor": self.monitor,
            "mode": self.mode,
            "static_color": self.static_color,
            "fps": self.fps,
            "max_step": self.max_step,
            "edge_depth": self.edge_depth,
            "center_bleed": self.center_bleed,
            "peak_mix": self.peak_mix,
            "feather_depth": self.feather_depth,
            "brightness": self.brightness,
            "smooth": self.smooth,
            "black_threshold": self.black_threshold,
            "saturation_boost": self.saturation_boost,
            "start_corner": self.start_corner,
            "reverse_direction": self.reverse_direction,
            "perimeter_offset": self.perimeter_offset,
            "letterbox": self.letterbox,
            "midtone_lift": self.midtone_lift,
            "strict_black": self.strict_black,
        })
    }

    pub fn apply_settings(&mut self, values: &Value) -> Result<()> {
        let settings = values
            .as_object()
            .ok_or_else(|| AppError::new("settings must be a JSON object"))?;
        let mut next = self.clone();
        for (key, value) in settings {
            match key.as_str() {
                "monitor" => next.monitor = json_string(value, key)?,
                "mode" => next.mode = json_string(value, key)?,
                "static_color" => next.static_color = json_string(value, key)?,
                "fps" => next.fps = json_i32(value, key)?,
                "max_step" => next.max_step = json_i32(value, key)?,
                "edge_depth" => next.edge_depth = json_i32(value, key)?,
                "center_bleed" => next.center_bleed = json_i32(value, key)?,
                "peak_mix" => next.peak_mix = json_i32(value, key)?,
                "feather_depth" => next.feather_depth = json_i32(value, key)?,
                "brightness" => next.brightness = json_f32(value, key)?,
                "smooth" => next.smooth = json_f32(value, key)?,
                "black_threshold" => next.black_threshold = json_f32(value, key)?,
                "saturation_boost" => next.saturation_boost = json_f32(value, key)?,
                "letterbox" => next.letterbox = json_bool(value, key)?,
                "midtone_lift" => next.midtone_lift = json_bool(value, key)?,
                "strict_black" => next.strict_black = json_bool(value, key)?,
                "start_corner" => next.start_corner = json_string(value, key)?,
                "reverse_direction" => next.reverse_direction = json_bool(value, key)?,
                "perimeter_offset" => next.perimeter_offset = json_i32(value, key)?,
                _ => return Err(AppError::new("unknown setting key")),
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
}

pub fn forget_device(path: &Path) -> Result<()> {
    update_root(path, false, |object, _| {
        object.remove("ip");
        object.remove("token");
        Ok(())
    })
}

fn update_root(
    path: &Path,
    create_if_missing: bool,
    update: impl FnOnce(&mut Map<String, Value>, &Config) -> Result<()>,
) -> Result<()> {
    with_config_lock(path, || {
        let mut root = match read_root(path)? {
            Some(root) => root,
            None if create_if_missing => Value::Object(Map::new()),
            None => return Ok(()),
        };
        let mut current = Config {
            config_path: path.to_path_buf(),
            ..Config::default()
        };
        load_values(&mut current, &root)?;
        current.validate()?;
        update(
            root.as_object_mut()
                .ok_or_else(|| AppError::new("config root must be an object"))?,
            &current,
        )?;
        let data = serde_json::to_vec_pretty(&root)
            .map_err(|_| AppError::new("cannot encode configuration"))?;
        write_atomic(path, &data)
    })
}

fn lock_path(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::new("invalid config file path"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut name = std::ffi::OsString::from(".");
    name.push(file_name);
    name.push(".lock");
    Ok(parent.join(name))
}

fn with_config_lock<T>(path: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(parent)
        .map_err(|_| AppError::new("cannot create config directory"))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(lock_path(path)?)
        .map_err(|_| AppError::new("cannot open config lock"))?;
    loop {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0 {
            break;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(AppError::new("cannot lock config file"));
        }
    }
    let _lock = lock;
    operation()
}

fn in_range(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && value >= min && value <= max
}

fn valid_static_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn read_root(path: &Path) -> Result<Option<Value>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AppError::new("cannot read config file")),
    };
    let root: Value =
        serde_json::from_str(&text).map_err(|_| AppError::new("invalid config JSON"))?;
    let object = root
        .as_object()
        .ok_or_else(|| AppError::new("config root must be an object"))?;
    let has_credentials = ["ip", "token"].iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    });
    let group_or_other_readable =
        fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o044 != 0);
    if has_credentials && group_or_other_readable {
        eprintln!(
            "warning: config file is readable by group or other while credentials are present"
        );
    }
    Ok(Some(root))
}

fn load_values(config: &mut Config, root: &Value) -> Result<()> {
    let object = root
        .as_object()
        .ok_or_else(|| AppError::new("config root must be an object"))?;
    if let Some(value) = object.get("ip") {
        config.host = json_string(value, "ip")?;
    }
    if let Some(value) = object.get("token") {
        config.token = json_string(value, "token")?;
    }
    if let Some(value) = object.get("monitor") {
        config.monitor = json_string(value, "monitor")?;
    }
    if let Some(value) = object.get("mode") {
        config.mode = json_string(value, "mode")?;
    }
    if let Some(value) = object.get("static_color") {
        config.static_color = json_string(value, "static_color")?;
    }
    if let Some(value) = object.get("port") {
        config.port = json_i32(value, "port")?;
    }
    if let Some(value) = object.get("fps") {
        config.fps = legacy_fps(value)?;
    }
    if let Some(value) = object.get("max_step") {
        config.max_step = json_i32(value, "max_step")?;
    }
    if let Some(value) = object.get("edge_depth") {
        config.edge_depth = json_i32(value, "edge_depth")?;
    }
    if let Some(value) = object.get("center_bleed") {
        config.center_bleed = json_i32(value, "center_bleed")?;
    }
    if let Some(value) = object.get("peak_mix") {
        config.peak_mix = json_i32(value, "peak_mix")?;
    }
    if let Some(value) = object.get("feather_depth") {
        config.feather_depth = json_i32(value, "feather_depth")?;
    }
    if let Some(value) = object.get("brightness") {
        config.brightness = json_f32(value, "brightness")?;
    }
    if let Some(value) = object.get("smooth") {
        config.smooth = json_f32(value, "smooth")?;
    }
    if let Some(value) = object.get("black_threshold") {
        config.black_threshold = json_f32(value, "black_threshold")?;
    }
    if let Some(value) = object.get("saturation_boost") {
        config.saturation_boost = json_f32(value, "saturation_boost")?;
    }
    if let Some(value) = object.get("letterbox") {
        config.letterbox = json_bool(value, "letterbox")?;
    }
    if let Some(value) = object.get("midtone_lift") {
        config.midtone_lift = json_bool(value, "midtone_lift")?;
    }
    if let Some(value) = object.get("strict_black") {
        config.strict_black = json_bool(value, "strict_black")?;
    }
    if !object.contains_key("start_corner") {
        if let Some(corner) = object.get("corner").and_then(Value::as_str) {
            config.start_corner = match corner {
                "bottom-center" => "bottom_center",
                "bottom-left" => "bottom_left",
                "top-left" => "top_left",
                "top-right" => "top_right",
                "bottom-right" => "bottom_right",
                _ => config.start_corner.as_str(),
            }
            .to_owned();
        }
    }
    if !object.contains_key("reverse_direction") {
        if let Some(reverse) = object.get("reverse").and_then(Value::as_bool) {
            config.reverse_direction = reverse;
        }
    }
    if !object.contains_key("perimeter_offset") {
        if let Some(offset) = object.get("offset").and_then(|value| json_i32(value, "offset").ok()) {
            if (-39..=39).contains(&offset) {
                config.perimeter_offset = offset;
            }
        }
    }
    if let Some(value) = object.get("start_corner") {
        config.start_corner = json_string(value, "start_corner")?;
    }
    if let Some(value) = object.get("reverse_direction") {
        config.reverse_direction = json_bool(value, "reverse_direction")?;
    }
    if let Some(value) = object.get("perimeter_offset") {
        config.perimeter_offset = json_i32(value, "perimeter_offset")?;
    }
    Ok(())
}

fn json_string(value: &Value, key: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::new(format!("config field {key} must be a string")))
}

fn json_bool(value: &Value, key: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| AppError::new(format!("config field {key} must be a boolean")))
}

fn json_i32(value: &Value, key: &str) -> Result<i32> {
    let integer = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| AppError::new(format!("config field {key} must be a 32-bit integer")))?;
    Ok(integer)
}

fn legacy_fps(value: &Value) -> Result<i32> {
    if let Some(integer) = value.as_i64() {
        return Ok(if integer > i64::from(MAX_FPS) {
            MAX_FPS
        } else {
            i32::try_from(integer)
                .map_err(|_| AppError::new("config field fps is outside its supported range"))?
        });
    }
    if let Some(integer) = value.as_u64() {
        return Ok(if integer > MAX_FPS as u64 {
            MAX_FPS
        } else {
            integer as i32
        });
    }
    Err(AppError::new("config field fps must be an integer"))
}

fn json_f32(value: &Value, key: &str) -> Result<f32> {
    let number = value
        .as_f64()
        .filter(|number| number.is_finite())
        .map(|number| number as f32)
        .filter(|number| number.is_finite())
        .ok_or_else(|| AppError::new(format!("config field {key} must be finite")))?;
    Ok(number)
}

fn write_known_settings(object: &mut Map<String, Value>, config: &Config) {
    let settings = config.settings_json();
    object.insert("port".into(), json!(config.port));
    for (key, value) in settings.as_object().expect("settings_json is an object") {
        object.insert(key.clone(), value.clone());
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::new("invalid config file path"))?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(parent)
        .map_err(|_| AppError::new("cannot create config directory"))?;

    let mut created = None;
    for _ in 0..32 {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let mut name = std::ffi::OsString::from(".");
        name.push(file_name);
        name.push(format!(".{}.{}.tmp", std::process::id(), id));
        let temporary = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => {
                created = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(AppError::new("cannot create temporary config file")),
        }
    }
    let (temporary, mut file) =
        created.ok_or_else(|| AppError::new("cannot create temporary config file"))?;

    let result = (|| -> std::io::Result<()> {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::new("cannot atomically save config file"));
    }
    Ok(())
}

pub fn self_test() -> Result<()> {
    let path = env::temp_dir().join(format!(
        "wayleaf-config-self-test-{}-{}.json",
        std::process::id(),
        TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        write_atomic(
            &path,
            br#"{"ip":"127.0.0.1","token":"test-token","portal_restore_token":"retained","future":{"enabled":true},"corner":"bottom-right","offset":26,"reverse":true,"fps":30}"#,
        )?;
        let mut config = Config::load(&path)?;
        if config.mode != "4D"
            || config.static_color != "#ffffff"
            || config.fps != MAX_FPS
            || ["corner", "offset", "reverse"]
                .iter()
                .any(|key| config.settings_json().get(key).is_some())
        {
            return Err(AppError::new(
                "self-test config defaults or settings failed",
            ));
        }
        let before = config.settings_json();
        for invalid in [json!({"mode":"5D"}), json!({"static_color":"red"})] {
            if config.apply_settings(&invalid).is_ok() || config.settings_json() != before {
                return Err(AppError::new(
                    "self-test mode or static color validation failed",
                ));
            }
        }
        if config
            .apply_settings(&json!({"monitor":"DP-3","brightness":0.8,"peak_mix":36}))
            .is_ok()
            || config.settings_json() != before
        {
            return Err(AppError::new(
                "self-test settings update was not transactional",
            ));
        }
        config.apply_settings(&json!({
            "monitor": "DP-3",
            "brightness": 0.8,
            "mode": "3D",
            "static_color": "#01A2fF",
        }))?;
        config.save()?;
        let reloaded = Config::load(&path)?;
        if reloaded.settings_json() != config.settings_json()
            || reloaded.host != config.host
            || reloaded.token != config.token
        {
            return Err(AppError::new("self-test config round trip failed"));
        }
        let mode = fs::metadata(&path)
            .map_err(|_| AppError::new("self-test config metadata failed"))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            return Err(AppError::new("self-test config permissions failed"));
        }
        forget_device(&path)?;
        let root =
            read_root(&path)?.ok_or_else(|| AppError::new("self-test config was removed"))?;
        if root.get("ip").is_some()
            || root.get("token").is_some()
            || root.get("portal_restore_token") != Some(&Value::String("retained".into()))
            || root.get("future") != Some(&json!({"enabled": true}))
        {
            return Err(AppError::new("self-test config preservation failed"));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&path);
    if let Ok(lock) = lock_path(&path) {
        let _ = fs::remove_file(lock);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path() -> PathBuf {
        env::temp_dir().join(format!(
            "wayleaf-config-test-{}-{}.json",
            std::process::id(),
            TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn settings_updates_are_atomic_and_credentials_are_not_exported() {
        let mut config = Config {
            host: "192.0.2.10".into(),
            token: "private-token".into(),
            ..Config::default()
        };
        let before = config.settings_json();
        assert!(config
            .apply_settings(&json!({"monitor":"DP-3","brightness":0.8,"peak_mix":36}))
            .is_err());
        assert_eq!(config.settings_json(), before);
        assert!(config
            .apply_settings(&json!({"monitor":"DP-3","future_setting":true}))
            .is_err());
        assert_eq!(config.settings_json(), before);
        assert!(config
            .apply_settings(&json!({"monitor":"DP-3","corner":"top-left"}))
            .is_err());
        assert_eq!(config.settings_json(), before);
        config
            .apply_settings(&json!({"monitor":"DP-3","brightness":0.8}))
            .expect("apply valid settings");
        assert_eq!(config.monitor, "DP-3");
        assert!(config.settings_json().get("token").is_none());
        assert!(config.settings_json().get("ip").is_none());
        assert!(config.settings_json().get("port").is_none());
    }

    #[test]
    fn save_preserves_unknown_values_and_forget_removes_only_credentials() {
        let path = test_path();
        write_atomic(
            &path,
            br#"{"ip":"192.0.2.10","token":"private-token","portal_restore_token":"unrelated","future":{"x":1},"corner":"bottom-right","offset":26,"reverse":true,"fps":40}"#,
        )
        .expect("seed config");
        let mut config = Config::load(&path).expect("load config");
        assert_eq!(config.fps, MAX_FPS);
        config.brightness = 0.7;
        config.save().expect("save config");
        let root = read_root(&path)
            .expect("read saved config")
            .expect("saved config");
        assert!(["corner", "offset", "reverse"]
            .iter()
            .all(|key| root.get(key).is_none()));
        assert_eq!(root["future"], json!({"x": 1}));
        assert_eq!(root["portal_restore_token"], "unrelated");
        assert_eq!(root["port"], json!(config.port));
        assert_eq!(root["token"], "private-token");
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
        forget_device(&path).expect("forget device");
        let root = read_root(&path)
            .expect("read forgotten config")
            .expect("config remains");
        assert!(root.get("ip").is_none() && root.get("token").is_none());
        assert_eq!(root["portal_restore_token"], "unrelated");
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }

    #[test]
    fn concurrent_settings_and_credentials_writers_merge() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let path = test_path();
        write_atomic(
            &path,
            br#"{"ip":"192.0.2.10","token":"old-token","monitor":"HDMI-A-1","brightness":0.6,"future":{"kept":true},"corner":"bottom-right","offset":26,"reverse":true}"#,
        )
        .expect("seed config");
        let settings_writer = Config::load(&path).expect("load settings writer");
        let mut credentials_writer = Config::load(&path).expect("load credentials writer");
        credentials_writer.host = "192.0.2.20".into();
        credentials_writer.token = "new-token".into();
        let barrier = Arc::new(Barrier::new(3));
        let settings_barrier = Arc::clone(&barrier);
        let settings = thread::spawn(move || {
            settings_barrier.wait();
            settings_writer.save_settings(&json!({"monitor":"DP-3"}))
        });
        let credentials_barrier = Arc::clone(&barrier);
        let credentials = thread::spawn(move || {
            credentials_barrier.wait();
            credentials_writer.save_credentials()
        });
        barrier.wait();
        settings
            .join()
            .expect("settings writer thread")
            .expect("save settings");
        credentials
            .join()
            .expect("credentials writer thread")
            .expect("save credentials");

        let root = read_root(&path)
            .expect("read merged config")
            .expect("merged config exists");
        assert_eq!(root["monitor"], "DP-3");
        assert_eq!(root["brightness"], 0.6);
        assert_eq!(root["ip"], "192.0.2.20");
        assert_eq!(root["token"], "new-token");
        assert_eq!(root["future"], json!({"kept": true}));
        assert!(["corner", "offset", "reverse"]
            .iter()
            .all(|key| root.get(key).is_none()));
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }

    #[test]
    fn alignment_settings_validate_and_round_trip() {
        let mut config = Config::default();
        assert_eq!(config.start_corner, "bottom_center");
        assert!(!config.reverse_direction);
        assert_eq!(config.perimeter_offset, 0);
        config
            .apply_settings(&json!({
                "start_corner": "top_left",
                "reverse_direction": true,
                "perimeter_offset": -39
            }))
            .expect("apply alignment settings");
        let before = config.settings_json();
        for invalid in [
            json!({"start_corner":"center"}),
            json!({"perimeter_offset":40}),
            json!({"perimeter_offset":-40}),
        ] {
            assert!(config.apply_settings(&invalid).is_err());
            assert_eq!(config.settings_json(), before);
        }
        let path = test_path();
        config.config_path = path.clone();
        config.save().expect("save alignment settings");
        let loaded = Config::load(&path).expect("reload alignment settings");
        assert_eq!(loaded.settings_json(), config.settings_json());
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }

    #[test]
    fn legacy_alignment_migrates_and_canonical_values_take_precedence() {
        let path = test_path();
        write_atomic(
            &path,
            br#"{"corner":"bottom-right","offset":26,"reverse":true}"#,
        )
        .expect("seed legacy alignment");
        let legacy = Config::load(&path).expect("load legacy alignment");
        assert_eq!(legacy.start_corner, "bottom_right");
        assert_eq!(legacy.perimeter_offset, 26);
        assert!(legacy.reverse_direction);
        legacy
            .save_settings(&json!({"monitor":"DP-3"}))
            .expect("save unrelated setting");
        let migrated = read_root(&path)
            .expect("read migrated alignment")
            .expect("migrated config");
        assert_eq!(migrated["start_corner"], "bottom_right");
        assert_eq!(migrated["perimeter_offset"], 26);
        assert_eq!(migrated["reverse_direction"], true);
        assert!(["corner", "offset", "reverse"]
            .iter()
            .all(|key| migrated.get(key).is_none()));

        write_atomic(
            &path,
            br#"{"corner":"bottom-left","offset":12,"reverse":true,"start_corner":"top_right","perimeter_offset":-4,"reverse_direction":false}"#,
        )
        .expect("seed mixed alignment");
        let canonical = Config::load(&path).expect("load canonical alignment");
        assert_eq!(canonical.start_corner, "top_right");
        assert_eq!(canonical.perimeter_offset, -4);
        assert!(!canonical.reverse_direction);
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }

    #[test]
    fn save_refuses_invalid_files_without_overwriting_or_leaking_credentials() {
        let path = test_path();
        let invalid = b"not-json";
        write_atomic(&path, invalid).expect("seed invalid config");
        let config = Config {
            config_path: path.clone(),
            token: "private-token".into(),
            ..Config::default()
        };
        let error = match config.save() {
            Err(error) => error,
            Ok(()) => panic!("invalid config was overwritten"),
        };
        assert!(!error.message.contains(&config.token));
        assert_eq!(fs::read(&path).expect("read invalid config"), invalid);
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }
    
    #[test]
    fn legacy_alignment_save_migrates_known_keys() {
        let path = test_path();
        write_atomic(
            &path,
            br#"{"corner":"bottom-right","offset":26,"reverse":true}"#,
        )
        .expect("seed legacy alignment");
        let mut config = Config::load(&path).expect("load legacy alignment");
        config.brightness = 0.7;
        config.save().expect("save migrated config");
        let root = read_root(&path)
            .expect("read saved config")
            .expect("saved config");
        assert_eq!(root["start_corner"], "bottom_right");
        assert_eq!(root["perimeter_offset"], 26);
        assert_eq!(root["reverse_direction"], true);
        assert!(["corner", "offset", "reverse"]
            .iter()
            .all(|key| root.get(key).is_none()));
        let _ = fs::remove_file(&path);
        if let Ok(lock) = lock_path(&path) {
            let _ = fs::remove_file(lock);
        }
    }

}
    
