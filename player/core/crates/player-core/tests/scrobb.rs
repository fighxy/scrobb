//! Связь со Scrobb через фасад: факты воспроизведения → записи истории →
//! очередь → запросы; привязка устройства (RFC 8628) и «сейчас играет».
//! Сервер Scrobb подделан, время двигает тест (`ManualClock`), поэтому
//! «сколько прозвучало» и сроки опросов не зависят от скорости машины.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use player_core::{
    Command, Core, CoreConfig, CoreError, Event, Ports, SCROBB_TOKEN_KEY, ScrobbSettings,
    ScrobbleState, ScrobbleStatus, Stores,
};
use player_domain::TrackId;
use player_history::{HistoryStore, NewPlay, Outcome, PlayId, PlayRecord, TARGET_SCROBB};
use player_library::MemoryLibrary;
use player_ports::{HttpTransport, PortError, SecureStore};
use player_store_sqlite::SqliteHistory;
use player_testkit::{Fixtures, ManualClock, ManualOutput};
use scrobb_api::{
    DeviceAuthorizeRequest, DeviceAuthorizeResponse, DeviceToken, ListenResult, ListenStatus,
    NowPlaying, SubmitListensRequest, SubmitListensResponse, Thresholds, paths,
};
use scrobb_client::{HttpRequest, HttpResponse, Method};

mod common;

use common::{Recorder, engine_config, play_until_seen, playing, start, track, wait_for};

const T0: i64 = 1_760_000_000_000;
/// Длительность треков в метаданных: порог — половина, 50 мс звучания.
const DURATION_MS: u64 = 100;
const TOKEN: &str = "scr_test";
const BASE: &str = "https://scrobb.test";
const USER_CODE: &str = "WDJB-MJHT";

/// Ответ сервера на пачку прослушиваний.
type Reply = Box<dyn Fn(&SubmitListensRequest) -> Result<HttpResponse, PortError> + Send + Sync>;

/// Сервер Scrobb в памяти: запоминает запросы и отвечает по адресу.
struct FakeScrobb {
    requests: Mutex<Vec<HttpRequest>>,
    /// Ответ на пачку прослушиваний.
    listens: Mutex<Reply>,
    /// Ответы на опрос привязки по очереди; кончились — «ещё не подтверждено».
    token_replies: Mutex<VecDeque<HttpResponse>>,
    /// Сколько секунд живёт код привязки.
    code_expires_in: u32,
}

impl FakeScrobb {
    fn accepting() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::default(),
            listens: Mutex::new(Box::new(|req| Ok(answer(req, ListenStatus::Accepted)))),
            token_replies: Mutex::default(),
            code_expires_in: 600,
        })
    }

    fn reply_with(&self, reply: Reply) {
        *self.listens.lock().unwrap() = reply;
    }

    /// Следующие ответы на опрос привязки.
    fn answer_token_polls(&self, replies: impl IntoIterator<Item = HttpResponse>) {
        self.token_replies.lock().unwrap().extend(replies);
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Запросы по адресу API, например `paths::LISTENS`.
    fn to(&self, method: Method, path: &str) -> Vec<HttpRequest> {
        let url = format!("{BASE}{path}");
        self.requests()
            .into_iter()
            .filter(|r| r.method == method && r.url == url)
            .collect()
    }

    /// Пачки прослушиваний, которые пришли на сервер.
    fn batches(&self) -> Vec<SubmitListensRequest> {
        self.to(Method::Post, paths::LISTENS)
            .iter()
            .map(body)
            .collect()
    }

    /// «Сейчас играет» по порядку: `Some` — трек и позиция, `None` — сброс.
    fn now_playing(&self) -> Vec<Option<(String, Option<u64>)>> {
        let url = format!("{BASE}{}", paths::NOW_PLAYING);
        self.requests()
            .iter()
            .filter(|r| r.url == url)
            .map(|r| match r.method {
                Method::Post => {
                    let np: NowPlaying = body(r);
                    Some((np.track, np.position_ms))
                }
                _ => None,
            })
            .collect()
    }
}

