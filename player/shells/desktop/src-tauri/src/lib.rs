//! Десктоп-оболочка плеера Scrobb: Tauri 3 (системный WebView через
//! `tauri-runtime-wry`) поверх ядра `player/core`. Ядро подключено как обычный
//! крейт, без FFI: окно говорит с ним командами Tauri и событиями (`bridge`).

mod adapters;
mod bridge;
mod covers;
mod library;

use std::path::Path;

use tauri::{Manager, RunEvent};

/// Запустить приложение.
pub fn run() {
    let builder = tauri::Builder::default().runtime(tauri_runtime_wry::Wry::default());
    // Windows: двойной щелчок по плейлисту при открытом плеере отдаёт файл ему,
    // второе окно не открывается. Плагин — первым: второй экземпляр передаёт
    // аргументы и выходит, ничего не успев открыть.
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        if let Some(player) = app.try_state::<bridge::Player>() {
            player.open_paths(launch_paths(args.into_iter().skip(1), Path::new(&cwd)));
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }));
    let app = builder
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
            let cwd = std::env::current_dir().unwrap_or_default();
            player.open_paths(launch_paths(std::env::args().skip(1), &cwd));
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

/// Файлы и папки из командной строки: относительные пути — от каталога
/// запуска, остальное (ключи, опечатки) отбрасывается.
fn launch_paths(args: impl Iterator<Item = String>, cwd: &Path) -> Vec<String> {
    args.map(|arg| cwd.join(arg))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_paths_are_absolute_and_exist() {
        let cwd = std::env::temp_dir().join(format!("scrobb-args-{}", std::process::id()));
        std::fs::create_dir_all(cwd.join("Музыка")).unwrap();
        std::fs::write(cwd.join("Музыка").join("Лучшее.m3u8"), "#EXTM3U\n").unwrap();
        let elsewhere = cwd.join("Музыка").to_string_lossy().into_owned();
        let args = [
            "Музыка/Лучшее.m3u8",
            "--flag",
            "нет.m3u8",
            elsewhere.as_str(),
        ];
        let paths = launch_paths(args.into_iter().map(String::from), &cwd);
        assert_eq!(
            paths,
            [
                cwd.join("Музыка/Лучшее.m3u8")
                    .to_string_lossy()
                    .into_owned(),
                elsewhere.clone()
            ]
        );
        std::fs::remove_dir_all(&cwd).unwrap();
    }
}
