//! Общие проверки хранилища истории: одни и те же для `MemoryHistory`
//! и `SqliteHistory`, поэтому поведение хранилищ не расходится.

use std::collections::BTreeMap;

use player_domain::TrackId;
use player_history::{
    DeliveryState, HistoryStore, NewPlay, ORIGIN_PLAYER, Outcome, PlayId, PlayRecord,
    QUEUE_MAX_AGE_MS, QUEUE_MAX_ITEMS, Settlement, TARGET_SCROBB, TrackSnapshot,
};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const LB: &str = "listenbrainz";

/// Прогнать все проверки; `make` создаёт пустое хранилище.
pub fn conformance<S: HistoryStore>(make: impl Fn() -> S) {
    lifecycle(&make());
    delivery(&make());
    expiry(&make());
    recovery(&make());
    recent_pages(&make());
}

/// Идентификатор в виде UUID с номером `n`.
pub fn play_id(n: u32) -> PlayId {
    PlayId(format!("00000000-0000-7000-8000-{n:012}"))
}

/// Начало прослушивания номер `n` в момент `started_at_ms`.
pub fn new_play(n: u32, started_at_ms: i64) -> NewPlay {
    NewPlay {
        id: play_id(n),
        started_at_ms,
        track: TrackSnapshot {
            title: format!("Трек {n}"),
            artists: vec!["Кино".into()],
            ..TrackSnapshot::default()
        },
        track_id: Some(TrackId(u64::from(n))),
    }
}

fn get(store: &dyn HistoryStore, id: &PlayId) -> PlayRecord {
    store
        .recent(None, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|p| &p.id == id)
        .expect("запись есть")
}

fn ids(plays: &[PlayRecord]) -> Vec<PlayId> {
    plays.iter().map(|p| p.id.clone()).collect()
}

fn begin_scrobbled(store: &dyn HistoryStore, n: u32, at: i64, targets: &[&str]) {
    store.begin(&new_play(n, at)).unwrap();
    store.mark_scrobbled(&play_id(n), 1, targets, at).unwrap();
}

/// Запись от начала до конца; снимок метаданных сохраняется без потерь.
pub fn lifecycle(store: &dyn HistoryStore) {
    let mut play = new_play(1, 1_760_000_000_123);
    play.track = TrackSnapshot {
        title: "Ёлка «в лесу»".into(),
        artists: vec!["Кино".into(), "Виктор Цой".into()],
        album: Some("Чёрный альбом".into()),
        album_artist: Some("Кино".into()),
        duration_ms: Some(400_000),
        recording_mbid: Some("6f1c2f5b-0000-0000-0000-000000000000".into()),
        ext_ids: BTreeMap::from([("isrc".into(), "RUA000000001".into())]),
    };
    store.begin(&play).unwrap();
    let id = &play.id;

    let p = get(store, id);
    assert_eq!(p.outcome, Outcome::Playing);
    assert_eq!((p.played_ms, p.scrobbled, p.ended_at_ms), (0, false, None));
    assert_eq!(p.track, play.track);
    assert_eq!(p.track_id, Some(TrackId(1)));
    assert_eq!(p.started_at_ms, play.started_at_ms);
    assert_eq!(p.origin, ORIGIN_PLAYER);
    assert!(!p.device_id.is_empty());
    assert!(store.begin(&play).is_err(), "id уникален");

    store.checkpoint(id, 15_000).unwrap();
    assert_eq!(get(store, id).played_ms, 15_000);

    store
        .mark_scrobbled(id, 200_000, &[TARGET_SCROBB], 1_760_000_200_000)
        .unwrap();
    let p = get(store, id);
    assert!(p.scrobbled);
    assert_eq!(p.played_ms, 200_000);
    assert_eq!(store.pending_count(TARGET_SCROBB).unwrap(), 1);

    store
        .finish(id, 1_760_000_300_000, 250_000, Outcome::Skipped)
        .unwrap();
    let p = get(store, id);
    assert_eq!(p.outcome, Outcome::Skipped);
    assert_eq!(
        (p.played_ms, p.ended_at_ms),
        (250_000, Some(1_760_000_300_000))
    );

    assert!(store.checkpoint(&play_id(99), 1).is_err());
    assert!(store.finish(&play_id(99), 1, 1, Outcome::Stopped).is_err());
}