impl HttpTransport for FakeScrobb {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, PortError> {
        self.requests.lock().unwrap().push(req.clone());
        let path = req.url.strip_prefix(BASE).unwrap();
        match (req.method, path) {
            (Method::Post, paths::LISTENS) => (self.listens.lock().unwrap())(&body(req)),
            (Method::Post, paths::DEVICE_AUTHORIZE) => Ok(json(
                200,
                &DeviceAuthorizeResponse {
                    device_code: "device-code".into(),
                    user_code: USER_CODE.into(),
                    verification_uri: format!("{BASE}/link"),
                    verification_uri_complete: format!("{BASE}/link?code={USER_CODE}"),
                    expires_in: self.code_expires_in,
                    interval: 1,
                },
            )),
            (Method::Post, paths::DEVICE_TOKEN) => Ok(self
                .token_replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| token_error("authorization_pending"))),
            _ => Ok(HttpResponse {
                status: 204,
                headers: Vec::new(),
                body: Vec::new(),
            }),
        }
    }
}

fn body<T: serde::de::DeserializeOwned>(req: &HttpRequest) -> T {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn json(status: u16, value: &impl serde::Serialize) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: serde_json::to_vec(value).unwrap(),
    }
}

fn answer(req: &SubmitListensRequest, status: ListenStatus) -> HttpResponse {
    let results = req
        .listens
        .iter()
        .map(|l| ListenResult {
            client_id: l.client_id.clone(),
            status,
            reason: None,
        })
        .collect();
    json(200, &SubmitListensResponse { results })
}

