use crate::config::Config;
use crate::{AppError, Result};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Run,
    Help,
    Version,
    SelfTest,
    CaptureTest,
    CaptureDebug,
    Pair,
    DeviceInfo,
    Control,
    ListMonitors,
}

pub struct Args {
    pub config: Config,
    pub mode: Mode,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let args: Vec<String> = args.into_iter().collect();
    if let Some(mode) = early_mode(&args) {
        return Ok(Args {
            config: Config::default(),
            mode,
        });
    }

    let config_path = find_config_path(&args).unwrap_or_else(|| Config::default().config_path);
    let mut config = Config::load(&config_path)?;
    let mut pair = false;
    let mut device_info = false;
    let mut capture_test = false;
    let mut capture_debug = false;
    let mut control = false;
    let mut list_monitors = false;
    let mut i = 0;

    while i < args.len() {
        let option = args[i].as_str();
        match option {
            "--help" | "-h" | "--version" | "--self-test" => {
                unreachable!("early modes were handled")
            }
            "--pair" => pair = true,
            "--device-info" => device_info = true,
            "--capture-test" => capture_test = true,
            "--capture-debug" => capture_debug = true,
            "--control" => control = true,
            "--list-monitors" => list_monitors = true,
            "--no-letterbox" => config.letterbox = false,
            "--letterbox" => config.letterbox = true,
            "--midtone-lift" => config.midtone_lift = true,
            "--no-midtone-lift" => config.midtone_lift = false,
            "--strict-black" => config.strict_black = true,
            "--no-strict-black" => config.strict_black = false,
            "--config" | "--ip" | "--token" | "--monitor" | "--port" | "--fps" | "--max-step"
            | "--edge-depth" | "--center-bleed" | "--peak-mix" | "--feather-depth"
            | "--brightness" | "--smooth" | "--black-threshold" | "--saturation-boost" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| AppError::new(format!("missing value for {option}")))?;
                match option {
                    "--config" => {}
                    "--ip" => config.host.clone_from(value),
                    "--token" => config.token.clone_from(value),
                    "--monitor" => config.monitor.clone_from(value),
                    "--port" => config.port = parse_i32(value, option)?,
                    "--fps" => config.fps = parse_i32(value, option)?,
                    "--max-step" => config.max_step = parse_i32(value, option)?,
                    "--edge-depth" => config.edge_depth = parse_i32(value, option)?,
                    "--center-bleed" => config.center_bleed = parse_i32(value, option)?,
                    "--peak-mix" => config.peak_mix = parse_i32(value, option)?,
                    "--feather-depth" => config.feather_depth = parse_i32(value, option)?,
                    "--brightness" => config.brightness = parse_f32(value, option)?,
                    "--smooth" => config.smooth = parse_f32(value, option)?,
                    "--black-threshold" => config.black_threshold = parse_f32(value, option)?,
                    "--saturation-boost" => config.saturation_boost = parse_f32(value, option)?,
                    _ => unreachable!("value-taking option was matched"),
                }
                i += 1;
            }
            _ => return Err(AppError::new("unknown command-line option")),
        }
        i += 1;
    }

    config.validate()?;
    let mode = if capture_test {
        Mode::CaptureTest
    } else if capture_debug {
        Mode::CaptureDebug
    } else if device_info {
        Mode::DeviceInfo
    } else if pair {
        Mode::Pair
    } else if list_monitors {
        Mode::ListMonitors
    } else if control {
        Mode::Control
    } else {
        Mode::Run
    };
    Ok(Args { config, mode })
}

fn early_mode(args: &[String]) -> Option<Mode> {
    let mut help = false;
    let mut self_test = false;
    let mut version = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => help = true,
            "--version" => version = true,
            "--self-test" => self_test = true,
            option if takes_value(option) => i += 1,
            _ => {}
        }
        i += 1;
    }
    if version {
        Some(Mode::Version)
    } else if self_test {
        Some(Mode::SelfTest)
    } else if help {
        Some(Mode::Help)
    } else {
        None
    }
}

