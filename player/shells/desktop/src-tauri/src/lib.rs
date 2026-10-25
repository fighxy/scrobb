//! Десктоп-оболочка плеера Scrobb: Tauri 3 (системный WebView через
//! `tauri-runtime-wry`) поверх ядра `player/core`. Ядро подключено как обычный
//! крейт, без FFI: окно говорит с ним командами Tauri и событиями (`bridge`).

mod adapters;
mod bridge;

use tauri::{Manager, RunEvent};

/// Запустить приложение.
pub fn run() {
    let app = tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let player = bridge::Player::start(app.handle())?;
            let files: Vec<String> = std::env::args()
                .skip(1)
                .filter(|a| std::path::Path::new(a).is_file())
                .collect();
            player.open_files(files);
            app.manage(player);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::dispatch,
            bridge::snapshot,
            bridge::add_files,
            bridge::recent_plays,
        ])
        .build(tauri::generate_context!())
        .expect("приложение собирается");
    app.run(|app, event| {
        if let RunEvent::Exit = event
            && let Some(player) = app.try_state::<bridge::Player>()
        {
            player.shutdown();
        }
    });
}
