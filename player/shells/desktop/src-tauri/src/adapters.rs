//! Адаптеры портов ядра для десктопа: часы, файлы, сеть, хранилище секретов.
//! Вывод звука — готовый `player-output-cpal`, история — `player-store-sqlite`.

use std::fs::File;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use player_domain::MediaUri;
use player_ports::{
    Clock, FileAccess, FileEntry, HttpRequest, HttpResponse, HttpTransport, MediaStream, PortError,
    SecureStore,
};
use scrobb_client::Method;

/// Пространство имён записей в хранилище секретов ОС.
pub const SECRETS_SERVICE: &str = "ru.scrobb.player";
/// Таймаут запросов к Scrobb: при закрытии плеера «сейчас играет» снимается
/// синхронно, и долгий таймаут задержал бы выход (`docs/player-history.md` §5.3).
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Системные часы.
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn monotonic_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn unix_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }
}

/// Файлы с локального диска: адрес трека — обычный путь.
pub struct LocalFiles;

impl FileAccess for LocalFiles {
    fn list(&self, _: &MediaUri) -> Result<Vec<FileEntry>, PortError> {
        // Сканер папок — следующий шаг (README оболочки).
        Err(PortError::Unsupported)
    }

    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError> {
        File::open(&uri.0)
            .map(|f| Box::new(f) as Box<dyn MediaStream>)
            .map_err(|e| PortError::Other(format!("{}: {e}", uri.0)))
    }
}

/// Сеть через `ureq`: запросы строит `scrobb-client`, здесь они только выполняются.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            // Коды ошибок разбирает `scrobb-client` (401, 426, 429…), а не транспорт.
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }
}

impl HttpTransport for UreqHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, PortError> {
        let fail = |e: ureq::Error| PortError::Other(e.to_string());
        let mut response = match req.method {
            Method::Get => {
                let mut r = self.agent.get(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k, v);
                }
                r.call().map_err(fail)?
            }
            Method::Delete => {
                let mut r = self.agent.delete(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k, v);
                }
                r.call().map_err(fail)?
            }
            Method::Post => {
                let mut r = self.agent.post(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k, v);
                }
                r.send(req.body.as_deref().unwrap_or_default())
                    .map_err(fail)?
            }
        };
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_owned())))
            .collect();
        let body = response.body_mut().read_to_vec().map_err(fail)?;
        Ok(HttpResponse {
            status: response.status().as_u16(),
            headers,
            body,
        })
    }
}

/// Хранилище секретов ОС: Windows Credential Manager, Keychain, Secret Service.
pub struct KeyringSecrets;

impl KeyringSecrets {
    fn entry(key: &str) -> Result<keyring::Entry, PortError> {
        keyring::Entry::new(SECRETS_SERVICE, key).map_err(|e| PortError::Other(e.to_string()))
    }
}

impl SecureStore for KeyringSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, PortError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(PortError::Other(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PortError> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| PortError::Other(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), PortError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(PortError::Other(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_move_forward() {
        let clock = SystemClock::new();
        let a = clock.monotonic_ms();
        std::thread::sleep(Duration::from_millis(5));
        assert!(clock.monotonic_ms() >= a + 5);
        // После 2020 года и до 2100-го.
        assert!((1_577_836_800_000..4_102_444_800_000).contains(&clock.unix_ms()));
    }

    #[test]
    fn local_files_open_and_report_missing() {
        let dir = std::env::temp_dir().join(format!("scrobb-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, b"abc").unwrap();
        let mut stream = LocalFiles
            .open(&MediaUri(path.to_string_lossy().into_owned()))
            .unwrap();
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut stream, &mut buf).unwrap();
        assert_eq!(buf, "abc");
        assert!(
            LocalFiles
                .open(&MediaUri("/nope/nope.flac".into()))
                .is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn http_errors_are_port_errors() {
        // Порт 9 (discard) на localhost закрыт: соединение отвергается сразу.
        let req = HttpRequest {
            method: Method::Get,
            url: "http://127.0.0.1:9/".into(),
            headers: vec![],
            body: None,
        };
        assert!(matches!(
            UreqHttp::new().send(&req),
            Err(PortError::Other(_))
        ));
    }
}
