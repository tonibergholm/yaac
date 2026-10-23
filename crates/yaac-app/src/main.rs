//! The yaac app binary. Without a flag it is the app: the real core, store and engine. The engine
//! harness and the persistence checks exist only in debug builds and `dev-tools` builds (see
//! README.md).

#[cfg(yaac_dev)]
mod harness;
mod host;
#[cfg(yaac_dev)]
mod selftest;
#[cfg(yaac_dev)]
mod site;

use host::{CefMainThread, CoreRpc};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc::unbounded_channel;
use yaac_cef::{EngineSettings, RpcHandler};
#[cfg(target_os = "linux")]
use yaac_platform::LinuxPlatform;
#[cfg(target_os = "macos")]
use yaac_platform::MacPlatform;
use yaac_platform::{AppPaths, Platform, ensure_private_dir};

/// Linux runs its CEF helper processes (zygote, renderer, GPU, utility) as this executable with
/// `--type=...`. They must be handled before anything else here, see `yaac_cef::run_helper`. The
/// macOS helper is a separate bundled binary, so there is nothing to do here.
#[cfg(target_os = "linux")]
fn helper_exit() -> Option<ExitCode> {
    if std::env::args().any(|arg| arg.starts_with("--type=")) {
        return Some(ExitCode::from(yaac_cef::run_helper() as u8));
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn helper_exit() -> Option<ExitCode> {
    None
}

fn host_paths() -> AppPaths {
    #[cfg(target_os = "macos")]
    let paths = MacPlatform.paths();
    #[cfg(target_os = "linux")]
    let paths = LinuxPlatform.paths();
    paths
}

/// What this run does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No flag: the app.
    Shell,
    /// `--engine-harness`: every engine stage, with a stub core.
    #[cfg(yaac_dev)]
    EngineHarness,
    /// `--rpc-harness`: the RPC stage only.
    #[cfg(yaac_dev)]
    RpcHarness,
    /// `--quit-harness`: requests quit while a view is still being created.
    #[cfg(yaac_dev)]
    QuitHarness,
    /// `--tab-harness`: closing tabs must not close the window.
    #[cfg(yaac_dev)]
    TabHarness,
    /// `--persist-write`: real core and store; opens tabs through the RPC path, then quits.
    #[cfg(yaac_dev)]
    PersistWrite,
    /// `--persist-verify`: real core and store on the same data directory; checks the tabs came back.
    #[cfg(yaac_dev)]
    PersistVerify,
}

impl Mode {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut mode = Mode::Shell;
        for arg in args {
            mode = match arg.as_str() {
                #[cfg(yaac_dev)]
                "--engine-harness" => Mode::EngineHarness,
                #[cfg(yaac_dev)]
                "--rpc-harness" => Mode::RpcHarness,
                #[cfg(yaac_dev)]
                "--quit-harness" => Mode::QuitHarness,
                #[cfg(yaac_dev)]
                "--tab-harness" => Mode::TabHarness,
                #[cfg(yaac_dev)]
                "--persist-write" => Mode::PersistWrite,
                #[cfg(yaac_dev)]
                "--persist-verify" => Mode::PersistVerify,
                // Development runs only: CEF reads this from the process arguments, which picks the
                // Ozone platform (for example `headless`, which needs no compositor).
                other if cfg!(yaac_dev) && other.starts_with("--ozone-platform=") => mode,
                other => return Err(format!("unknown argument {other}")),
            };
        }
        Ok(mode)
    }

    #[cfg(yaac_dev)]
    fn stages(self) -> Option<harness::Stages> {
        match self {
            Mode::EngineHarness => Some(harness::Stages::All),
            Mode::RpcHarness => Some(harness::Stages::RpcOnly),
            Mode::QuitHarness => Some(harness::Stages::QuitDuringCreate),
            Mode::TabHarness => Some(harness::Stages::TabClose),
            _ => None,
        }
    }
}

/// Where this run keeps its data. Harness runs use a fresh directory. The persistence checks need
/// `YAAC_DATA_DIR`, so they never touch a real user's database, and `--persist-write` needs an
/// empty directory.
fn data_dir_for(mode: Mode, paths: &AppPaths) -> Result<PathBuf, String> {
    #[cfg(yaac_dev)]
    {
        if mode.stages().is_some() {
            return Ok(std::env::temp_dir().join(format!("yaac-harness-{}", std::process::id())));
        }
        if matches!(mode, Mode::PersistWrite | Mode::PersistVerify) {
            let dir = std::env::var_os("YAAC_DATA_DIR")
                .map(PathBuf::from)
                .ok_or("the persistence checks need YAAC_DATA_DIR set to a scratch directory")?;
            if mode == Mode::PersistWrite && !is_empty_or_missing(&dir)? {
                return Err(format!(
                    "--persist-write needs an empty YAAC_DATA_DIR; {} is not empty",
                    dir.display()
                ));
            }
            return Ok(dir);
        }
    }
    let _ = mode;
    Ok(std::env::var_os("YAAC_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.data.clone()))
}

#[cfg(yaac_dev)]
fn is_empty_or_missing(dir: &Path) -> Result<bool, String> {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(format!("cannot read {}: {error}", dir.display())),
    }
}

fn main() -> ExitCode {
    if let Some(code) = helper_exit() {
        return code;
    }
    let mode = match Mode::from_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("yaac-app: {message}");
            return ExitCode::from(2);
        }
    };

    let paths = host_paths();
    let data_dir = match data_dir_for(mode, &paths) {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("yaac-app: {message}");
            return ExitCode::from(2);
        }
    };
    // The data directory is validated: a symlink or a directory of another user is refused, and a
    // looser mode is tightened. The logs and the runtime directory always exist.
    if let Err(error) = ensure_private_dir(&data_dir).and_then(|()| ensure_private_dir(&paths.logs))
    {
        eprintln!("yaac-app: {}: {error}", data_dir.display());
        return ExitCode::from(1);
    }
    if mode == Mode::Shell
        && let Err(error) = paths.create_all()
    {
        eprintln!("yaac-app: cannot create app directories: {error}");
        return ExitCode::from(1);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("yaac-app: cannot start runtime: {error}");
            return ExitCode::from(1);
        }
    };

    let (events_tx, events_rx) = unbounded_channel();
    let core_cell = Arc::new(OnceLock::new());
    #[cfg(yaac_dev)]
    let (calls_tx, calls_rx) = unbounded_channel();
    #[cfg(yaac_dev)]
    let rpc: Arc<dyn RpcHandler> = if mode.stages().is_some() {
        Arc::new(harness::StubRpc { calls: calls_tx })
    } else {
        Arc::new(CoreRpc {
            core: core_cell.clone(),
        })
    };
    #[cfg(not(yaac_dev))]
    let rpc: Arc<dyn RpcHandler> = Arc::new(CoreRpc {
        core: core_cell.clone(),
    });

    let settings = EngineSettings {
        data_dir: data_dir.clone(),
        log_file: paths.logs.join("cef.log"),
        ui_assets: host::ui_assets(mode),
        rpc,
        rpc_runtime: runtime.handle().clone(),
        events: events_tx,
    };

    let handle = runtime.handle().clone();
    let start_cell = core_cell.clone();
    let shutdown_cell = core_cell;
    let code = yaac_cef::run_browser(
        settings,
        move |engine| {
            host::start(
                mode,
                engine,
                handle,
                events_rx,
                #[cfg(yaac_dev)]
                calls_rx,
                data_dir,
                start_cell,
            );
        },
        move |engine| host::shutdown(engine, shutdown_cell, runtime),
    );
    let _ = CefMainThread;
    ExitCode::from(code as u8)
}