fn unauthorized() -> HttpResponse {
    HttpResponse {
        status: 401,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

/// Ошибка опроса привязки по RFC 8628: `authorization_pending`, `slow_down`…
fn token_error(code: &str) -> HttpResponse {
    json(400, &serde_json::json!({ "error": code }))
}

fn token_ok(token: &str) -> HttpResponse {
    json(
        200,
        &DeviceToken {
            access_token: token.into(),
            token_id: 7,
            name: "Pixel 8".into(),
        },
    )
}

#[derive(Default)]
struct Secrets(Mutex<HashMap<String, String>>);

impl Secrets {
    fn with_token() -> Arc<Self> {
        let secrets = Arc::new(Self::default());
        secrets.set(SCROBB_TOKEN_KEY, TOKEN).unwrap();
        secrets
    }

    fn token(&self) -> Option<String> {
        self.get(SCROBB_TOKEN_KEY).unwrap()
    }
}

impl SecureStore for Secrets {
    fn get(&self, key: &str) -> Result<Option<String>, PortError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PortError> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), PortError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Ядро с историей; `scrobb` — сервер и секреты, если связь со Scrobb включена.
struct Harness {
    core: Core,
    output: ManualOutput,
    recorder: Arc<Recorder>,
    clock: Arc<ManualClock>,
    store: Arc<SqliteHistory>,
}

impl Harness {
    fn new(scrobb: Option<(Arc<FakeScrobb>, Arc<Secrets>)>) -> Self {
        Self::with_store(Arc::new(SqliteHistory::open_in_memory().unwrap()), scrobb)
    }

    fn with_store(
        store: Arc<SqliteHistory>,
        scrobb: Option<(Arc<FakeScrobb>, Arc<Secrets>)>,
    ) -> Self {
        let library = MemoryLibrary::new();
        for id in 1..=3 {
            let mut t = track(id, "full.flac");
            t.title = format!("Трек {id}");
            t.duration_ms = Some(DURATION_MS);
            library.insert(t);
        }
        let output = ManualOutput::default();
        let recorder = Arc::new(Recorder::default());
        let clock = ManualClock::new(T0);
        let (http, secrets) = match &scrobb {
            Some((http, secrets)) => (
                Some(http.clone() as Arc<dyn HttpTransport>),
                Some(secrets.clone() as Arc<dyn SecureStore>),
            ),
            None => (None, None),
        };
        let ports = Ports {
            clock: clock.clone(),
            audio: Box::new(output.clone()),
            files: Fixtures::shared(),
            http,
            secrets,
            media_session: None,
        };
        let config = CoreConfig {
            engine: engine_config(),
            thresholds: Thresholds {
                min_track_ms: 0,
                ..Thresholds::default()
            },
            scrobb: scrobb.map(|_| ScrobbSettings {
                base_url: BASE.into(),
                version: "0.1.0".into(),
                platform: "linux".into(),
            }),
        };
        let stores = Stores {
            library: Arc::new(library),
            history: Some(store.clone()),
            playlists: None,
        };
        let core = Core::new(ports, stores, config, recorder.clone());
        Self {
            core,
            output,
            recorder,
            clock,
            store,
        }
    }

    fn start(&self, ids: &[u64], first: u64) {
        start(&self.core, &self.output, &self.recorder, ids, first);
    }

    /// Записи истории, старые первыми.
    fn plays(&self) -> Vec<PlayRecord> {
        let mut plays = self.core.recent_plays(None, 100).unwrap();
        plays.reverse();
        plays
    }

    /// Дождаться `n` закрытых записей.
    #[track_caller]
    fn closed_plays(&self, n: usize) -> Vec<PlayRecord> {
        wait_for(&format!("{n} закрытых записей"), || {
            let plays = self.plays();
            plays.len() == n && plays.iter().all(|p| p.outcome != Outcome::Playing)
        });
        self.plays()
    }

    fn pending(&self) -> usize {
        self.store.pending_count(TARGET_SCROBB).unwrap()
    }

    #[track_caller]
    fn wait_status(&self, pending: usize, state: ScrobbleState) {
        let status = ScrobbleStatus { pending, state };
        wait_for(&format!("{status:?}"), || {
            self.recorder.scrobbling().last() == Some(&status)
        });
    }

    /// Ошибки, которые ядро показало пользователю.
    fn errors(&self) -> Vec<String> {
        self.recorder.errors()
    }

    /// Начать привязку и дождаться кода.
    #[track_caller]
    fn link(&self) {
        self.core
            .dispatch(Command::LinkScrobbAccount {
                device_name: "Pixel 8".into(),
            })
            .unwrap();
        self.wait_status(0, ScrobbleState::Linking);
    }

    /// Перемотать и дождаться, пока ядро получит от движка «перемотано».
    /// Звук не забираем: короткий тестовый трек иначе доиграл бы до конца.
    fn seek(&self, position_ms: u64) {
        let mark = self.recorder.mark();
        self.core.dispatch(Command::Seek { position_ms }).unwrap();
        wait_for("перемотка", || {
            self.recorder.events()[mark..]
                .iter()
                .any(|e| matches!(e, Event::State(_)))
        });
    }
}

/// Убедиться, что за 50 мс ничего не произошло: поток истории сверяется с
/// часами теста каждые 2 мс, так что лишний запрос успел бы появиться.
#[track_caller]
fn stays(what: &str, cond: impl Fn() -> bool) {
    std::thread::sleep(Duration::from_millis(50));
    assert!(cond(), "{what}");
}

// --- История и отправка ---

#[test]
fn every_play_is_recorded_with_its_outcome() {
    let h = Harness::new(None);
    h.start(&[1, 2, 3], 1);
    // Порог 50 мс пройден, затем «следующий»: пропущен, но засчитан.
    h.clock.advance(80);
    let mark = h.recorder.mark();
    h.core.dispatch(Command::Next).unwrap();
    play_until_seen(&h.output, &h.recorder, mark, playing(2));
    // Второй доигрывает до третьего без паузы: дослушан, но время не шло — не засчитан.
    let mark = h.recorder.mark();
    play_until_seen(&h.output, &h.recorder, mark, playing(3));
    h.clock.advance(10);
    h.core.dispatch(Command::Stop).unwrap();

    let plays = h.closed_plays(3);
    let summary: Vec<_> = plays
        .iter()
        .map(|p| (p.track_id, p.outcome, p.scrobbled, p.played_ms))
        .collect();
    assert_eq!(
        summary,
        [
            (Some(TrackId(1)), Outcome::Skipped, true, 80),
            (Some(TrackId(2)), Outcome::Finished, false, 0),
            (Some(TrackId(3)), Outcome::Stopped, false, 10),
        ]
    );
    let first = &plays[0];
    assert_eq!(first.started_at_ms, T0);
    assert_eq!(first.ended_at_ms, Some(T0 + 80));
    assert_eq!(first.track.title, "Трек 1");
    assert_eq!(first.track.artists, ["Тест"]);
    assert_eq!(first.device_id, h.store.device_id());
    assert_eq!(plays[2].started_at_ms, T0 + 80);
    // Отправка не настроена: в очередь ничего не встало.
    assert_eq!(h.pending(), 0);
}

#[test]
fn pause_is_not_counted_and_rewind_starts_new_play() {
    let h = Harness::new(None);
    h.start(&[1], 1);
    h.clock.advance(20);
    h.core.dispatch(Command::Pause).unwrap();
    h.clock.advance(10_000);
    h.core.dispatch(Command::Resume).unwrap();
    h.clock.advance(10);
    // В самое начало — новое прослушивание того же трека (§18.7).
    h.core.dispatch(Command::Seek { position_ms: 0 }).unwrap();
    h.clock.advance(5);
    h.core.dispatch(Command::Stop).unwrap();

    let plays = h.closed_plays(2);
    let summary: Vec<_> = plays.iter().map(|p| (p.outcome, p.played_ms)).collect();
    assert_eq!(summary, [(Outcome::Skipped, 30), (Outcome::Stopped, 5)]);
    assert_eq!(plays[1].track_id, Some(TrackId(1)));
    assert_ne!(plays[0].id, plays[1].id);
}

#[test]
fn scrobble_is_sent_with_play_id_once_threshold_passed() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    h.wait_status(0, ScrobbleState::Ready);
    h.start(&[1], 1);
    h.clock.advance(60);
    // Порог пройден во время звучания — отправка сразу, не дожидаясь конца трека.
    wait_for("пачка в Scrobb", || !server.batches().is_empty());

    let request = &server.to(Method::Post, paths::LISTENS)[0];
    assert!(
        request
            .headers
            .contains(&("Authorization".into(), format!("Bearer {TOKEN}")))
    );
    let listen = &server.batches()[0].listens[0];
    let play = &h.plays()[0];
    assert_eq!(play.outcome, Outcome::Playing);
    assert_eq!(listen.client_id, play.id.0);
    assert_eq!(listen.listened_at, T0 / 1000);
    assert_eq!(listen.played_ms, 60);
    assert_eq!(listen.track, "Трек 1");
    assert_eq!(listen.duration_ms, Some(DURATION_MS));
    wait_for("очередь пуста", || h.pending() == 0);

    h.core.dispatch(Command::Stop).unwrap();
    assert!(h.closed_plays(1)[0].scrobbled);
    assert_eq!(
        server.batches().len(),
        1,
        "засчитанное отправляется один раз"
    );
}

#[test]
fn revoked_token_keeps_queue_until_new_link() {
    let server = FakeScrobb::accepting();
    server.reply_with(Box::new(|_| Ok(unauthorized())));
    let secrets = Secrets::with_token();
    let h = Harness::new(Some((server.clone(), secrets.clone())));
    h.start(&[1], 1);
    h.clock.advance(60);
    h.wait_status(1, ScrobbleState::Unauthorized);
    assert_eq!(h.pending(), 1, "очередь цела");

    // С тем же токеном не пробуем, даже когда подошёл срок.
    h.clock.advance(30_000);
    stays(
        "с отозванным токеном запросов нет",
        || server.batches().len() == 1,
    );

    // Токен сменился (устройство привязали заново): очередь уходит сама.
    secrets.set(SCROBB_TOKEN_KEY, "scr_new").unwrap();
    server.reply_with(Box::new(|req| Ok(answer(req, ListenStatus::Duplicate))));
    h.clock.advance(30_000);
    h.wait_status(0, ScrobbleState::Ready);
    assert_eq!(server.batches().len(), 2);
}

#[test]
fn server_errors_are_retried_later() {
    let server = FakeScrobb::accepting();
    server.reply_with(Box::new(|_| Err(PortError::Other("нет сети".into()))));
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    h.start(&[1], 1);
    h.clock.advance(60);
    h.wait_status(1, ScrobbleState::Retrying);

    // Сеть вернулась; первая задержка — от 30 с до минуты.
    server.reply_with(Box::new(|req| Ok(answer(req, ListenStatus::Accepted))));
    h.clock.advance(60_000);
    h.wait_status(0, ScrobbleState::Ready);
    assert_eq!(server.batches().len(), 2);
}

#[test]
fn without_link_plays_are_scrobbled_but_not_queued() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Arc::new(Secrets::default()))));
    h.wait_status(0, ScrobbleState::NotLinked);
    h.start(&[1], 1);
    h.clock.advance(60);
    h.core.dispatch(Command::Stop).unwrap();
    assert!(h.closed_plays(1)[0].scrobbled);
    assert_eq!(h.pending(), 0);
    assert!(server.requests().is_empty(), "без токена в сеть не ходим");
}

