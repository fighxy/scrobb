//! Клиент родного API Scrobb в стиле sans-IO.
//!
//! Крейт не ходит в сеть сам: он строит [`HttpRequest`] и разбирает
//! [`HttpResponse`], а запрос выполняет тот, кто его использует, —
//! адаптер плеера (порт `HttpTransport`), десктоп-клиент на Tauri или
//! `fetch` в браузере через `bindings/wasm`. Так один и тот же код
//! протокола работает на всех платформах и тестируется без сети.

use scrobb_api::{
    ClientKind, DeviceAuthorizeRequest, DeviceAuthorizeResponse, DeviceToken, DeviceTokenError,
    DeviceTokenErrorCode, DeviceTokenRequest, ListenSubmission, MAX_LISTENS_PER_BATCH, NowPlaying,
    ScrobbleConfig, SubmitListensRequest, SubmitListensResponse, paths,
};
use serde::{Serialize, de::DeserializeOwned};

pub mod retry;

/// HTTP-метод.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `DELETE`.
    Delete,
}

/// Запрос, который должен выполнить транспорт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Метод.
    pub method: Method,
    /// Полный адрес.
    pub url: String,
    /// Заголовки.
    pub headers: Vec<(String, String)>,
    /// Тело в JSON.
    pub body: Option<Vec<u8>>,
}

/// Ответ, полученный транспортом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Код ответа.
    pub status: u16,
    /// Заголовки.
    pub headers: Vec<(String, String)>,
    /// Тело.
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Ошибка API, на которую клиент реагирует по-разному (§18.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// `401`: токен отозван — остановить отправку, сохранить очередь, предложить войти заново.
    Unauthorized,
    /// `426`: версия клиента ниже минимальной — предложить обновиться.
    UpgradeRequired,
    /// `429`: подождать `Retry-After` секунд.
    RateLimited {
        /// Значение `Retry-After`, если сервер его прислал.
        retry_after_s: Option<u64>,
    },
    /// `5xx`: повторить с экспоненциальной задержкой ([`retry`]).
    Server(u16),
    /// Другой неожиданный код.
    Unexpected(u16),
    /// Ответ не разобрался.
    Decode(String),
    /// Ошибка на стороне клиента: например, пачка больше лимита.
    Invalid(String),
}

/// Кто обращается к API; попадает в `User-Agent` (§18.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    /// Тип клиента.
    pub kind: ClientKind,
    /// Версия клиента.
    pub version: String,
    /// Платформа: `windows`, `android`, `ios`…
    pub platform: String,
}

impl ClientInfo {
    /// `ScrobbPlayer/0.1.0 (android)`.
    pub fn user_agent(&self) -> String {
        let product = match self.kind {
            ClientKind::Extension => "ScrobbExtension",
            ClientKind::Desktop => "ScrobbDesktop",
            ClientKind::Player => "ScrobbPlayer",
        };
        format!("{product}/{} ({})", self.version, self.platform)
    }
}

/// Клиент родного API.
#[derive(Debug, Clone)]
pub struct ScrobbClient {
    base_url: String,
    info: ClientInfo,
    token: Option<String>,
}

impl ScrobbClient {
    /// `base_url` — например, `https://scrobb.ru`.
    pub fn new(base_url: impl Into<String>, info: ClientInfo) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            info,
            token: None,
        }
    }

    /// Токен устройства `scr_…` после привязки.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// `GET /api/v1/scrobble/config`; `etag` — из прошлого ответа.
    pub fn config(&self, etag: Option<&str>) -> HttpRequest {
        let mut req = self.request(Method::Get, paths::SCROBBLE_CONFIG, None);
        if let Some(etag) = etag {
            req.headers.push(("If-None-Match".into(), etag.into()));
        }
        req
    }

    /// Разбор ответа на [`config`](Self::config): `Ok(None)` — `304`, кэш актуален.
    pub fn parse_config(&self, resp: &HttpResponse) -> Result<Option<ScrobbleConfig>, ApiError> {
        if resp.status == 304 {
            return Ok(None);
        }
        parse(resp).map(Some)
    }

    /// `POST /api/v1/scrobble/now-playing`.
    pub fn now_playing(&self, np: &NowPlaying) -> HttpRequest {
        self.request(Method::Post, paths::NOW_PLAYING, Some(json(np)))
    }

    /// `DELETE /api/v1/scrobble/now-playing`.
    pub fn clear_now_playing(&self) -> HttpRequest {
        self.request(Method::Delete, paths::NOW_PLAYING, None)
    }

    /// `POST /api/v1/scrobble/listens`; не больше [`MAX_LISTENS_PER_BATCH`] за раз.
    pub fn submit_listens(&self, listens: &[ListenSubmission]) -> Result<HttpRequest, ApiError> {
        if listens.is_empty() || listens.len() > MAX_LISTENS_PER_BATCH {
            return Err(ApiError::Invalid(format!(
                "в пачке {} прослушиваний",
                listens.len()
            )));
        }
        let body = SubmitListensRequest {
            listens: listens.to_vec(),
        };
        Ok(self.request(Method::Post, paths::LISTENS, Some(json(&body))))
    }

    /// Разбор ответа на [`submit_listens`](Self::submit_listens).
    pub fn parse_submit_listens(
        &self,
        resp: &HttpResponse,
    ) -> Result<SubmitListensResponse, ApiError> {
        parse(resp)
    }

    /// `POST /api/v1/devices/authorize` — начало привязки устройства (§18.4).
    pub fn device_authorize(&self, device_name: &str) -> HttpRequest {
        let body = DeviceAuthorizeRequest {
            client_kind: self.info.kind,
            platform: self.info.platform.clone(),
            client_version: self.info.version.clone(),
            device_name: device_name.into(),
        };
        self.request(Method::Post, paths::DEVICE_AUTHORIZE, Some(json(&body)))
    }

    /// Разбор ответа на [`device_authorize`](Self::device_authorize).
    pub fn parse_device_authorize(
        &self,
        resp: &HttpResponse,
    ) -> Result<DeviceAuthorizeResponse, ApiError> {
        parse(resp)
    }

    /// `POST /api/v1/devices/token` — опрос не чаще `interval` секунд.
    pub fn device_token(&self, device_code: &str) -> HttpRequest {
        let body = DeviceTokenRequest {
            device_code: device_code.into(),
        };
        self.request(Method::Post, paths::DEVICE_TOKEN, Some(json(&body)))
    }

    /// Разбор ответа на [`device_token`](Self::device_token).
    pub fn parse_device_token(&self, resp: &HttpResponse) -> Result<DevicePoll, ApiError> {
        if resp.status == 400 {
            let err: DeviceTokenError = decode(&resp.body)?;
            return Ok(match err.error {
                DeviceTokenErrorCode::AuthorizationPending => DevicePoll::Pending,
                DeviceTokenErrorCode::SlowDown => DevicePoll::SlowDown,
                DeviceTokenErrorCode::AccessDenied => DevicePoll::Denied,
                DeviceTokenErrorCode::ExpiredToken => DevicePoll::Expired,
            });
        }
        parse(resp).map(DevicePoll::Linked)
    }

    /// `DELETE /api/v1/devices/current` — выход и отзыв токена.
    pub fn sign_out(&self) -> HttpRequest {
        self.request(Method::Delete, paths::DEVICE_CURRENT, None)
    }

    fn request(&self, method: Method, path: &str, body: Option<Vec<u8>>) -> HttpRequest {
        let mut headers = vec![("User-Agent".to_owned(), self.info.user_agent())];
        if let Some(token) = &self.token {
            headers.push(("Authorization".into(), format!("Bearer {token}")));
        }
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        HttpRequest {
            method,
            url: format!("{}{path}", self.base_url),
            headers,
            body,
        }
    }
}

