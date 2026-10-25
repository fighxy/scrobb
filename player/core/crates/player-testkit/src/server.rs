//! Сервер Scrobb в памяти — эталон протокола для тестов и dev-сервера
//! (`examples/scrobb-dev-server.rs`):
//!
//! - привязка устройства (RFC 8628) подтверждается сразу, токен — в аккаунт,
//!   заданный [`ScrobbServer::link_to`];
//! - скробблы и «сейчас играет» принимаются и считаются;
//! - синхронизация плейлистов — по `docs/playlists.md` §7: присланное
//!   сливается по правилам `scrobb_sync::playlists`, каждая изменившаяся
//!   запись получает следующий номер аккаунта, курсор — номер последней
//!   отданной записи, к элементам ответа добавляются их плейлисты.
//!
//! Бэкенд делает то же самое в Postgres; здесь — чтобы проверять клиента
//! целиком, от правки до другого устройства.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use player_ports::{HttpRequest, HttpResponse, HttpTransport, PortError};
use scrobb_api::{
    DeviceAuthorizeResponse, DeviceToken, ListenResult, ListenStatus, SubmitListensRequest,
    SubmitListensResponse, paths,
};
use scrobb_client::Method;
use scrobb_sync::playlists::{ItemRecord, PlaylistRecord, SyncRequest, SyncResponse};
use serde::Serialize;
use serde_json::Value;

/// Аккаунт по умолчанию: в него привязываются устройства.
pub const MAIN: &str = "main";

/// Данные одного аккаунта.
#[derive(Debug, Default)]
pub struct Account {
    /// Номер последнего изменения: курсор синхронизации.
    pub seq: u64,
    /// Плейлисты и номер их последнего изменения.
    pub playlists: BTreeMap<String, (PlaylistRecord, u64)>,
    /// Элементы и номер их последнего изменения.
    pub items: BTreeMap<String, (ItemRecord<Value>, u64)>,
    /// Сколько скробблов принято.
    pub listens: usize,
}

#[derive(Debug)]
struct State {
    accounts: HashMap<String, Account>,
    /// Токен → аккаунт.
    tokens: HashMap<String, String>,
    /// Куда привязывать новые устройства.
    linking: String,
    issued: u32,
    /// Сколько записей сервер отдаёт за раз (меньше — чаще `more`).
    page_limit: usize,
    /// Возвращать ли в ответе то, что прислано этим же запросом.
    echo: bool,
    /// Ответ на синхронизацию вместо обычного: `404` — «не умеем», `500`, `401`.
    sync_status: Option<u16>,
    /// Сеть «упала»: транспорт отвечает ошибкой.
    offline: bool,
    requests: Vec<HttpRequest>,
}

/// Сервер Scrobb в памяти.
#[derive(Debug)]
pub struct ScrobbServer {
    base: String,
    state: Mutex<State>,
}

