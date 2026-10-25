//! Синхронизация плейлистов между устройствами через сервер Scrobb
//! (`docs/playlists.md` §7). Два ядра — «ноутбук» и «телефон» — со своими
//! библиотеками, хранилищами и часами говорят с одним эталонным сервером
//! в памяти (`player_testkit::server`). Часы «телефона» отстают на 5 минут.

use std::sync::{Arc, Mutex};

use player_core::{
    Command, Core, CoreConfig, Event, PlaylistSyncState, PlaylistSyncStatus, Ports,
    SCROBB_TOKEN_KEY, ScrobbSettings, ScrobbleState, Stores,
};
use player_domain::{MediaUri, ReplayGain, Track, TrackId};
use player_history::MemoryHistory;
use player_library::MemoryLibrary;
use player_playlists::{MemoryPlaylists, PlaylistId, PlaylistStore};
use player_ports::{PortError, SecureStore};
use player_store_sqlite::SqlitePlaylists;
use player_testkit::disk::TempDir;
use player_testkit::server::{MAIN, ScrobbServer};
use player_testkit::{Fixtures, ManualClock, ManualOutput};
use scrobb_api::paths;

mod common;

use common::{Recorder, engine_config, wait_for};

const BASE: &str = "https://scrobb.test";
const T0: i64 = 1_761_381_065_000;
/// Часы «телефона» отстают.
const SKEW_MS: i64 = 5 * 60_000;

#[derive(Default)]
struct Secrets(Mutex<std::collections::HashMap<String, String>>);

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

const SONGS: [(&str, &str, &str); 4] = [
    ("Кино", "Кукушка", "Чёрный альбом"),
    ("Кино", "Звезда по имени Солнце", "Звезда по имени Солнце"),
    ("Сплин", "Романс", "Гранатовый альбом"),
    ("Аквариум", "Город золотой", "Десять стрел"),
];

/// Одни и те же песни, но свои пути и id на каждом устройстве.
fn library(folder: &str, first_id: u64) -> Arc<MemoryLibrary> {
    let library = Arc::new(MemoryLibrary::new());
    for (n, (artist, title, album)) in SONGS.iter().enumerate() {
        library.insert(Track {
            id: TrackId(first_id + n as u64),
            uri: MediaUri(format!("{folder}{artist} - {title}.flac")),
            title: (*title).into(),
            artists: vec![(*artist).into()],
            album: Some((*album).into()),
            album_artist: Some((*artist).into()),
            duration_ms: Some(200_000),
            track_no: None,
            disc_no: None,
            recording_mbid: None,
            isrc: None,
            replay_gain: ReplayGain::default(),
        });
    }
    library
}

struct Device {
    core: Core,
    recorder: Arc<Recorder>,
    clock: Arc<ManualClock>,
    secrets: Arc<Secrets>,
    first_id: u64,
}

impl Device {
    fn new(
        server: &Arc<ScrobbServer>,
        token: Option<String>,
        playlists: Arc<dyn PlaylistStore>,
        first_id: u64,
        unix_ms: i64,
    ) -> Self {
        let clock = ManualClock::new(unix_ms);
        let secrets = Arc::new(Secrets::default());
        if let Some(token) = token {
            secrets.set(SCROBB_TOKEN_KEY, &token).unwrap();
        }
        let folder = if first_id == 1 {
            "/home/ivan/Музыка/"
        } else {
            "C:\\Music\\"
        };
        let recorder = Arc::new(Recorder::default());
        let ports = Ports {
            clock: clock.clone(),
            audio: Box::new(ManualOutput::default()),
            files: Fixtures::shared(),
            http: Some(server.clone()),
            secrets: Some(secrets.clone()),
            media_session: None,
        };
        let stores = Stores {
            library: library(folder, first_id),
            history: Some(Arc::new(MemoryHistory::new(format!("device-{first_id}")))),
            playlists: Some(playlists),
        };
        let config = CoreConfig {
            engine: engine_config(),
            scrobb: Some(ScrobbSettings {
                base_url: BASE.into(),
                version: "0.1.0".into(),
                platform: "linux".into(),
            }),
            ..CoreConfig::default()
        };
        let core = Core::new(ports, stores, config, recorder.clone());
        let device = Self {
            core,
            recorder,
            clock,
            secrets,
            first_id,
        };
        device.settled(0);
        device
    }