fn find_config_path(args: &[String]) -> Option<PathBuf> {
    let mut found = None;
    let mut i = 0;
    while i < args.len() {
        let option = args[i].as_str();
        if option == "--config" {
            if let Some(value) = args.get(i + 1) {
                found = Some(PathBuf::from(value));
                i += 1;
            }
        } else if takes_value(option) {
            i += 1;
        }
        i += 1;
    }
    found
}

fn takes_value(option: &str) -> bool {
    matches!(
        option,
        "--config"
            | "--ip"
            | "--token"
            | "--monitor"
            | "--port"
            | "--fps"
            | "--max-step"
            | "--edge-depth"
            | "--center-bleed"
            | "--peak-mix"
            | "--feather-depth"
            | "--brightness"
            | "--smooth"
            | "--black-threshold"
            | "--saturation-boost"
    )
}

fn parse_i32(value: &str, option: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .map_err(|_| AppError::new(format!("invalid value for {option}")))
}

fn parse_f32(value: &str, option: &str) -> Result<f32> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| AppError::new(format!("invalid value for {option}")))?;
    if !parsed.is_finite() {
        return Err(AppError::new(format!("invalid value for {option}")));
    }
    Ok(parsed)
}

pub fn usage() -> &'static str {
    "Usage: wayleaf-4d [--pair --ip HOST] | [--ip HOST --token TOKEN] [options]\n\
     --config PATH             config file (default: XDG config dir/nanoleaf-sync/config.json)\n\
     --pair                    pair now and securely save the device token\n\
     --fps 1..10 --port 1..65535 (UDP fallback, default 60222)\n\
     --brightness 0..2 --saturation-boost 0..3 --smooth 0.05..1\n\
     --black-threshold 0..0.1 --max-step 1..255 --edge-depth 5..20%\n\
     --center-bleed 0..50 --peak-mix 0..35 --feather-depth 0..20\n\
     --no-letterbox --letterbox --midtone-lift --no-midtone-lift\n\
     --strict-black --no-strict-black --monitor NAME\n\
     --device-info --self-test --capture-test --capture-debug\n\
     --control --list-monitors\n\
     --version                 print Wayleaf 4D version"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "wayleaf-cli-test-{}-{nonce}.json",
            std::process::id()
        ))
    }

    #[test]
    fn early_modes_skip_invalid_config_and_options_cover_live_controls() {
        let path = unique_path();
        fs::write(&path, "invalid").expect("invalid test config");
        let args = parse([
            "--config".into(),
            path.display().to_string(),
            "--help".into(),
        ])
        .expect("help does not load config");
        assert!(matches!(args.mode, Mode::Help));
        let _ = fs::remove_file(path);

        let args = parse([
            "--config".into(),
            unique_path().display().to_string(),
            "--monitor".into(),
            "DP-3".into(),
            "--center-bleed".into(),
            "20".into(),
            "--peak-mix".into(),
            "30".into(),
            "--feather-depth".into(),
            "10".into(),
            "--control".into(),
        ])
        .expect("parse controls");
        assert!(matches!(args.mode, Mode::Control));
        assert_eq!(args.config.monitor, "DP-3");
        assert_eq!(args.config.center_bleed, 20);
        assert_eq!(args.config.peak_mix, 30);
        assert_eq!(args.config.feather_depth, 10);
        let args = parse([
            "--config".into(),
            unique_path().display().to_string(),
            "--corner".into(),
            "top-left".into(),
        ]);
        assert!(args.is_err());
    }

    #[test]
    fn token_values_never_appear_in_cli_errors() {
        let secret = "do-not-print-this-token";
        let result = parse([
            "--config".into(),
            unique_path().display().to_string(),
            "--token".into(),
            secret.into(),
            "--fps".into(),
            "not-a-number".into(),
        ]);
        let message = result.err().expect("invalid fps").message;
        assert!(!message.contains(secret));
    }
}