/// Очередь по сервисам: порядок, поштучные итоги, повторная постановка не сбрасывает итог.
pub fn delivery(store: &dyn HistoryStore) {
    // Начаты не по порядку номеров: очередь идёт по времени начала.
    begin_scrobbled(store, 3, 3_000, &[TARGET_SCROBB, LB]);
    begin_scrobbled(store, 1, 1_000, &[TARGET_SCROBB, LB]);
    begin_scrobbled(store, 2, 2_000, &[TARGET_SCROBB, LB]);
    store.begin(&new_play(4, 4_000)).unwrap(); // не засчитано

    let first = store.pending(TARGET_SCROBB, 2).unwrap();
    assert_eq!(ids(&first), [play_id(1), play_id(2)]);
    assert!(first.iter().all(|p| p.scrobbled));

    store
        .settle(
            TARGET_SCROBB,
            &[
                Settlement {
                    id: play_id(1),
                    state: DeliveryState::Accepted,
                    reason: None,
                },
                Settlement {
                    id: play_id(2),
                    state: DeliveryState::Rejected,
                    reason: Some("below_threshold".into()),
                },
                Settlement {
                    id: play_id(77),
                    state: DeliveryState::Accepted,
                    reason: None,
                },
            ],
            5_000,
        )
        .unwrap();
    assert_eq!(
        ids(&store.pending(TARGET_SCROBB, 50).unwrap()),
        [play_id(3)]
    );
    assert_eq!(
        store.pending_count(LB).unwrap(),
        3,
        "другой сервис не задет"
    );

    // Повторная отметка не возвращает доставленное в очередь.
    store
        .mark_scrobbled(&play_id(1), 2, &[TARGET_SCROBB], 6_000)
        .unwrap();
    assert_eq!(store.pending_count(TARGET_SCROBB).unwrap(), 1);
    assert_eq!(store.pending_count("nowhere").unwrap(), 0);
}

/// Лимиты очереди: по возрасту и по числу записей.
pub fn expiry(store: &dyn HistoryStore) {
    let now = 30 * DAY_MS;
    begin_scrobbled(store, 0, now - QUEUE_MAX_AGE_MS - 1, &[TARGET_SCROBB]);
    begin_scrobbled(store, 1, now - QUEUE_MAX_AGE_MS + 1, &[TARGET_SCROBB]);
    assert_eq!(store.expire(TARGET_SCROBB, now).unwrap(), 1);
    assert_eq!(
        ids(&store.pending(TARGET_SCROBB, 50).unwrap()),
        [play_id(1)]
    );

    // Ещё QUEUE_MAX_ITEMS свежих: самая старая из ждущих (№ 1) вытесняется.
    for n in 0..QUEUE_MAX_ITEMS as u32 {
        begin_scrobbled(store, 10 + n, now - DAY_MS + i64::from(n), &[TARGET_SCROBB]);
    }
    assert_eq!(store.expire(TARGET_SCROBB, now).unwrap(), 1);
    assert_eq!(store.pending_count(TARGET_SCROBB).unwrap(), QUEUE_MAX_ITEMS);
    assert_eq!(store.pending(TARGET_SCROBB, 1).unwrap()[0].id, play_id(10));
    assert_eq!(store.expire(TARGET_SCROBB, now).unwrap(), 0);
}

/// Записи, не закрытые из-за падения, закрываются при следующем запуске.
pub fn recovery(store: &dyn HistoryStore) {
    store.begin(&new_play(1, 10_000)).unwrap();
    store.checkpoint(&play_id(1), 5_000).unwrap();
    store.begin(&new_play(2, 20_000)).unwrap();
    store
        .finish(&play_id(2), 21_000, 1_000, Outcome::Finished)
        .unwrap();

    assert_eq!(store.recover_interrupted().unwrap(), 1);
    let p = get(store, &play_id(1));
    assert_eq!(p.outcome, Outcome::Interrupted);
    assert_eq!((p.played_ms, p.ended_at_ms), (5_000, Some(15_000)));
    assert_eq!(get(store, &play_id(2)).outcome, Outcome::Finished);
    assert_eq!(store.recover_interrupted().unwrap(), 0);
}

/// Лента идёт страницами от новых к старым.
pub fn recent_pages(store: &dyn HistoryStore) {
    for n in 1..=5 {
        store.begin(&new_play(n, i64::from(n) * 1_000)).unwrap();
    }
    let page = store.recent(None, 2).unwrap();
    assert_eq!(ids(&page), [play_id(5), play_id(4)]);
    let page = store.recent(Some(page[1].started_at_ms), 2).unwrap();
    assert_eq!(ids(&page), [play_id(3), play_id(2)]);
    assert!(store.recent(Some(1_000), 10).unwrap().is_empty());
}

#[cfg(test)]
mod tests {
    use player_history::MemoryHistory;

    #[test]
    fn memory_history_passes_conformance() {
        super::conformance(MemoryHistory::default);
    }
}
