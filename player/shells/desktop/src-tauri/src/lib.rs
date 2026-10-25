//! Десктоп-оболочка плеера Scrobb: Tauri 3 (системный WebView через
//! `tauri-runtime-wry`) поверх ядра `player/core`. Ядро подключено как обычный
//! крейт, без FFI: окно говорит с ним командами Tauri и событиями (`bridge`).

mod adapters;
mod bridge;
mod covers;
mod library;

use tauri::{Manager, RunEvent};

/// Запустить приложение.
pub fn run() {
    let app = tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Обложки готовятся в пуле `covers`, поток WebView не ждёт.
        .register_asynchronous_uri_scheme_protocol(covers::SCHEME, |ctx, request, responder| {
            let Some(player) = ctx.app_handle().try_state::<bridge::Player>() else {
                let unavailable = tauri::http::Response::builder()
                    .status(tauri::http::StatusCode::SERVICE_UNAVAILABLE)
                    .body(Vec::new())
                    .expect("пустой ответ собирается");
                return responder.respond(unavailable);
            };
            let covers = player.covers();
            let path = request.uri().path().to_owned();
            covers
                .clone()
                .spawn(move || responder.respond(covers.respond(&path)));
        })
        .setup(|app| {
            let player = bridge::Player::start(app.handle())?;
            let paths: Vec<String> = std::env::args()
                .skip(1)
                .filter(|a| std::path::Path::new(a).exists())
                .collect();
            player.open_paths(paths);
            app.manage(player);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::dispatch,
            bridge::snapshot,
            bridge::add_files,
            bridge::add_folder,
            bridge::remove_folder,
            bridge::rescan_library,
            bridge::recent_plays,
            bridge::queue,
            bridge::cover_color,
            bridge::playlists,
            bridge::playlist,
            bridge::create_playlist,
            bridge::update_playlist,
            bridge::delete_playlist,
            bridge::add_to_playlist,
            bridge::remove_from_playlist,
            bridge::move_in_playlist,
            bridge::import_playlist,
            bridge::export_playlist,
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