#[test]
fn play_left_open_by_crash_is_closed_on_start() {
    let store = Arc::new(SqliteHistory::open_in_memory().unwrap());
    let id = PlayId("0190a5b2-0000-7000-8000-000000000001".into());
    store
        .begin(&NewPlay {
            id: id.clone(),
            started_at_ms: T0 - 60_000,
            track: Default::default(),
            track_id: Some(TrackId(1)),
        })
        .unwrap();
    store.checkpoint(&id, 15_000).unwrap();
    let h = Harness::with_store(store, None);
    let plays = h.closed_plays(1);
    assert_eq!(plays[0].outcome, Outcome::Interrupted);
    assert_eq!(plays[0].ended_at_ms, Some(T0 - 45_000));
}

// --- Привязка устройства ---

#[test]
fn linking_saves_token_and_starts_sending() {
    let server = FakeScrobb::accepting();
    let secrets = Arc::new(Secrets::default());
    let h = Harness::new(Some((server.clone(), secrets.clone())));
    h.wait_status(0, ScrobbleState::NotLinked);
    h.link();

    // Код показан, страница с кодом — для системного браузера.
    let code = h.recorder.events().into_iter().find_map(|e| match e {
        Event::ScrobbLinkCode {
            user_code,
            verification_uri_complete,
        } => Some((user_code, verification_uri_complete)),
        _ => None,
    });
    assert_eq!(
        code,
        Some((USER_CODE.into(), format!("{BASE}/link?code={USER_CODE}")))
    );
    let authorize: DeviceAuthorizeRequest =
        body(&server.to(Method::Post, paths::DEVICE_AUTHORIZE)[0]);
    assert_eq!(authorize.device_name, "Pixel 8");
    assert_eq!(
        serde_json::to_value(authorize.client_kind).unwrap(),
        "player"
    );
    assert_eq!(
        (
            authorize.platform.as_str(),
            authorize.client_version.as_str()
        ),
        ("linux", "0.1.0")
    );

    // Опрос не чаще `interval`: пользователь ещё не подтвердил, потом подтвердил.
    server.answer_token_polls([token_error("authorization_pending"), token_ok("scr_new")]);
    stays("до интервала не опрашиваем", || {
        server.to(Method::Post, paths::DEVICE_TOKEN).is_empty()
    });
    h.clock.advance(1_000);
    wait_for("первый опрос", || {
        server.to(Method::Post, paths::DEVICE_TOKEN).len() == 1
    });
    h.clock.advance(1_000);
    h.wait_status(0, ScrobbleState::Ready);
    assert_eq!(secrets.token().as_deref(), Some("scr_new"));
    assert!(h.errors().is_empty(), "{:?}", h.errors());

    // Теперь засчитанное уходит с новым токеном.
    h.start(&[1], 1);
    h.clock.advance(60);
    wait_for("пачка в Scrobb", || !server.batches().is_empty());
    assert!(
        server.to(Method::Post, paths::LISTENS)[0]
            .headers
            .contains(&("Authorization".into(), "Bearer scr_new".into()))
    );
}

