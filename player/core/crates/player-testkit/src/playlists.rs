//! Общие проверки хранилища плейлистов: одни и те же для `MemoryPlaylists`
//! и `SqlitePlaylists`, поэтому поведение хранилищ не расходится.

use std::collections::BTreeMap;

use player_playlists::{
    Edit, Hlc, ItemId, ItemRecord, Playlist, PlaylistId, PlaylistItem, PlaylistRecord,
    PlaylistStore, SyncState, TrackSnapshot,
};
use scrobb_sync::Lww;

/// Прогнать все проверки; `make` создаёт пустое хранилище.
pub fn conformance<S: PlaylistStore>(make: impl Fn() -> S) {
    lifecycle(&make());
    items(&make());
    snapshot_round_trip(&make());
    local_edits_are_unsent(&make());
    merge_from_server(&make());
    sync_bookkeeping(&make());
}

/// Узел часов «этого устройства» в проверках.
pub const NODE: u32 = 7;
/// Узел часов «другого устройства».
pub const OTHER: u32 = 9;

/// Метка часов узла `node` в `at_ms`.
pub fn stamp(at_ms: i64, node: u32) -> Hlc {
    Hlc {
        wall_ms: at_ms as u64,
        counter: 0,
        node,
    }
}

/// Правка этого устройства в `at_ms`.
pub fn edit(at_ms: i64) -> Edit {
    Edit {
        at_ms,
        hlc: stamp(at_ms, NODE),
    }
}

/// Плейлист с другого устройства, как его присылает сервер.
pub fn remote_playlist(n: u32, title: &str, at: Hlc) -> PlaylistRecord {
    PlaylistRecord {
        id: playlist(n, 0).id.0,
        created_at_ms: at.wall_ms as i64,
        origin: None,
        title: Lww {
            value: title.into(),
            stamp: at,
        },
        description: Lww {
            value: None,
            stamp: at,
        },
        deleted: None,
    }
}

/// Элемент с другого устройства.
pub fn remote_item(id: &str, playlist_n: u32, position: &str, at: Hlc) -> ItemRecord {
    let local = item(id, position, id);
    ItemRecord {
        id: id.into(),
        playlist_id: playlist(playlist_n, 0).id.0,
        added_at_ms: at.wall_ms as i64,
        track: local.track,
        position: Lww {
            value: position.into(),
            stamp: at,
        },
        deleted: None,
    }
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
    store.create(&a, edit(1_000)).unwrap();
    store.create(&b, edit(2_000)).unwrap();
    assert!(
        store.create(&a, edit(2_500)).is_err(),
        "тот же id второй раз — ошибка"
    );
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
        .update(&a.id, "Новое имя", Some("описание"), edit(3_000))
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
    store.update(&a.id, "Новое имя", None, edit(3_500)).unwrap();
    assert_eq!(
        store.get(&a.id).unwrap().unwrap().playlist.description,
        None
    );

    store.delete(&a.id, edit(4_000)).unwrap();
    assert!(store.get(&a.id).unwrap().is_none());
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(
        store.update(&a.id, "x", None, edit(5_000)).is_err(),
        "удалённый не правится"
    );
    assert!(
        store
            .add_items(&a.id, &[item("i", "V", "x")], edit(5_000))
            .is_err()
    );
    assert!(store.delete(&a.id, edit(5_000)).is_err());
    assert!(store.get(&PlaylistId("нет".into())).unwrap().is_none());
}

