//! DTO родного API Scrobb (`docs/architecture.md` §18.3, §18.4).
//!
//! Нижний крейт яруса Scrobb: только типы и сериализация, без ввода-вывода.
//! Собирается под `wasm32-unknown-unknown`, поэтому те же типы могут
//! использовать веб-приложение и расширение через `bindings/wasm`.
//!
//! Источник истины — OpenAPI бэкенда; пока его нет, типы написаны вручную
//! по документу. Позже их заменит генерация (`contracts/README.md`).

use serde::{Deserialize, Serialize};

/// Пути родного API относительно базового адреса (`https://scrobb.ru`).
pub mod paths {
    /// Серверная конфигурация клиента.
    pub const SCROBBLE_CONFIG: &str = "/api/v1/scrobble/config";
    /// «Сейчас играет»: `POST` — установить, `DELETE` — сбросить.
    pub const NOW_PLAYING: &str = "/api/v1/scrobble/now-playing";
    /// Пачка прослушиваний.
    pub const LISTENS: &str = "/api/v1/scrobble/listens";
    /// Краткая сводка для меню клиента.
    pub const ME_SUMMARY: &str = "/api/v1/me/summary";
    /// Начало привязки устройства (RFC 8628).
    pub const DEVICE_AUTHORIZE: &str = "/api/v1/devices/authorize";
    /// Опрос токена устройства.
    pub const DEVICE_TOKEN: &str = "/api/v1/devices/token";
    /// Выход: отзыв токена текущего устройства.
    pub const DEVICE_CURRENT: &str = "/api/v1/devices/current";
    /// Синхронизация плейлистов: отправить изменённое, получить изменённое
    /// (`docs/playlists.md` §7; тело и ответ — `scrobb_sync::playlists`).
    pub const SYNC_PLAYLISTS: &str = "/api/v1/sync/playlists";
}

/// Максимум прослушиваний в одном запросе (§18.3).
pub const MAX_LISTENS_PER_BATCH: usize = 50;

/// Тип клиента. `Player` — новое значение: бэкенд пока знает только
/// `extension` и `desktop` (§18.3), его нужно добавить на сервере.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// Расширение браузера.
    Extension,
    /// Десктоп-клиент по медиасессиям.
    Desktop,
    /// Свой плеер.
    Player,
}

/// Откуда пришло прослушивание; нужно для диагностики и дедупликации (§5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Тип клиента.
    pub kind: ClientKind,
    /// Коннектор: `yandex-music`, `smtc:<AppUserModelId>`, `mpris:<плеер>`, `player`.
    pub connector: String,
}

/// Пороги правила скроббла (§18.7). Приходят в [`ScrobbleConfig`];
/// значения по умолчанию клиент хранит у себя.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thresholds {
    /// Трек должен быть строго длиннее этого значения.
    pub min_track_ms: u64,
    /// Какую долю длительности нужно прослушать, в процентах.
    pub percent: u8,
    /// Сколько звучания достаточно в любом случае и при неизвестной длительности.
    pub max_wait_ms: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            min_track_ms: 30_000,
            percent: 50,
            max_wait_ms: 240_000,
        }
    }
}

/// Ответ `GET /api/v1/scrobble/config`. Названия полей — черновик до OpenAPI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrobbleConfig {
    /// Пороги правила скроббла.
    pub thresholds: Thresholds,
    /// Размер пачки прослушиваний.
    pub batch_size: u16,
    /// Минимальная поддерживаемая версия клиента этого типа.
    pub min_client_version: Option<String>,
    /// Отключённые коннекторы (kill switch).
    #[serde(default)]
    pub disabled_connectors: Vec<String>,
}

/// MusicBrainz-идентификаторы, если клиент их знает (например, из тегов файла).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mbids {
    /// Запись (recording).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<String>,
    /// Релиз (release).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    /// Артисты.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<String>,
}

/// Одно прослушивание в пачке `POST /api/v1/scrobble/listens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenSubmission {
    /// UUID, присвоенный клиентом при постановке в очередь; нужен для поштучного ответа.
    pub client_id: String,
    /// Начало прослушивания, секунды Unix UTC (как `listened_at` в ListenBrainz).
    pub listened_at: i64,
    /// Название трека.
    pub track: String,
    /// Артист.
    pub artist: String,
    /// Альбом.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Артист альбома.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// Длительность трека.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Сколько трек реально звучал; сервер повторно проверяет правило скроббла.
    pub played_ms: u64,
    /// MusicBrainz-идентификаторы.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbids: Option<Mbids>,
    /// Источник.
    pub source: Source,
}