/// Итог опроса токена устройства (RFC 8628, §3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    /// Пользователь ещё не подтвердил — опросить снова через `interval`.
    Pending,
    /// Увеличить интервал опроса на 5 секунд.
    SlowDown,
    /// Пользователь отказал.
    Denied,
    /// Код истёк — начать привязку заново.
    Expired,
    /// Устройство привязано.
    Linked(DeviceToken),
}

fn json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("DTO сериализуются без ошибок")
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::Decode(e.to_string()))
}

fn parse<T: DeserializeOwned>(resp: &HttpResponse) -> Result<T, ApiError> {
    match resp.status {
        200..=299 => decode(&resp.body),
        401 => Err(ApiError::Unauthorized),
        426 => Err(ApiError::UpgradeRequired),
        429 => Err(ApiError::RateLimited {
            retry_after_s: resp
                .header("Retry-After")
                .and_then(|v| v.trim().parse().ok()),
        }),
        500..=599 => Err(ApiError::Server(resp.status)),
        other => Err(ApiError::Unexpected(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrobb_api::{ListenStatus, Source};

    fn client() -> ScrobbClient {
        let info = ClientInfo {
            kind: ClientKind::Player,
            version: "0.1.0".into(),
            platform: "android".into(),
        };
        ScrobbClient::new("https://scrobb.ru/", info).with_token("scr_test")
    }

    fn listen(id: &str) -> ListenSubmission {
        ListenSubmission {
            client_id: id.into(),
            listened_at: 1_760_000_000,
            track: "Звезда по имени Солнце".into(),
            artist: "Кино".into(),
            album: None,
            album_artist: None,
            duration_ms: Some(225_000),
            played_ms: 112_500,
            mbids: None,
            source: Source {
                kind: ClientKind::Player,
                connector: "player".into(),
            },
        }
    }

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("retry-after".into(), "30".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn submit_builds_authorized_request() {
        let req = client().submit_listens(&[listen("a")]).unwrap();
        assert_eq!(req.url, "https://scrobb.ru/api/v1/scrobble/listens");
        assert!(
            req.headers
                .contains(&("Authorization".into(), "Bearer scr_test".into()))
        );
        assert!(
            req.headers
                .contains(&("User-Agent".into(), "ScrobbPlayer/0.1.0 (android)".into()))
        );
    }

    #[test]
    fn submit_rejects_oversized_batch() {
        let batch: Vec<_> = (0..=MAX_LISTENS_PER_BATCH)
            .map(|i| listen(&i.to_string()))
            .collect();
        assert!(matches!(
            client().submit_listens(&batch),
            Err(ApiError::Invalid(_))
        ));
    }

    #[test]
    fn parses_per_listen_results_and_errors() {
        let ok = response(
            200,
            r#"{"results":[{"client_id":"a","status":"duplicate"}]}"#,
        );
        assert_eq!(
            client().parse_submit_listens(&ok).unwrap().results[0].status,
            ListenStatus::Duplicate
        );
        let limited = response(429, "");
        assert_eq!(
            client().parse_submit_listens(&limited),
            Err(ApiError::RateLimited {
                retry_after_s: Some(30)
            })
        );
    }

    #[test]
    fn device_poll_maps_rfc8628_errors() {
        let pending = response(400, r#"{"error":"authorization_pending"}"#);
        assert_eq!(
            client().parse_device_token(&pending),
            Ok(DevicePoll::Pending)
        );
        let linked = response(
            200,
            r#"{"access_token":"scr_x","token_id":7,"name":"Android"}"#,
        );
        assert!(
            matches!(client().parse_device_token(&linked), Ok(DevicePoll::Linked(t)) if t.token_id == 7)
        );
    }
}
