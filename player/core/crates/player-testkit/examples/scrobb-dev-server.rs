//! Сервер Scrobb для разработки: эталонный [`ScrobbServer`] по настоящему HTTP.
//! Привязка устройства подтверждается сразу, скробблы принимаются,
//! плейлисты синхронизируются по `docs/playlists.md` §7; всё — в памяти,
//! до остановки сервера.
//!
//! ```sh
//! cd player/core
//! cargo run -p player-testkit --example scrobb-dev-server -- --port 8787
//! # в другом терминале — плеер, говорящий с ним:
//! SCROBB_BASE_URL=http://127.0.0.1:8787 pnpm tauri dev
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use player_testkit::server::ScrobbServer;
use scrobb_client::Method;

/// Самое большое тело запроса: пачка синхронизации — несколько сотен килобайт.
const MAX_BODY: usize = 16 * 1024 * 1024;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut port: u16 = 8787;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .expect("--port <число>");
            }
            other => panic!("неизвестный аргумент {other}; есть только --port"),
        }
    }
    let base = format!("http://127.0.0.1:{port}");
    let server = ScrobbServer::new(&base);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("порт свободен");
    println!("Scrobb dev-сервер: {base} (привязка подтверждается сразу)");
    for stream in listener.incoming().flatten() {
        let server = Arc::clone(&server);
        thread::spawn(move || {
            if let Err(e) = serve(&server, stream) {
                eprintln!("соединение: {e}");
            }
        });
    }
}

/// Один запрос на соединение (`Connection: close`): клиентам плеера этого хватает.
fn serve(server: &ScrobbServer, stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or("/").to_owned(),
    );
    let mut length = 0;
    let mut authorization = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim().to_owned();
            if name.eq_ignore_ascii_case("content-length") {
                length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            }
        }
    }
    let mut body = vec![0; length.min(MAX_BODY)];
    reader.read_exact(&mut body)?;

    let (status, kind, answer) = if path.starts_with("/link") {
        // Страница, которую плеер открывает в браузере при привязке.
        let page = "<!doctype html><meta charset=utf-8><title>Scrobb</title>\
                    <p>Устройство привязано (dev-сервер подтверждает сам). Можно закрыть вкладку.</p>";
        (200, "text/html; charset=utf-8", page.as_bytes().to_vec())
    } else {
        let method = match method.as_str() {
            "GET" => Some(Method::Get),
            "POST" => Some(Method::Post),
            "DELETE" => Some(Method::Delete),
            _ => None,
        };
        let (status, answer) = match method {
            Some(m) => server.handle(m, &path, authorization.as_deref(), &body),
            None => (405, Vec::new()),
        };
        (status, "application/json", answer)
    };
    println!("{method} {path} → {status}");
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Status",
    };
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.len()
    )?;
    out.write_all(&answer)?;
    out.flush()
}