impl ScrobbServer {
    /// Сервер по адресу `base` (например, `https://scrobb.test`).
    pub fn new(base: &str) -> Arc<Self> {
        Arc::new(Self {
            base: base.trim_end_matches('/').to_owned(),
            state: Mutex::new(State {
                accounts: HashMap::new(),
                tokens: HashMap::new(),
                linking: MAIN.to_owned(),
                issued: 0,
                page_limit: usize::MAX,
                echo: true,
                sync_status: None,
                offline: false,
                requests: Vec::new(),
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Выдать токен уже привязанного устройства в аккаунте `account`.
    pub fn token_for(&self, account: &str) -> String {
        issue(&mut self.lock(), account)
    }

    /// Новые привязки — в аккаунт `account`.
    pub fn link_to(&self, account: &str) {
        self.lock().linking = account.to_owned();
    }

    /// Сколько записей отдавать за раз.
    pub fn set_page_limit(&self, limit: usize) {
        self.lock().page_limit = limit.max(1);
    }

    /// Не возвращать в ответе присланное тем же запросом (спецификация это
    /// разрешает): тогда «ещё есть» говорит только о чужих правках.
    pub fn set_echo(&self, echo: bool) {
        self.lock().echo = echo;
    }

    /// Отвечать на синхронизацию кодом `status` (`None` — как обычно).
    pub fn fail_sync(&self, status: Option<u16>) {
        self.lock().sync_status = status;
    }

    /// «Уронить» или «поднять» сеть.
    pub fn set_offline(&self, offline: bool) {
        self.lock().offline = offline;
    }

    /// Все запросы по порядку.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.lock().requests.clone()
    }

    /// Тела запросов синхронизации по порядку.
    pub fn sync_requests(&self) -> Vec<SyncRequest<Value>> {
        let url = format!("{}{}", self.base, paths::SYNC_PLAYLISTS);
        self.requests()
            .iter()
            .filter(|r| r.url == url)
            .filter_map(|r| serde_json::from_slice(r.body.as_deref().unwrap_or(b"")).ok())
            .collect()
    }

    /// Живые (не удалённые) плейлисты аккаунта: id → название.
    pub fn live_playlists(&self, account: &str) -> BTreeMap<String, String> {
        let state = self.lock();
        state
            .accounts
            .get(account)
            .map(|a| {
                a.playlists
                    .values()
                    .filter(|(p, _)| p.deleted.is_none())
                    .map(|(p, _)| (p.id.clone(), p.title.value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Сколько скробблов принято в аккаунт.
    pub fn listens(&self, account: &str) -> usize {
        self.lock().accounts.get(account).map_or(0, |a| a.listens)
    }

    /// Ответить на запрос: метод, путь от корня сервера, заголовок
    /// `Authorization` и тело. Общий вход для транспорта в тестах и для
    /// настоящего HTTP в dev-сервере.
    pub fn handle(
        &self,
        method: Method,
        path: &str,
        authorization: Option<&str>,
        body: &[u8],
    ) -> (u16, Vec<u8>) {
        let mut state = self.lock();
        let account = authorization
            .and_then(|a| a.strip_prefix("Bearer "))
            .and_then(|t| state.tokens.get(t).cloned());
        let path = path.split('?').next().unwrap_or(path);
        match (method, path) {
            (Method::Post, paths::DEVICE_AUTHORIZE) => {
                state.issued += 1;
                let code = format!("WDJB-{:04}", state.issued);
                json(
                    200,
                    &DeviceAuthorizeResponse {
                        device_code: format!("device-{}", state.issued),
                        user_code: code.clone(),
                        verification_uri: format!("{}/link", self.base),
                        verification_uri_complete: format!("{}/link?code={code}", self.base),
                        expires_in: 600,
                        interval: 1,
                    },
                )
            }
            (Method::Post, paths::DEVICE_TOKEN) => {
                let linking = state.linking.clone();
                let token = issue(&mut state, &linking);
                let token_id = i64::from(state.issued);
                json(
                    200,
                    &DeviceToken {
                        access_token: token,
                        token_id,
                        name: "Scrobb Player".into(),
                    },
                )
            }
            (Method::Delete, paths::DEVICE_CURRENT) => {
                if let Some(token) = authorization.and_then(|a| a.strip_prefix("Bearer ")) {
                    state.tokens.remove(token);
                }
                (204, Vec::new())
            }
            (_, paths::NOW_PLAYING) if account.is_some() => (204, Vec::new()),
            (Method::Post, paths::LISTENS) => {
                let Some(account) = account else {
                    return (401, Vec::new());
                };
                let Ok(batch) = serde_json::from_slice::<SubmitListensRequest>(body) else {
                    return (400, Vec::new());
                };
                state.accounts.entry(account).or_default().listens += batch.listens.len();
                let results = batch
                    .listens
                    .into_iter()
                    .map(|l| ListenResult {
                        client_id: l.client_id,
                        status: ListenStatus::Accepted,
                        reason: None,
                    })
                    .collect();
                json(200, &SubmitListensResponse { results })
            }
            (Method::Post, paths::SYNC_PLAYLISTS) => {
                if let Some(status) = state.sync_status {
                    return (status, Vec::new());
                }
                let Some(account) = account else {
                    return (401, Vec::new());
                };
                let Ok(request) = serde_json::from_slice::<SyncRequest<Value>>(body) else {
                    return (400, Vec::new());
                };
                let (limit, echo) = (state.page_limit, state.echo);
                let account = state.accounts.entry(account).or_default();
                json(200, &sync(account, request, limit, echo))
            }
            _ if account.is_none() => (401, Vec::new()),
            _ => (404, Vec::new()),
        }
    }
}

impl HttpTransport for ScrobbServer {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, PortError> {
        {
            let mut state = self.lock();
            state.requests.push(req.clone());
            if state.offline {
                return Err(PortError::Other("нет сети".into()));
            }
        }
        let path = req
            .url
            .strip_prefix(&self.base)
            .ok_or_else(|| PortError::Other(format!("чужой адрес {}", req.url)))?;
        let authorization = req
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.as_str());
        let (status, body) = self.handle(
            req.method,
            path,
            authorization,
            req.body.as_deref().unwrap_or(b""),
        );
        Ok(HttpResponse {
            status,
            headers: Vec::new(),
            body,
        })
    }
}

fn issue(state: &mut State, account: &str) -> String {
    state.issued += 1;
    let token = format!("scr_{account}_{}", state.issued);
    state.tokens.insert(token.clone(), account.to_owned());
    token
}

fn json(status: u16, value: &impl Serialize) -> (u16, Vec<u8>) {
    (
        status,
        serde_json::to_vec(value).expect("ответ сериализуется"),
    )
}

/// Запись, изменённая после курсора.
enum Changed<'a> {
    Playlist(&'a PlaylistRecord),
    Item(&'a ItemRecord<Value>),
}

/// Слить присланное и отдать изменённое после курсора (`docs/playlists.md` §7).
/// `echo: false` — не возвращать присланное этим запросом.
pub fn sync(
    account: &mut Account,
    request: SyncRequest<Value>,
    page_limit: usize,
    echo: bool,
) -> SyncResponse<Value> {
    let sent: HashSet<String> = if echo {
        HashSet::new()
    } else {
        request
            .playlists
            .iter()
            .map(|p| p.id.clone())
            .chain(request.items.iter().map(|i| i.id.clone()))
            .collect()
    };
    for record in request.playlists {
        let changed = match account.playlists.get_mut(&record.id) {
            Some((local, _)) => local.merge(record.clone()),
            None => {
                account
                    .playlists
                    .insert(record.id.clone(), (record.clone(), 0));
                true
            }
        };
        if changed {
            account.seq += 1;
            account.playlists.get_mut(&record.id).expect("есть").1 = account.seq;
        }
    }
    for record in request.items {
        let changed = match account.items.get_mut(&record.id) {
            Some((local, _)) => local.merge(record.clone()),
            None => {
                account.items.insert(record.id.clone(), (record.clone(), 0));
                true
            }
        };
        if changed {
            account.seq += 1;
            account.items.get_mut(&record.id).expect("есть").1 = account.seq;
        }
    }
    let after: u64 = request
        .cursor
        .as_deref()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let limit = (request.limit as usize).min(page_limit).max(1);
    // Изменённое после курсора по порядку номеров: плейлисты и элементы вместе.
    let mut changed: Vec<(u64, Changed<'_>)> = account
        .playlists
        .values()
        .filter(|(p, seq)| *seq > after && !sent.contains(&p.id))
        .map(|(p, seq)| (*seq, Changed::Playlist(p)))
        .chain(
            account
                .items
                .values()
                .filter(|(i, seq)| *seq > after && !sent.contains(&i.id))
                .map(|(i, seq)| (*seq, Changed::Item(i))),
        )
        .collect();
    changed.sort_by_key(|(seq, _)| *seq);
    let more = changed.len() > limit;
    changed.truncate(limit);
    let cursor = changed.last().map_or(after, |(seq, _)| *seq);
    let mut playlists = Vec::new();
    let mut items = Vec::new();
    for (_, record) in changed {
        match record {
            Changed::Playlist(p) => playlists.push(p.clone()),
            Changed::Item(i) => items.push(i.clone()),
        }
    }
    // У каждого элемента в ответе — его плейлист, даже если тот не менялся.
    for item in &items {
        if !playlists.iter().any(|p| p.id == item.playlist_id)
            && let Some((p, _)) = account.playlists.get(&item.playlist_id)
        {
            playlists.push(p.clone());
        }
    }
    SyncResponse {
        cursor: cursor.to_string(),
        more,
        playlists,
        items,
    }
}