/// Элементы: порядок по ключу, перестановка, удаление, сводка.
pub fn items(store: &dyn PlaylistStore) {
    let p = playlist(1, 1_000);
    store.create(&p, edit(1_000)).unwrap();
    store
        .add_items(
            &p.id,
            &[
                item("a", "V", "Кукушка"),
                item("b", "k", "Звезда"),
                item("c", "e", "Группа крови"),
            ],
            edit(2_000),
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
        .add_items(&p.id, &[item("aa", "V", "Перемен")], edit(2_100))
        .unwrap();
    assert_eq!(
        titles(store, &p.id),
        ["Кукушка", "Перемен", "Группа крови", "Звезда"]
    );
    assert!(
        store
            .add_items(&p.id, &[item("a", "z", "дубль")], edit(2_200))
            .is_err(),
        "тот же id элемента — ошибка"
    );

    store
        .set_position(&p.id, &ItemId("b".into()), "1", edit(3_000))
        .unwrap();
    assert_eq!(
        titles(store, &p.id),
        ["Звезда", "Кукушка", "Перемен", "Группа крови"]
    );
    assert!(
        store
            .set_position(&p.id, &ItemId("нет".into()), "2", edit(3_000))
            .is_err()
    );

    store
        .remove_items(
            &p.id,
            &[ItemId("c".into()), ItemId("нет".into())],
            edit(4_000),
        )
        .unwrap();
    assert_eq!(titles(store, &p.id), ["Звезда", "Кукушка", "Перемен"]);
    assert!(
        store
            .set_position(&p.id, &ItemId("c".into()), "2", edit(4_100))
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
    store.create(&p, edit(1_000)).unwrap();
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
        .add_items(&p.id, &[full.clone(), bare.clone()], edit(2_000))
        .unwrap();
    let got = store.get(&p.id).unwrap().unwrap();
    assert_eq!(got.items, [full, bare]);
    assert_eq!(got.playlist.origin.as_deref(), Some("import:jspf"));
    assert_eq!(got.playlist.description.as_deref(), Some("из ListenBrainz"));
}

fn ids<R>(records: &[(R, u64)], id: impl Fn(&R) -> &str) -> Vec<String> {
    records.iter().map(|(r, _)| id(r).to_owned()).collect()
}

/// Правки здесь — неотправленное, с метками по полям; отправленное и не
/// менявшееся с тех пор — уже нет.
pub fn local_edits_are_unsent(store: &dyn PlaylistStore) {
    let p = playlist(1, 1_000);
    store.create(&p, edit(1_000)).unwrap();
    store
        .add_items(
            &p.id,
            &[item("a", "V", "Кукушка"), item("b", "k", "Звезда")],
            edit(2_000),
        )
        .unwrap();
    let unsent = store.unsent(10).unwrap();
    let (record, _) = &unsent.playlists[0];
    assert_eq!(
        (record.id.as_str(), record.title.value.as_str()),
        (p.id.0.as_str(), "Плейлист 1")
    );
    assert_eq!(record.title.stamp, stamp(1_000, NODE));
    assert_eq!(
        ids(&unsent.items, |r| &r.id),
        ["a", "b"],
        "в порядке правок"
    );
    let (a, _) = &unsent.items[0];
    assert_eq!(
        (a.playlist_id.as_str(), a.track.title.as_str()),
        (p.id.0.as_str(), "Кукушка")
    );
    assert_eq!(
        a.position,
        Lww {
            value: "V".into(),
            stamp: stamp(2_000, NODE)
        }
    );
    let page = store.unsent(1).unwrap();
    assert_eq!(
        (page.playlists.len(), page.items.len()),
        (1, 1),
        "плейлисты — все, элементы — сколько просили"
    );

    // Правка между чтением и отметкой остаётся неотправленной.
    store
        .set_position(&p.id, &ItemId("b".into()), "1", edit(3_000))
        .unwrap();
    store.mark_sent(&unsent).unwrap();
    let left = store.unsent(10).unwrap();
    assert!(left.playlists.is_empty());
    assert_eq!(ids(&left.items, |r| &r.id), ["b"]);
    assert_eq!(left.items[0].0.position.stamp, stamp(3_000, NODE));
    store.mark_sent(&left).unwrap();
    assert!(store.unsent(10).unwrap().is_empty());

    store
        .update(&p.id, "Плейлист 1", Some("описание"), edit(4_000))
        .unwrap();
    let unsent = store.unsent(10).unwrap();
    let (record, _) = &unsent.playlists[0];
    assert_eq!(
        record.title.stamp,
        stamp(1_000, NODE),
        "название не менялось — метка прежняя"
    );
    assert_eq!(record.description.stamp, stamp(4_000, NODE));
    store.mark_sent(&unsent).unwrap();
    store
        .update(&p.id, "Плейлист 1", Some("описание"), edit(5_000))
        .unwrap();
    assert!(
        store.unsent(10).unwrap().is_empty(),
        "сохранение без изменений — не правка"
    );

    store
        .remove_items(&p.id, &[ItemId("a".into())], edit(6_000))
        .unwrap();
    store.delete(&p.id, edit(7_000)).unwrap();
    let unsent = store.unsent(10).unwrap();
    assert_eq!(unsent.playlists[0].0.deleted, Some(stamp(7_000, NODE)));
    assert_eq!(ids(&unsent.items, |r| &r.id), ["a"]);
    assert_eq!(unsent.items[0].0.deleted, Some(stamp(6_000, NODE)));
}

/// Записи с сервера сливаются с местными по правилам `scrobb_sync::playlists`.
pub fn merge_from_server(store: &dyn PlaylistStore) {
    let mine = playlist(1, 1_000);
    store.create(&mine, edit(1_000)).unwrap();
    let theirs = PlaylistRecord {
        origin: Some("import:m3u".into()),
        ..remote_playlist(2, "С телефона", stamp(5_000, OTHER))
    };
    let their_items = vec![
        remote_item("r1", 2, "k", stamp(5_000, OTHER)),
        remote_item("r2", 2, "V", stamp(5_100, OTHER)),
    ];
    let merged = store
        .merge(vec![theirs.clone()], their_items.clone())
        .unwrap();
    assert_eq!(merged.changed, 3);
    assert_eq!(merged.latest, Some(stamp(5_100, OTHER)));
    let id2 = playlist(2, 0).id;
    let got = store.get(&id2).unwrap().expect("пришёл с сервера");
    assert_eq!(got.playlist.title, "С телефона");
    assert_eq!(got.playlist.origin.as_deref(), Some("import:m3u"));
    assert_eq!(
        got.playlist.updated_at_ms, 5_100,
        "изменён тогда, когда правили там"
    );
    assert_eq!(titles(store, &id2), ["r2", "r1"], "по ключу порядка");
    assert!(
        got.items.iter().all(|i| i.location.is_none()),
        "путь не приходит"
    );
    assert_eq!(store.list().unwrap()[0].playlist.id, id2);
    let unsent = store.unsent(10).unwrap();
    assert_eq!(ids(&unsent.playlists, |r| &r.id), [mine.id.0.as_str()]);
    assert!(unsent.items.is_empty(), "пришедшее не отправляется обратно");
    assert_eq!(
        store
            .merge(vec![theirs.clone()], their_items)
            .unwrap()
            .changed,
        0,
        "повтор ничего не меняет"
    );

    // Своя неотправленная правка: старая чужая её не затирает, новая — побеждает.
    store.update(&mine.id, "Моё", None, edit(6_000)).unwrap();
    let same_description = Lww {
        value: None,
        stamp: stamp(1_000, NODE),
    };
    let older = PlaylistRecord {
        created_at_ms: 1_000,
        description: same_description.clone(),
        ..remote_playlist(1, "Старое", stamp(5_500, OTHER))
    };
    assert_eq!(store.merge(vec![older], Vec::new()).unwrap().changed, 0);
    assert_eq!(store.get(&mine.id).unwrap().unwrap().playlist.title, "Моё");
    let newer = PlaylistRecord {
        created_at_ms: 1_000,
        description: same_description,
        ..remote_playlist(1, "С ноутбука", stamp(6_500, OTHER))
    };
    assert_eq!(store.merge(vec![newer], Vec::new()).unwrap().changed, 1);
    assert_eq!(
        store.get(&mine.id).unwrap().unwrap().playlist.title,
        "С ноутбука"
    );
    let unsent = store.unsent(10).unwrap();
    assert_eq!(
        unsent.playlists[0].0.title.value, "С ноутбука",
        "неотправленное уходит уже слитым"
    );

    // Убрано и удалено там.
    let removed = ItemRecord {
        deleted: Some(stamp(7_000, OTHER)),
        ..remote_item("r1", 2, "k", stamp(5_000, OTHER))
    };
    assert_eq!(store.merge(Vec::new(), vec![removed]).unwrap().changed, 1);
    assert_eq!(titles(store, &id2), ["r2"]);
    let deleted = PlaylistRecord {
        deleted: Some(stamp(8_000, OTHER)),
        ..theirs
    };
    store.merge(vec![deleted], Vec::new()).unwrap();
    assert!(store.get(&id2).unwrap().is_none());
    assert_eq!(store.list().unwrap().len(), 1);

    // Элемент без плейлиста — ошибка, и не применяется ничего.
    let third = remote_playlist(3, "Третий", stamp(9_000, OTHER));
    let orphan = remote_item("o", 4, "V", stamp(9_000, OTHER));
    assert!(store.merge(vec![third.clone()], vec![orphan]).is_err());
    assert!(store.get(&playlist(3, 0).id).unwrap().is_none());
    let with_it = remote_item("t", 3, "V", stamp(9_000, OTHER));
    store.merge(vec![third], vec![with_it]).unwrap();
    assert_eq!(titles(store, &playlist(3, 0).id), ["t"]);
}

/// Узел, курсор, самая поздняя метка и сброс синхронизации.
pub fn sync_bookkeeping(store: &dyn PlaylistStore) {
    assert_eq!(store.sync_state().unwrap(), SyncState::default());
    store.set_node(42).unwrap();
    store.set_cursor(Some("17")).unwrap();
    let p = playlist(1, 1_000);
    store.create(&p, edit(1_000)).unwrap();
    store
        .add_items(&p.id, &[item("a", "V", "Кукушка")], edit(2_000))
        .unwrap();
    store
        .merge(
            Vec::new(),
            vec![remote_item("r", 1, "k", stamp(9_000, OTHER))],
        )
        .unwrap();
    assert_eq!(
        store.sync_state().unwrap(),
        SyncState {
            node: Some(42),
            cursor: Some("17".into()),
            latest: Some(stamp(9_000, OTHER)),
        }
    );
    let unsent = store.unsent(10).unwrap();
    store.mark_sent(&unsent).unwrap();
    assert!(store.unsent(10).unwrap().is_empty());

    store.reset_sync().unwrap();
    let state = store.sync_state().unwrap();
    assert_eq!((state.node, state.cursor), (Some(42), None));
    let again = store.unsent(10).unwrap();
    assert_eq!(ids(&again.playlists, |r| &r.id), [p.id.0.as_str()]);
    let mut items = ids(&again.items, |r| &r.id);
    items.sort();
    assert_eq!(
        items,
        ["a", "r"],
        "и своё, и пришедшее — для нового аккаунта"
    );
    store.set_cursor(None).unwrap();
}
