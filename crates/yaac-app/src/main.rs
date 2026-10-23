//! The yaac app binary. Phase 1 has no core: the engine harness (`--engine-harness`,
//! `--rpc-harness`) drives the engine directly. Without a flag the binary opens a shell window.
//! See README.md for the commands and the phase 2 wiring point.

mod harness;
mod host;

use host::{Mode, NoCore, StubRpc};
use std::process::ExitCode;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use yaac_cef::{EngineSettings, RpcHandler};
use yaac_platform::{MacPlatform, Platform};

fn main() -> ExitCode {
    let mode = match Mode::from_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("yaac-app: {message}");
            return ExitCode::from(2);
        }
    };

    let paths = MacPlatform.paths();
    if let Err(error) = paths.create_all() {
        eprintln!("yaac-app: cannot create app directories: {error}");
        return ExitCode::from(1);
    }
    // Harness runs use a fresh data directory, so caches never carry over between runs.
    let data_dir = match mode {
        Mode::Shell => std::env::var_os("YAAC_DATA_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or(paths.data.clone()),
        _ => std::env::temp_dir().join(format!("yaac-harness-{}", std::process::id())),
    };

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
    let (calls_tx, calls_rx) = unbounded_channel();
    let rpc: Arc<dyn RpcHandler> = match mode {
        Mode::Shell => Arc::new(NoCore),
        Mode::EngineHarness | Mode::RpcHarness => Arc::new(StubRpc { calls: calls_tx }),
    };
    let settings = EngineSettings {
        data_dir,
        log_file: paths.logs.join("cef.log"),
        ui_assets: host::ui_assets(mode),
        rpc,
        rpc_runtime: runtime.handle().clone(),
        events: events_tx,
    };

    let handle = runtime.handle().clone();
    let code = yaac_cef::run_browser(settings, move |engine| {
        host::start(mode, engine, handle, events_rx, calls_rx);
    });
    drop(runtime);
    ExitCode::from(code as u8)
}