#[test]
fn slow_down_stretches_polling_and_denial_ends_linking() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Arc::new(Secrets::default()))));
    h.link();
    server.answer_token_polls([token_error("slow_down"), token_error("access_denied")]);
    let polls = || server.to(Method::Post, paths::DEVICE_TOKEN).len();

    h.clock.advance(1_000);
    wait_for("первый опрос", || polls() == 1);
    // `slow_down`: интервал вырос с 1 до 6 с (RFC 8628, §3.5).
    h.clock.advance(5_999);
    stays("раньше нового интервала не опрашиваем", || polls() == 1);
    h.clock.advance(1);
    h.wait_status(0, ScrobbleState::NotLinked);
    assert_eq!(polls(), 2);
    assert!(h.errors().iter().any(|e| e.contains("отклонена")));
}

#[test]
fn expired_code_and_cancelled_linking_stop_polling() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Arc::new(Secrets::default()))));
    h.link();
    // Код живёт 600 с: позже опрашивать бессмысленно.
    h.clock.advance(601_000);
    h.wait_status(0, ScrobbleState::NotLinked);
    assert!(h.errors().iter().any(|e| e.contains("истёк")));
    assert!(server.to(Method::Post, paths::DEVICE_TOKEN).is_empty());

    // Пользователь закрыл окно с кодом.
    h.link();
    h.core.dispatch(Command::CancelScrobbLink).unwrap();
    h.wait_status(0, ScrobbleState::NotLinked);
    h.clock.advance(10_000);
    stays("после отмены не опрашиваем", || {
        server.to(Method::Post, paths::DEVICE_TOKEN).is_empty()
    });
}

