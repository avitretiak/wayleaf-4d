use std::process::ExitCode;
use wayleaf_4d::{cli, config, control, device, kms, runtime, sampling, AppError, Config, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wayleaf-4d: {error}");
            ExitCode::from(if error.temporary { 75 } else { 1 })
        }
    }
}

fn dispatch() -> Result<()> {
    let args = cli::parse(std::env::args().skip(1))?;
    let mut config = args.config;
    match args.mode {
        cli::Mode::Help => print!("{}", cli::usage()),
        cli::Mode::Version => println!("Wayleaf 4D {VERSION}"),
        cli::Mode::SelfTest => self_test()?,
        cli::Mode::CaptureTest => runtime::capture_test(&config)?,
        cli::Mode::CaptureDebug => {
            require_device(&config, "--capture-debug")?;
            runtime::capture_debug(&config)?;
        }
        cli::Mode::Pair => {
            if config.host.is_empty() {
                return Err(AppError::new("--ip is required to pair"));
            }
            device::pair_device(&mut config)?;
        }
        cli::Mode::DeviceInfo => {
            require_device(&config, "--device-info")?;
            let info = device::device_info(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&info).map_err(|error| {
                    AppError::new(format!("cannot format device information: {error}"))
                })?
            );
        }
        cli::Mode::Control => control::run(config)?,
        cli::Mode::ListMonitors => {
            let monitors = kms::monitors()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&monitors).map_err(|error| {
                    AppError::new(format!("cannot format monitor list: {error}"))
                })?
            );
        }
        cli::Mode::Run => {
            require_device(&config, "streaming")?;
            runtime::run(config)?;
        }
    }
    Ok(())
}

fn require_device(config: &Config, operation: &str) -> Result<()> {
    if config.host.is_empty() || config.token.is_empty() {
        return Err(AppError::new(format!(
            "config or --ip and --token are required for {operation}"
        )));
    }
    Ok(())
}

fn self_test() -> Result<()> {
    config::self_test()?;
    device::self_test()?;
    sampling::self_test()?;
    kms::self_test()?;
    wayleaf_4d::capture::self_test()?;
    control::self_test()?;
    runtime::self_test()?;
    println!("self-test: all module checks passed");
    Ok(())
}
