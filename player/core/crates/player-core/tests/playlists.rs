//! Плейлисты в фасаде: правки, связь с библиотекой, импорт и экспорт.
//! Хранилище — настоящий SQLite в памяти, библиотека — `MemoryLibrary`.

use std::sync::Arc;

use player_core::{Core, CoreConfig, CoreError, Ports, Stores};
use player_domain::{MediaUri, ReplayGain, Track, TrackId};
use player_library::MemoryLibrary;
use player_playlists::formats::Format;
use player_playlists::{ItemId, PlaylistId};
use player_store_sqlite::SqlitePlaylists;
use player_testkit::{Fixtures, ManualClock, ManualOutput};

mod common;

use common::{Recorder, engine_config};

fn song(id: u64, path: &str, artist: &str, title: &str, album: &str) -> Track {
    Track {
        id: TrackId(id),
        uri: MediaUri(path.into()),
        title: title.into(),
        artists: vec![artist.into()],
        album: Some(album.into()),
        album_artist: Some(artist.into()),
        duration_ms: Some(200_000 + id * 1000),
        track_no: None,
        disc_no: None,
        recording_mbid: None,
        isrc: (id == 2).then(|| "RUA018700012".into()),
        replay_gain: ReplayGain::default(),
    }
}

struct Fixture {
    core: Core,
    library: Arc<MemoryLibrary>,
    clock: Arc<ManualClock>,
}

fn fixture(with_store: bool) -> Fixture {
    let library = Arc::new(MemoryLibrary::new());
    library.insert(song(
        1,
        "/home/ivan/Музыка/Кино/01 Кукушка.flac",
        "Кино",
        "Кукушка",
        "Чёрный альбом",
    ));
    library.insert(song(
        2,
        "/home/ivan/Музыка/Кино/02 Звезда.flac",
        "Кино",
        "Звезда по имени Солнце",
        "Звезда",
    ));
    library.insert(song(
        3,
        "/home/ivan/Музыка/Сплин/Романс.flac",
        "Сплин",
        "Романс",
        "Гранатовый альбом",
    ));
    library.insert(song(
        4,
        "/home/ivan/Музыка/Кино/03 Группа крови.flac",
        "Кино",
        "Группа крови",
        "Звезда",
    ));
    let clock = ManualClock::new(1_761_381_065_000);
    let ports = Ports {
        clock: clock.clone(),
        audio: Box::new(ManualOutput::default()),
        files: Fixtures::shared(),
        http: None,
        secrets: None,
        media_session: None,
    };
    let stores = Stores {
        library: library.clone(),
        history: None,
        playlists: with_store.then(|| Arc::new(SqlitePlaylists::open_in_memory().unwrap()) as _),
    };
    let config = CoreConfig {
        engine: engine_config(),
        ..CoreConfig::default()
    };
    let core = Core::new(ports, stores, config, Arc::new(Recorder::default()));
    Fixture {
        core,
        library,
        clock,
    }
}

fn titles(core: &Core, id: &PlaylistId) -> Vec<String> {
    core.playlist(id)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.item.track.title)
        .collect()
}

#[test]
fn create_add_move_remove_and_list() {
    let f = fixture(true);
    let core = &f.core;
    assert!(matches!(
        core.create_playlist("   ", None),
        Err(CoreError::Playlist(_))
    ));
    let p = core.create_playlist(" В дорогу ", Some("  ")).unwrap();
    assert_eq!(
        (p.title.as_str(), p.description.as_deref()),
        ("В дорогу", None)
    );

    assert_eq!(
        core.add_to_playlist(&p.id, &[TrackId(1), TrackId(3), TrackId(99)], None)
            .unwrap(),
        2
    );
    assert_eq!(
        core.add_to_playlist(&p.id, &[TrackId(2)], Some(1)).unwrap(),
        1
    );
    assert_eq!(
        titles(core, &p.id),
        ["Кукушка", "Звезда по имени Солнце", "Романс"]
    );

    let view = core.playlist(&p.id).unwrap();
    assert_eq!(view.items[1].track_id, Some(TrackId(2)));
    assert_eq!(
        view.items[1].item.track.isrc(),
        Some("RUA018700012"),
        "ISRC из тегов — в снимке"
    );
    assert_eq!(
        view.items[0].item.location.as_deref(),
        Some("/home/ivan/Музыка/Кино/01 Кукушка.flac")
    );

    let last = view.items[2].item.id.clone();
    core.move_in_playlist(&p.id, &last, 0).unwrap();
    assert_eq!(
        titles(core, &p.id),
        ["Романс", "Кукушка", "Звезда по имени Солнце"]
    );
    core.move_in_playlist(&p.id, &last, 99).unwrap();
    assert_eq!(
        titles(core, &p.id),
        ["Кукушка", "Звезда по имени Солнце", "Романс"]
    );
    // Вниз на одну позицию: место считается среди остальных элементов.
    let first = core.playlist(&p.id).unwrap().items[0].item.id.clone();
    core.move_in_playlist(&p.id, &first, 1).unwrap();
    assert_eq!(
        titles(core, &p.id),
        ["Звезда по имени Солнце", "Кукушка", "Романс"]
    );
    core.move_in_playlist(&p.id, &first, 0).unwrap();
    assert!(
        core.move_in_playlist(&p.id, &ItemId("нет".into()), 0)
            .is_err()
    );

    core.remove_from_playlist(&p.id, &[view.items[0].item.id.clone()])
        .unwrap();
    assert_eq!(titles(core, &p.id), ["Звезда по имени Солнце", "Романс"]);

    f.clock.advance(1000);
    let other = core.create_playlist("Кино", None).unwrap();
    core.add_to_playlist(
        &other.id,
        &[TrackId(1), TrackId(2), TrackId(4), TrackId(3)],
        None,
    )
    .unwrap();
    let cards = core.playlists().unwrap();
    assert_eq!(cards[0].playlist.id, other.id, "недавно изменённые первыми");
    assert_eq!(cards[0].items, 4);
    // Мозаика — разные альбомы: «Группа крови» с того же альбома, что «Звезда», не повторяется.
    assert_eq!(cards[0].covers, [TrackId(1), TrackId(2), TrackId(3)]);

    core.update_playlist(&other.id, "Кино и только", Some("лучшее"))
        .unwrap();
    assert_eq!(
        core.playlist(&other.id).unwrap().playlist.title,
        "Кино и только"
    );
    core.delete_playlist(&other.id).unwrap();
    assert_eq!(core.playlists().unwrap().len(), 1);
    assert!(matches!(
        core.playlist(&other.id),
        Err(CoreError::Playlist(_))
    ));
}