    /// Ноутбук: свежее хранилище в памяти, привязан к основному аккаунту.
    fn laptop(server: &Arc<ScrobbServer>) -> Self {
        Self::new(
            server,
            Some(server.token_for(MAIN)),
            Arc::new(MemoryPlaylists::new()),
            1,
            T0,
        )
    }

    /// Телефон: часы отстают на 5 минут.
    fn phone(server: &Arc<ScrobbServer>) -> Self {
        Self::new(
            server,
            Some(server.token_for(MAIN)),
            Arc::new(MemoryPlaylists::new()),
            11,
            T0 - SKEW_MS,
        )
    }

    fn statuses(&self, from: usize) -> Vec<PlaylistSyncStatus> {
        self.recorder.events()[from..]
            .iter()
            .filter_map(|e| match e {
                Event::PlaylistSync(s) => Some(*s),
                _ => None,
            })
            .collect()
    }

    /// Дождаться итога обмена (не «идёт обмен») после отметки.
    #[track_caller]
    fn settled(&self, mark: usize) -> PlaylistSyncState {
        wait_for("итог синхронизации", || {
            self.statuses(mark)
                .iter()
                .any(|s| s.state != PlaylistSyncState::Syncing)
        });
        self.statuses(mark).last().unwrap().state
    }

    /// Синхронизировать сейчас и дождаться итога.
    #[track_caller]
    fn sync(&self) -> PlaylistSyncState {
        let mark = self.recorder.mark();
        self.core.dispatch(Command::SyncPlaylists).unwrap();
        self.settled(mark)
    }

    fn changed_since(&self, mark: usize) -> bool {
        self.recorder.events()[mark..]
            .iter()
            .any(|e| matches!(e, Event::PlaylistsChanged))
    }

    /// Названия треков плейлиста по порядку.
    fn titles(&self, id: &PlaylistId) -> Vec<String> {
        self.core
            .playlist(id)
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.item.track.title)
            .collect()
    }

    /// id трека библиотеки этого устройства по номеру песни в [`SONGS`].
    fn song(&self, n: u64) -> TrackId {
        TrackId(self.first_id + n)
    }
}

/// Ноутбук завёл плейлист «Дорога» из двух песен, оба устройства синхронизированы.
fn shared_playlist(server: &Arc<ScrobbServer>) -> (Device, Device, PlaylistId) {
    let laptop = Device::laptop(server);
    let phone = Device::phone(server);
    let id = laptop.core.create_playlist("Дорога", None).unwrap().id;
    laptop
        .core
        .add_to_playlist(&id, &[laptop.song(0), laptop.song(1)], None)
        .unwrap();
    assert_eq!(laptop.sync(), PlaylistSyncState::Synced);
    assert_eq!(phone.sync(), PlaylistSyncState::Synced);
    (laptop, phone, id)
}

#[test]
fn playlist_travels_to_another_device_and_finds_its_files() {
    let server = ScrobbServer::new(BASE);
    let laptop = Device::laptop(&server);
    let phone = Device::phone(&server);
    let id = laptop
        .core
        .create_playlist("Дорога", Some("в машину"))
        .unwrap()
        .id;
    laptop
        .core
        .add_to_playlist(&id, &[laptop.song(0), laptop.song(1)], None)
        .unwrap();
    assert_eq!(laptop.sync(), PlaylistSyncState::Synced);

    let sent = server.sync_requests();
    let body = serde_json::to_string(&sent).unwrap();
    assert!(body.contains("Дорога") && body.contains("Кукушка"));
    assert!(
        !body.contains("/home/ivan"),
        "пути к файлам не уходят на сервер"
    );

    let mark = phone.recorder.mark();
    assert_eq!(phone.sync(), PlaylistSyncState::Synced);
    assert!(
        phone.changed_since(mark),
        "окно телефона перечитает плейлисты"
    );
    let view = phone.core.playlist(&id).unwrap();
    assert_eq!(view.playlist.title, "Дорога");
    assert_eq!(view.playlist.description.as_deref(), Some("в машину"));
    let found: Vec<_> = view.items.iter().map(|i| i.track_id).collect();
    assert_eq!(
        found,
        [Some(phone.song(0)), Some(phone.song(1))],
        "треки нашлись в библиотеке телефона по артисту и названию"
    );
    assert!(view.items.iter().all(|i| i.item.location.is_none()));
    let status = phone.statuses(0).last().copied().unwrap();
    assert_eq!(status.last_synced_ms.map(|t| t >= T0 - SKEW_MS), Some(true));

    // Повторный обмен без правок ничего не меняет и не будит окно.
    let mark = phone.recorder.mark();
    phone.sync();
    assert!(!phone.changed_since(mark));
}

