//! История и скробблинг через фасад: факты воспроизведения → записи истории →
//! очередь → запрос в Scrobb. Время двигает тест (`ManualClock`), поэтому
//! «сколько прозвучало» не зависит от скорости машины.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use player_core::{
    Command, Core, CoreConfig, Ports, SCROBB_TOKEN_KEY, ScrobbSettings, ScrobbleState,
    ScrobbleStatus, Stores,
};
use player_domain::TrackId;
use player_history::{HistoryStore, NewPlay, Outcome, PlayId, PlayRecord, TARGET_SCROBB};
use player_library::MemoryLibrary;
use player_ports::{HttpTransport, PortError, SecureStore};
use player_store_sqlite::SqliteHistory;
use player_testkit::{Fixtures, ManualClock, ManualOutput};
use scrobb_api::{
    ListenResult, ListenStatus, SubmitListensRequest, SubmitListensResponse, Thresholds,
};
use scrobb_client::{HttpRequest, HttpResponse, Method};

mod common;

use common::{Recorder, engine_config, play_until_seen, playing, start, track, wait_for};

const T0: i64 = 1_760_000_000_000;
/// Длительность треков в метаданных: порог — половина, 50 мс звучания.
const DURATION_MS: u64 = 100;
const TOKEN: &str = "scr_test";

/// Ответ сервера на пачку прослушиваний.
type Reply = Box<dyn Fn(&SubmitListensRequest) -> Result<HttpResponse, PortError> + Send + Sync>;

/// Сервер Scrobb в памяти: запоминает запросы, отвечает по `reply`.
struct FakeScrobb {
    requests: Mutex<Vec<HttpRequest>>,
    reply: Mutex<Reply>,
}

impl FakeScrobb {
    fn accepting() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::default(),
            reply: Mutex::new(Box::new(|req| Ok(answer(req, ListenStatus::Accepted)))),
        })
    }

    fn reply_with(&self, reply: Reply) {
        *self.reply.lock().unwrap() = reply;
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// Пачки прослушиваний, которые пришли на сервер.
    fn batches(&self) -> Vec<SubmitListensRequest> {
        self.requests()
            .iter()
            .map(|r| serde_json::from_slice(r.body.as_deref().unwrap()).unwrap())
            .collect()
    }
}

impl HttpTransport for FakeScrobb {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, PortError> {
        self.requests.lock().unwrap().push(req.clone());
        let body = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        (self.reply.lock().unwrap())(&body)
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
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: serde_json::to_vec(&SubmitListensResponse { results }).unwrap(),
    }
}

#[derive(Default)]
struct Secrets(Mutex<HashMap<String, String>>);

impl Secrets {
    fn with_token() -> Arc<Self> {
        let secrets = Arc::new(Self::default());
        secrets.set(SCROBB_TOKEN_KEY, TOKEN).unwrap();
        secrets
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

/// Ядро с историей; `scrobb` — сервер и секреты, если отправка включена.
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
                base_url: "https://scrobb.test".into(),
                version: "0.1.0".into(),
                platform: "linux".into(),
            }),
        };
        let stores = Stores {
            library: Arc::new(library),
            history: Some(store.clone()),
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
    fn wait_status(&self, status: ScrobbleStatus) {
        wait_for(&format!("{status:?}"), || {
            self.recorder.scrobbling().last() == Some(&status)
        });
    }
}

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
    assert_eq!(first.track.title, "full.flac");
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
    h.wait_status(ScrobbleStatus {
        pending: 0,
        state: ScrobbleState::Ready,
    });
    h.start(&[1], 1);
    h.clock.advance(60);
    // Порог пройден во время звучания — отправка сразу, не дожидаясь конца трека.
    wait_for("запрос в Scrobb", || !server.requests().is_empty());

    let request = &server.requests()[0];
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.url, "https://scrobb.test/api/v1/scrobble/listens");
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
    assert_eq!(listen.track, "full.flac");
    assert_eq!(listen.duration_ms, Some(DURATION_MS));
    wait_for("очередь пуста", || h.pending() == 0);

    h.core.dispatch(Command::Stop).unwrap();
    assert!(h.closed_plays(1)[0].scrobbled);
    assert_eq!(
        server.requests().len(),
        1,
        "засчитанное отправляется один раз"
    );
}