#[test]
fn unlinking_revokes_token_and_keeps_history() {
    let server = FakeScrobb::accepting();
    let secrets = Secrets::with_token();
    let h = Harness::new(Some((server.clone(), secrets.clone())));
    h.start(&[1], 1);
    wait_for("сейчас играет", || {
        !server.now_playing().is_empty()
    });

    h.core.dispatch(Command::UnlinkScrobbAccount).unwrap();
    h.wait_status(0, ScrobbleState::NotLinked);
    assert_eq!(secrets.token(), None);
    // Сначала снять «сейчас играет», пока токен действует, затем отозвать токен.
    let calls: Vec<_> = server
        .requests()
        .iter()
        .map(|r| (r.method, r.url.strip_prefix(BASE).unwrap().to_owned()))
        .collect();
    assert_eq!(
        calls,
        [
            (Method::Post, paths::NOW_PLAYING.to_owned()),
            (Method::Delete, paths::NOW_PLAYING.to_owned()),
            (Method::Delete, paths::DEVICE_CURRENT.to_owned()),
        ]
    );

    // История пишется дальше, но ничего не отправляется.
    h.clock.advance(60);
    h.core.dispatch(Command::Stop).unwrap();
    assert!(h.closed_plays(1)[0].scrobbled);
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn scrobb_commands_need_configured_connection() {
    let h = Harness::new(None);
    let link = Command::LinkScrobbAccount {
        device_name: "Pixel 8".into(),
    };
    assert_eq!(h.core.dispatch(link), Err(CoreError::MissingPort("http")));
    assert!(!h.core.capabilities().scrobbling);
    let linked = Harness::new(Some((FakeScrobb::accepting(), Secrets::with_token())));
    assert!(linked.core.capabilities().scrobbling);
}

// --- «Сейчас играет» ---

#[test]
fn now_playing_follows_pause_seek_and_stop() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    let count = || server.now_playing().len();
    h.start(&[1], 1);
    wait_for("трек начался", || count() == 1);
    assert_eq!(server.now_playing()[0], Some(("Трек 1".into(), Some(0))));

    // Пауза — ничего; пауза дольше 5 минут — сброс; продолжение — снова.
    h.core.dispatch(Command::Pause).unwrap();
    h.clock.advance(299_999);
    stays("короткая пауза не сбрасывает", || count() == 1);
    h.clock.advance(1);
    wait_for("сброс после паузы", || count() == 2);
    assert_eq!(server.now_playing()[1], None);
    h.core.dispatch(Command::Resume).unwrap();
    wait_for("продолжение", || count() == 3);
    assert!(matches!(&server.now_playing()[2], Some((t, Some(_))) if t == "Трек 1"));

    // Перемотка — не чаще раза в минуту.
    h.seek(150);
    stays(
        "перемотка вскоре после продолжения не шлётся",
        || count() == 3,
    );
    h.clock.advance(60_000);
    h.seek(300);
    wait_for("перемотка через минуту", || {
        count() == 4
    });
    assert_eq!(server.now_playing()[3], Some(("Трек 1".into(), Some(300))));

    // Стоп — сброс.
    h.core.dispatch(Command::Stop).unwrap();
    wait_for("сброс на стопе", || count() == 5);
    assert_eq!(server.now_playing()[4], None);
}

#[test]
fn next_track_replaces_now_playing_without_clearing() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    h.start(&[1, 2], 1);
    let mark = h.recorder.mark();
    h.core.dispatch(Command::Next).unwrap();
    play_until_seen(&h.output, &h.recorder, mark, playing(2));
    wait_for("трек 2", || server.now_playing().len() >= 2);
    assert_eq!(
        server.now_playing()[..2],
        [
            Some(("Трек 1".into(), Some(0))),
            Some(("Трек 2".into(), Some(0)))
        ]
    );
}

#[test]
fn now_playing_is_cleared_when_core_closes() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    h.start(&[1], 1);
    wait_for("сейчас играет", || {
        server.now_playing().len() == 1
    });
    drop(h);
    assert_eq!(server.now_playing().last(), Some(&None));
}
