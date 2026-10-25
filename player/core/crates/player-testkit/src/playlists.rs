//! Общие проверки хранилища плейлистов: одни и те же для `MemoryPlaylists`
//! и `SqlitePlaylists`, поэтому поведение хранилищ не расходится.

use std::collections::BTreeMap;

use player_playlists::{ItemId, Playlist, PlaylistId, PlaylistItem, PlaylistStore, TrackSnapshot};

/// Прогнать все проверки; `make` создаёт пустое хранилище.
pub fn conformance<S: PlaylistStore>(make: impl Fn() -> S) {
    lifecycle(&make());
    items(&make());
    snapshot_round_trip(&make());
}

/// Плейлист номер `n`, созданный в `at_ms`.
pub fn playlist(n: u32, at_ms: i64) -> Playlist {
    Playlist {
        id: PlaylistId(format!("00000000-0000-7000-8000-{n:012}")),
        title: format!("Плейлист {n}"),
        description: None,
        origin: None,
        created_at_ms: at_ms,
        updated_at_ms: at_ms,
    }
}

/// Элемент `id` с ключом порядка `position`.
pub fn item(id: &str, position: &str, title: &str) -> PlaylistItem {
    PlaylistItem {
        id: ItemId(id.into()),
        position: position.into(),
        added_at_ms: 1_000,
        location: Some(format!("/music/{title}.flac")),
        track: TrackSnapshot {
            title: title.into(),
            artists: vec!["Кино".into()],
            duration_ms: Some(200_000),
            ..TrackSnapshot::default()
        },
    }
}

fn titles(store: &dyn PlaylistStore, id: &PlaylistId) -> Vec<String> {
    store
        .get(id)
        .unwrap()
        .expect("плейлист есть")
        .items
        .into_iter()
        .map(|i| i.track.title)
        .collect()
}

/// Создание, список, правка, удаление.
pub fn lifecycle(store: &dyn PlaylistStore) {
    let a = playlist(1, 1_000);
    let b = playlist(2, 2_000);
    store.create(&a).unwrap();
    store.create(&b).unwrap();
    assert!(store.create(&a).is_err(), "тот же id второй раз — ошибка");
    let order: Vec<_> = store
        .list()
        .unwrap()
        .into_iter()
        .map(|s| s.playlist.id)
        .collect();
    assert_eq!(
        order,
        [b.id.clone(), a.id.clone()],
        "недавно изменённые первыми"
    );

    store
        .update(&a.id, "Новое имя", Some("описание"), 3_000)
        .unwrap();
    let got = store.get(&a.id).unwrap().unwrap().playlist;
    assert_eq!(
        (got.title.as_str(), got.description.as_deref()),
        ("Новое имя", Some("описание"))
    );
    assert_eq!((got.created_at_ms, got.updated_at_ms), (1_000, 3_000));
    assert_eq!(
        store.list().unwrap()[0].playlist.id,
        a.id,
        "правка поднимает в списке"
    );
    store.update(&a.id, "Новое имя", None, 3_500).unwrap();
    assert_eq!(
        store.get(&a.id).unwrap().unwrap().playlist.description,
        None
    );

    store.delete(&a.id, 4_000).unwrap();
    assert!(store.get(&a.id).unwrap().is_none());
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(
        store.update(&a.id, "x", None, 5_000).is_err(),
        "удалённый не правится"
    );
    assert!(
        store
            .add_items(&a.id, &[item("i", "V", "x")], 5_000)
            .is_err()
    );
    assert!(store.delete(&a.id, 5_000).is_err());
    assert!(store.get(&PlaylistId("нет".into())).unwrap().is_none());
}

/// Элементы: порядок по ключу, перестановка, удаление, сводка.
pub fn items(store: &dyn PlaylistStore) {
    let p = playlist(1, 1_000);
    store.create(&p).unwrap();
    store
        .add_items(
            &p.id,
            &[
                item("a", "V", "Кукушка"),
                item("b", "k", "Звезда"),
                item("c", "e", "Группа крови"),
            ],
            2_000,
        )
        .unwrap();
    // Ключи по ASCII: «V» < «e» < «k».
    assert_eq!(
        titles(store, &p.id),
        ["Кукушка", "Группа крови", "Звезда"],
        "по ключу, не по порядку добавления"
    );
    // Равные ключи (правки с двух устройств) — по id.
    store
        .add_items(&p.id, &[item("aa", "V", "Перемен")], 2_100)
        .unwrap();
    assert_eq!(
        titles(store, &p.id),
        ["Кукушка", "Перемен", "Группа крови", "Звезда"]
    );
    assert!(
        store
            .add_items(&p.id, &[item("a", "z", "дубль")], 2_200)
            .is_err(),
        "тот же id элемента — ошибка"
    );

    store
        .set_position(&p.id, &ItemId("b".into()), "1", 3_000)
        .unwrap();
    assert_eq!(
        titles(store, &p.id),
        ["Звезда", "Кукушка", "Перемен", "Группа крови"]
    );
    assert!(
        store
            .set_position(&p.id, &ItemId("нет".into()), "2", 3_000)
            .is_err()
    );

    store
        .remove_items(&p.id, &[ItemId("c".into()), ItemId("нет".into())], 4_000)
        .unwrap();
    assert_eq!(titles(store, &p.id), ["Звезда", "Кукушка", "Перемен"]);
    assert!(
        store
            .set_position(&p.id, &ItemId("c".into()), "2", 4_100)
            .is_err(),
        "убранный не двигается"
    );

    let summary = store.list().unwrap().remove(0);
    assert_eq!((summary.items, summary.duration_ms), (3, 600_000));
    assert_eq!(
        summary.playlist.updated_at_ms, 4_000,
        "правка элементов — правка плейлиста"
    );
}

/// Снимок трека и путь возвращаются без потерь.
pub fn snapshot_round_trip(store: &dyn PlaylistStore) {
    let p = Playlist {
        origin: Some("import:jspf".into()),
        description: Some("из ListenBrainz".into()),
        ..playlist(7, 1_000)
    };
    store.create(&p).unwrap();
    let mut ext_ids = BTreeMap::new();
    ext_ids.insert("isrc".to_owned(), "RUA018700012".to_owned());
    ext_ids.insert("spotify".to_owned(), "4uLU6hMCjMI75M1A2tKUQC".to_owned());
    let full = PlaylistItem {
        id: ItemId("full".into()),
        position: "V".into(),
        added_at_ms: 1_234,
        location: Some("C:\\Музыка\\Кино\\04 Кукушка.flac".into()),
        track: TrackSnapshot {
            title: "Кукушка".into(),
            artists: vec!["Кино".into(), "Виктор Цой".into()],
            album: Some("Чёрный альбом".into()),
            album_artist: Some("Кино".into()),
            duration_ms: Some(397_000),
            recording_mbid: Some("6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f".into()),
            ext_ids,
        },
    };
    let bare = PlaylistItem {
        id: ItemId("bare".into()),
        position: "k".into(),
        added_at_ms: 1_235,
        location: None,
        track: TrackSnapshot {
            title: "Только название".into(),
            ..TrackSnapshot::default()
        },
    };
    store
        .add_items(&p.id, &[full.clone(), bare.clone()], 2_000)
        .unwrap();
    let got = store.get(&p.id).unwrap().unwrap();
    assert_eq!(got.items, [full, bare]);
    assert_eq!(got.playlist.origin.as_deref(), Some("import:jspf"));
    assert_eq!(got.playlist.description.as_deref(), Some("из ListenBrainz"));
}
