pub mod app_state;
pub mod bridge;
pub mod commands;
pub mod domain;
pub mod providers;
pub mod runtime;
pub mod security;
pub mod storage;

#[cfg(debug_assertions)]
use specta_typescript::Typescript;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

struct LoggingGuard {
    _worker: tracing_appender::non_blocking::WorkerGuard,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta_builder = commands::specta_builder().dangerously_cast_bigints_to_number();

    #[cfg(debug_assertions)]
    specta_builder
        .export(
            Typescript::default(),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts"),
        )
        .expect("failed to export Tauri TypeScript bindings");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app| {
            let app_data_directory = app.path().app_data_dir()?;
            let logging = init_tracing(&app_data_directory)?;
            let state =
                tauri::async_runtime::block_on(app_state::AppState::open(&app_data_directory))?;

            app.manage(logging);
            app.manage(state);
            specta_builder.mount_events(app);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building ImageWorkbench");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            if let Err(error) =
                tauri::async_runtime::block_on(app_handle.state::<app_state::AppState>().shutdown())
            {
                tracing::error!(%error, "failed to mark local work interrupted during shutdown");
            }
        }
    });
}

fn init_tracing(
    app_data_directory: &std::path::Path,
) -> Result<LoggingGuard, Box<dyn std::error::Error>> {
    let log_directory = app_data_directory.join("logs");
    std::fs::create_dir_all(&log_directory)?;
    let appender = tracing_appender::rolling::daily(log_directory, "imageworkbench.log");
    let (writer, worker) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer)
        .try_init();

    Ok(LoggingGuard { _worker: worker })
}