#[test]
fn concurrent_edits_converge_and_causality_beats_skewed_clock() {
    let server = ScrobbServer::new(BASE);
    let (laptop, phone, id) = shared_playlist(&server);

    // Одновременно: ноутбук переименовывает, телефон меняет описание,
    // поднимает «Звезду» наверх и добавляет «Романс».
    laptop
        .core
        .update_playlist(&id, "Дорога на дачу", None)
        .unwrap();
    phone
        .core
        .update_playlist(&id, "Дорога", Some("в машину"))
        .unwrap();
    let star = phone.core.playlist(&id).unwrap().items[1].item.id.clone();
    phone.core.move_in_playlist(&id, &star, 0).unwrap();
    phone
        .core
        .add_to_playlist(&id, &[phone.song(2)], None)
        .unwrap();
    laptop.sync();
    phone.sync();
    laptop.sync();
    for device in [&laptop, &phone] {
        let view = device.core.playlist(&id).unwrap();
        assert_eq!(view.playlist.title, "Дорога на дачу");
        assert_eq!(view.playlist.description.as_deref(), Some("в машину"));
        assert_eq!(
            device.titles(&id),
            ["Звезда по имени Солнце", "Кукушка", "Романс"]
        );
    }

    // Телефон уже видел переименование ноутбука. Его часы отстают на 5 минут,
    // но следующее переименование на телефоне — позже по смыслу и побеждает.
    phone.clock.advance(1_000);
    phone
        .core
        .update_playlist(&id, "Дача", Some("в машину"))
        .unwrap();
    phone.sync();
    laptop.sync();
    assert_eq!(laptop.core.playlist(&id).unwrap().playlist.title, "Дача");
    assert_eq!(phone.core.playlist(&id).unwrap().playlist.title, "Дача");
}

#[test]
fn deletion_wins_over_offline_edits_and_removed_item_stays_removed() {
    let server = ScrobbServer::new(BASE);
    let (laptop, phone, id) = shared_playlist(&server);
    let other = laptop.core.create_playlist("Утро", None).unwrap().id;
    laptop
        .core
        .add_to_playlist(&other, &[laptop.song(0), laptop.song(3)], None)
        .unwrap();
    laptop.sync();
    phone.sync();

    // Ноутбук удаляет «Дорогу» и убирает «Кукушку» из «Утра»; телефон, ещё
    // не зная этого, правит «Дорогу» и переставляет ту же «Кукушку».
    laptop.core.delete_playlist(&id).unwrap();
    let cuckoo = laptop.core.playlist(&other).unwrap().items[0]
        .item
        .id
        .clone();
    laptop
        .core
        .remove_from_playlist(&other, std::slice::from_ref(&cuckoo))
        .unwrap();
    laptop.sync();
    phone
        .core
        .update_playlist(&id, "Дорога (моя)", None)
        .unwrap();
    phone
        .core
        .add_to_playlist(&id, &[phone.song(2)], None)
        .unwrap();
    phone.core.move_in_playlist(&other, &cuckoo, 1).unwrap();
    phone.sync();
    laptop.sync();

    for device in [&laptop, &phone] {
        assert!(device.core.playlist(&id).is_err(), "удаление окончательно");
        assert_eq!(device.titles(&other), ["Город золотой"]);
    }
    assert_eq!(
        server
            .live_playlists(MAIN)
            .into_values()
            .collect::<Vec<_>>(),
        ["Утро"]
    );
}