#[test]
fn tracks_missing_here_stay_and_are_found_later() {
    let f = fixture(true);
    let core = &f.core;
    let csv = "Track Name,Artist Name(s),Album Name,ISRC\n\
               Кукушка,Кино,Лучшие песни,\n\
               Something,Other Artist,,USSM17800845\n\
               Whatever,Kino,,RUA018700012\n";
    let report = core
        .import_playlist("Из Spotify.csv", csv.as_bytes(), None)
        .unwrap();
    assert_eq!((report.total, report.matched), (3, 2));
    assert_eq!(
        report.playlist.title, "Из Spotify",
        "название — по имени файла"
    );
    assert_eq!(report.playlist.origin.as_deref(), Some("import:csv"));

    let view = core.playlist(&report.playlist.id).unwrap();
    let found: Vec<_> = view.items.iter().map(|i| i.track_id).collect();
    assert_eq!(
        found,
        [Some(TrackId(1)), None, Some(TrackId(2))],
        "по ключу и по ISRC"
    );
    // Найденный трек дополняет снимок из библиотеки, название — как в файле.
    assert_eq!(view.items[2].item.track.title, "Whatever");
    assert_eq!(
        view.items[0].item.track.album.as_deref(),
        Some("Лучшие песни"),
        "что было в файле — остаётся"
    );
    assert_eq!(view.items[2].item.track.album.as_deref(), Some("Звезда"));
    assert_eq!(
        view.items[0].item.location.as_deref(),
        Some("/home/ivan/Музыка/Кино/01 Кукушка.flac")
    );

    // Файл появился в библиотеке — трек находится сам, без повторного импорта.
    let mut new = song(
        5,
        "/home/ivan/Музыка/new.flac",
        "Other Artist",
        "Something",
        "",
    );
    new.isrc = Some("USSM17800845".into());
    f.library.insert(new);
    assert_eq!(
        core.playlist(&report.playlist.id).unwrap().items[1].track_id,
        Some(TrackId(5))
    );
}

#[test]
fn m3u8_import_resolves_relative_paths_and_export_round_trips() {
    let f = fixture(true);
    let core = &f.core;
    let m3u = "#EXTM3U\n#PLAYLIST:Вечер\n#EXTINF:201,Кино - Кукушка\n../Музыка/Кино/01 Кукушка.flac\n#EXTINF:180,Нет - Такого\n../Музыка/нет.flac\n";
    let report = core
        .import_playlist("вечер.m3u8", m3u.as_bytes(), Some("/home/ivan/Плейлисты"))
        .unwrap();
    assert_eq!(report.playlist.title, "Вечер");
    assert_eq!((report.total, report.matched), (2, 1));

    let out = core
        .export_playlist(
            &report.playlist.id,
            Format::M3u,
            Some("/home/ivan/Плейлисты"),
        )
        .unwrap();
    assert!(
        out.text.contains("\n../Музыка/Кино/01 Кукушка.flac\n"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("\n../Музыка/нет.flac\n"),
        "путь из файла сохраняется, даже если файла нет"
    );
    assert_eq!(out.skipped, 0);

    // Импорт из стриминга (без путей), экспорт в M3U8 — путь берётся из библиотеки.
    let txt = core
        .import_playlist(
            "list.txt",
            "Сплин - Романс\nКто-то - Что-то\n".as_bytes(),
            None,
        )
        .unwrap();
    let out = core
        .export_playlist(&txt.playlist.id, Format::M3u, None)
        .unwrap();
    assert!(
        out.text.contains("/home/ivan/Музыка/Сплин/Романс.flac"),
        "{}",
        out.text
    );
    assert_eq!(out.skipped, 1, "без файла здесь — не в M3U8");

    let jspf = core
        .export_playlist(&report.playlist.id, Format::Jspf, None)
        .unwrap();
    let again = core
        .import_playlist("копия.jspf", jspf.text.as_bytes(), None)
        .unwrap();
    assert_eq!((again.total, again.matched), (2, 1));
    assert_eq!(again.playlist.title, "Вечер");
    assert!(core.import_playlist("bad.jspf", b"{oops", None).is_err());
    assert_eq!(
        core.playlists().unwrap().len(),
        3,
        "неразобранный файл не создаёт плейлист"
    );
}

#[test]
fn without_store_playlists_are_unavailable() {
    let f = fixture(false);
    assert_eq!(f.core.playlists(), Err(CoreError::MissingPort("playlists")));
}