/// Тело запроса `POST /api/v1/scrobble/listens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitListensRequest {
    /// До [`MAX_LISTENS_PER_BATCH`] прослушиваний.
    pub listens: Vec<ListenSubmission>,
}

/// Итог по одному прослушиванию.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenStatus {
    /// Принято.
    Accepted,
    /// Уже было (повтор пачки после таймаута).
    Duplicate,
    /// Отклонено; причина — в `reason`.
    Rejected,
}

/// Поштучный ответ на прослушивание.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenResult {
    /// `client_id` из запроса.
    pub client_id: String,
    /// Итог.
    pub status: ListenStatus,
    /// Причина отказа, например `below_threshold`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Ответ `POST /api/v1/scrobble/listens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitListensResponse {
    /// По одному результату на каждое прослушивание из запроса.
    pub results: Vec<ListenResult>,
}

/// Тело `POST /api/v1/scrobble/now-playing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowPlaying {
    /// Название трека.
    pub track: String,
    /// Артист.
    pub artist: String,
    /// Альбом.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Длительность.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Текущая позиция.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_ms: Option<u64>,
    /// Источник.
    pub source: Source,
}

/// Тело `POST /api/v1/devices/authorize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorizeRequest {
    /// Тип клиента.
    pub client_kind: ClientKind,
    /// Платформа: `windows`, `android`, `ios`…
    pub platform: String,
    /// Версия клиента.
    pub client_version: String,
    /// Имя устройства, которое увидит пользователь.
    pub device_name: String,
}

/// Ответ `POST /api/v1/devices/authorize` (RFC 8628, §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorizeResponse {
    /// Секрет для опроса токена; хранить только до конца привязки.
    pub device_code: String,
    /// Код, который пользователь сверяет на сайте.
    pub user_code: String,
    /// Страница привязки.
    pub verification_uri: String,
    /// Страница привязки с подставленным кодом.
    pub verification_uri_complete: String,
    /// Сколько секунд действует `device_code`.
    pub expires_in: u32,
    /// Минимальный интервал опроса, секунды.
    pub interval: u32,
}

/// Тело `POST /api/v1/devices/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceTokenRequest {
    /// Код из [`DeviceAuthorizeResponse`].
    pub device_code: String,
}

/// Успешный ответ `POST /api/v1/devices/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceToken {
    /// Токен устройства `scr_…`; хранится только в хранилище секретов ОС.
    pub access_token: String,
    /// Идентификатор токена на сервере.
    pub token_id: i64,
    /// Имя устройства в «Приложениях».
    pub name: String,
}

/// Код ошибки опроса по RFC 8628, §3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceTokenErrorCode {
    /// Пользователь ещё не подтвердил.
    AuthorizationPending,
    /// Опрашивать реже.
    SlowDown,
    /// Пользователь отказал.
    AccessDenied,
    /// Код истёк.
    ExpiredToken,
}

/// Тело ошибки `POST /api/v1/devices/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceTokenError {
    /// Код ошибки.
    pub error: DeviceTokenErrorCode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_omits_empty_optional_fields() {
        let listen = ListenSubmission {
            client_id: "c1".into(),
            listened_at: 1_760_000_000,
            track: "Группа крови".into(),
            artist: "Кино".into(),
            album: None,
            album_artist: None,
            duration_ms: Some(285_000),
            played_ms: 142_500,
            mbids: None,
            source: Source {
                kind: ClientKind::Player,
                connector: "player".into(),
            },
        };
        let json = serde_json::to_value(&listen).unwrap();
        assert_eq!(json["source"]["kind"], "player");
        assert!(json.get("album").is_none());
        assert!(json.get("mbids").is_none());
    }

    #[test]
    fn device_token_error_uses_rfc8628_codes() {
        let err: DeviceTokenError = serde_json::from_str(r#"{"error":"slow_down"}"#).unwrap();
        assert_eq!(err.error, DeviceTokenErrorCode::SlowDown);
    }
}