#[test]
fn revoked_token_keeps_queue_until_new_link() {
    let server = FakeScrobb::accepting();
    server.reply_with(Box::new(|_| {
        Ok(HttpResponse {
            status: 401,
            headers: Vec::new(),
            body: Vec::new(),
        })
    }));
    let secrets = Secrets::with_token();
    let h = Harness::new(Some((server.clone(), secrets.clone())));
    h.start(&[1], 1);
    h.clock.advance(60);
    h.wait_status(ScrobbleStatus {
        pending: 1,
        state: ScrobbleState::Unauthorized,
    });
    assert_eq!(h.pending(), 1, "очередь цела");

    // С тем же токеном не пробуем, даже когда подошёл срок.
    h.clock.advance(30_000);
    h.core.dispatch(Command::Pause).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(server.requests().len(), 1);

    // Устройство привязали заново: очередь уходит при следующей попытке.
    secrets.set(SCROBB_TOKEN_KEY, "scr_new").unwrap();
    server.reply_with(Box::new(|req| Ok(answer(req, ListenStatus::Duplicate))));
    h.clock.advance(30_000);
    h.core.dispatch(Command::Resume).unwrap();
    h.wait_status(ScrobbleStatus {
        pending: 0,
        state: ScrobbleState::Ready,
    });
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn server_errors_are_retried_later() {
    let server = FakeScrobb::accepting();
    server.reply_with(Box::new(|_| Err(PortError::Other("нет сети".into()))));
    let h = Harness::new(Some((server.clone(), Secrets::with_token())));
    h.start(&[1], 1);
    h.clock.advance(60);
    h.wait_status(ScrobbleStatus {
        pending: 1,
        state: ScrobbleState::Retrying,
    });

    // Сеть вернулась; первая задержка — от 30 с до минуты.
    server.reply_with(Box::new(|req| Ok(answer(req, ListenStatus::Accepted))));
    h.clock.advance(60_000);
    h.core.dispatch(Command::Pause).unwrap();
    h.wait_status(ScrobbleStatus {
        pending: 0,
        state: ScrobbleState::Ready,
    });
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn without_link_plays_are_scrobbled_but_not_queued() {
    let server = FakeScrobb::accepting();
    let h = Harness::new(Some((server.clone(), Arc::new(Secrets::default()))));
    h.wait_status(ScrobbleStatus {
        pending: 0,
        state: ScrobbleState::NotLinked,
    });
    h.start(&[1], 1);
    h.clock.advance(60);
    h.core.dispatch(Command::Stop).unwrap();
    assert!(h.closed_plays(1)[0].scrobbled);
    assert_eq!(h.pending(), 0);
    assert!(server.requests().is_empty());
}

#[test]
fn play_left_open_by_crash_is_closed_on_start() {
    let store = Arc::new(SqliteHistory::open_in_memory().unwrap());
    store
        .begin(&NewPlay {
            id: PlayId("0190a5b2-0000-7000-8000-000000000001".into()),
            started_at_ms: T0 - 60_000,
            track: Default::default(),
            track_id: Some(TrackId(1)),
        })
        .unwrap();
    store
        .checkpoint(
            &PlayId("0190a5b2-0000-7000-8000-000000000001".into()),
            15_000,
        )
        .unwrap();
    let h = Harness::with_store(store, None);
    let plays = h.closed_plays(1);
    assert_eq!(plays[0].outcome, Outcome::Interrupted);
    assert_eq!(plays[0].ended_at_ms, Some(T0 - 45_000));
}