#[test]
fn edits_go_out_by_themselves_and_others_arrive_every_minute() {
    let server = ScrobbServer::new(BASE);
    let laptop = Device::laptop(&server);
    let phone = Device::phone(&server);
    let before = server.sync_requests().len();

    let id = laptop.core.create_playlist("Вечер", None).unwrap().id;
    laptop
        .core
        .add_to_playlist(&id, &[laptop.song(3)], None)
        .unwrap();
    // Несколько правок подряд — один запрос после паузы.
    laptop.clock.advance(1_000);
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert_eq!(server.sync_requests().len(), before, "ещё не пора");
    laptop.clock.advance(600);
    wait_for("отправка после правки", || {
        server
            .sync_requests()
            .iter()
            .any(|r| r.playlists.iter().any(|p| p.title.value == "Вечер"))
    });
    let with_it: Vec<_> = server
        .sync_requests()
        .into_iter()
        .filter(|r| !r.items.is_empty())
        .collect();
    assert_eq!(with_it.len(), 1, "плейлист и элемент — одним запросом");

    // Телефон заберёт сам — при плановом обмене раз в минуту.
    let mark = phone.recorder.mark();
    phone.clock.advance(60_000);
    wait_for("плановый обмен телефона", || {
        phone.changed_since(mark)
    });
    assert_eq!(phone.titles(&id), ["Город золотой"]);
}

#[test]
fn large_playlists_go_in_pages_both_ways() {
    let server = ScrobbServer::new(BASE);
    let laptop = Device::laptop(&server);
    let phone = Device::phone(&server);
    // Сервер не возвращает присланное: продолжать отправку клиент должен сам.
    server.set_echo(false);
    let id = laptop.core.create_playlist("Всё подряд", None).unwrap().id;
    let tracks: Vec<TrackId> = (0..620).map(|n| laptop.song(n % 4)).collect();
    assert_eq!(
        laptop.core.add_to_playlist(&id, &tracks, None).unwrap(),
        620
    );
    laptop.sync();
    let pushed: Vec<usize> = server
        .sync_requests()
        .iter()
        .map(|r| r.items.len())
        .filter(|&n| n > 0)
        .collect();
    assert_eq!(pushed, [500, 120], "элементы — пачками по 500");
    // Переименование после элементов: запись плейлиста на сервере теперь
    // последняя, и к ранним страницам с элементами сервер прикладывает её сам.
    laptop
        .core
        .update_playlist(&id, "Всё подряд, по порядку", None)
        .unwrap();
    laptop.sync();

    server.set_page_limit(50);
    let before = server.sync_requests().len();
    phone.sync();
    let rounds = server.sync_requests().len() - before;
    assert!(
        rounds >= 13,
        "сервер отдаёт по 50 — нужно много запросов ({rounds})"
    );
    assert_eq!(phone.titles(&id), laptop.titles(&id));
    assert_eq!(phone.titles(&id).len(), 620);
    assert_eq!(
        phone.core.playlist(&id).unwrap().playlist.title,
        "Всё подряд, по порядку"
    );
}

#[test]
fn server_problems_are_reported_and_retried() {
    let server = ScrobbServer::new(BASE);
    let laptop = Device::laptop(&server);
    let id = laptop.core.create_playlist("Вечер", None).unwrap().id;

    // Сервер ещё не умеет синхронизацию: спросить снова только через час.
    server.fail_sync(Some(404));
    assert_eq!(laptop.sync(), PlaylistSyncState::Unsupported);
    let asked = server.sync_requests().len();
    laptop.clock.advance(120_000);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let sync_url = format!("{BASE}{}", paths::SYNC_PLAYLISTS);
    let to_sync = || {
        server
            .requests()
            .iter()
            .filter(|r| r.url == sync_url)
            .count()
    };
    assert_eq!(to_sync(), asked, "через две минуты — не спрашивает");
    server.fail_sync(None);
    let mark = laptop.recorder.mark();
    laptop.clock.advance(3_600_000);
    assert_eq!(laptop.settled(mark), PlaylistSyncState::Synced);
    assert_eq!(server.live_playlists(MAIN).len(), 1);

    // Сбой сервера и нет сети — «повтор позже», правки копятся и уходят потом.
    server.fail_sync(Some(503));
    laptop
        .core
        .update_playlist(&id, "Вечер у окна", None)
        .unwrap();
    assert_eq!(laptop.sync(), PlaylistSyncState::Retrying);
    server.fail_sync(None);
    server.set_offline(true);
    laptop
        .core
        .add_to_playlist(&id, &[laptop.song(0)], None)
        .unwrap();
    assert_eq!(laptop.sync(), PlaylistSyncState::Retrying);
    server.set_offline(false);
    assert_eq!(laptop.sync(), PlaylistSyncState::Synced);
    assert_eq!(
        server
            .live_playlists(MAIN)
            .into_values()
            .collect::<Vec<_>>(),
        ["Вечер у окна"]
    );

    // Токен отозван — стоят и синхронизация, и скробблы.
    server.fail_sync(Some(401));
    assert_eq!(laptop.sync(), PlaylistSyncState::Unauthorized);
    assert_eq!(
        laptop.recorder.scrobbling().last().unwrap().state,
        ScrobbleState::Unauthorized
    );
}

#[test]
fn linking_sends_everything_and_new_account_gets_it_all() {
    let server = ScrobbServer::new(BASE);
    let device = Device::new(&server, None, Arc::new(MemoryPlaylists::new()), 1, T0);
    assert_eq!(
        device.statuses(0).last().unwrap().state,
        PlaylistSyncState::NotLinked
    );
    let id = device.core.create_playlist("До привязки", None).unwrap().id;
    device
        .core
        .add_to_playlist(&id, &[device.song(0)], None)
        .unwrap();
    assert_eq!(device.sync(), PlaylistSyncState::NotLinked);
    assert!(
        server.sync_requests().is_empty(),
        "без привязки — ни одного запроса"
    );

    let link = |account: &str| {
        server.link_to(account);
        let mark = device.recorder.mark();
        device
            .core
            .dispatch(Command::LinkScrobbAccount {
                device_name: "Ноутбук".into(),
            })
            .unwrap();
        // Код показан; сервер «подтвердит» при первом опросе через секунду.
        wait_for("код привязки", || {
            device.recorder.events()[mark..]
                .iter()
                .any(|e| matches!(e, Event::ScrobbLinkCode { .. }))
        });
        device.clock.advance(1_000);
        wait_for(
            "синхронизация после привязки",
            || {
                device
                    .statuses(mark)
                    .iter()
                    .any(|s| s.state == PlaylistSyncState::Synced)
            },
        );
    };
    link(MAIN);
    assert_eq!(
        server
            .live_playlists(MAIN)
            .into_values()
            .collect::<Vec<_>>(),
        ["До привязки"]
    );

    device.core.dispatch(Command::UnlinkScrobbAccount).unwrap();
    wait_for("отвязано", || {
        device.secrets.get(SCROBB_TOKEN_KEY).unwrap().is_none()
    });
    link("второй");
    assert_eq!(
        server
            .live_playlists("второй")
            .into_values()
            .collect::<Vec<_>>(),
        ["До привязки"],
        "в новый аккаунт — всё местное заново"
    );
}

#[test]
fn sqlite_device_remembers_what_it_sent_across_restarts() {
    let server = ScrobbServer::new(BASE);
    let dir = TempDir::new("sync-user-db");
    let path = dir.path().join("user.db");
    let token = server.token_for(MAIN);
    let open = |unix_ms: i64| {
        Device::new(
            &server,
            Some(token.clone()),
            Arc::new(SqlitePlaylists::open(&path).unwrap()),
            1,
            unix_ms,
        )
    };
    let first = open(T0);
    let id = first.core.create_playlist("Вечер", None).unwrap().id;
    first
        .core
        .add_to_playlist(&id, &[first.song(0)], None)
        .unwrap();
    first.sync();
    drop(first);

    // Перезапуск с отставшими часами: отправлять нечего, а новая правка —
    // всё равно позже прежних.
    let sent_before = server.sync_requests().len();
    let second = open(T0 - 3_600_000);
    let startup = &server.sync_requests()[sent_before..];
    assert!(
        startup
            .iter()
            .all(|r| r.playlists.is_empty() && r.items.is_empty()),
        "после перезапуска ничего не отправляется повторно"
    );
    assert!(
        startup.first().and_then(|r| r.cursor.clone()).is_some(),
        "курсор запомнен"
    );
    second
        .core
        .update_playlist(&id, "Вечер у окна", None)
        .unwrap();
    second.sync();
    assert_eq!(
        server
            .live_playlists(MAIN)
            .into_values()
            .collect::<Vec<_>>(),
        ["Вечер у окна"]
    );
}
