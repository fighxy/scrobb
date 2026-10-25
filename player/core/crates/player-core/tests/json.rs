//! Формат JSON, в котором ядро говорит с оболочками без FFI-привязок —
//! окном Tauri и вебом. TS-типы оболочки (`player/shells/desktop/src/core.ts`)
//! повторяют его; этот тест не даёт формату поменяться незаметно.

use player_core::{Command, CoreError, Event, ScrobbleState, ScrobbleStatus};
use player_domain::{
    MediaUri, PlaybackStatus, PlayerState, RepeatMode, ReplayGain, Track, TrackId,
};
use player_history::{Outcome, PlayId, PlayRecord, TrackSnapshot};
use serde_json::{Value, json};

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn commands_are_tagged_objects() {
    let cases = [
        (
            Command::PlayQueue {
                tracks: vec![TrackId(1), TrackId(2)],
                start: 1,
            },
            json!({"type": "play_queue", "data": {"tracks": [1, 2], "start": 1}}),
        ),
        (Command::Pause, json!({"type": "pause"})),
        (
            Command::Seek { position_ms: 1500 },
            json!({"type": "seek", "data": {"position_ms": 1500}}),
        ),
        (
            Command::SetRepeat(RepeatMode::All),
            json!({"type": "set_repeat", "data": "all"}),
        ),
        (
            Command::SetVolume(0.5),
            json!({"type": "set_volume", "data": 0.5}),
        ),
        (
            Command::LinkScrobbAccount {
                device_name: "Ноутбук".into(),
            },
            json!({"type": "link_scrobb_account", "data": {"device_name": "Ноутбук"}}),
        ),
        (
            Command::UnlinkScrobbAccount,
            json!({"type": "unlink_scrobb_account"}),
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(to_json(&command), expected);
        // Оболочка присылает тот же JSON обратно.
        assert_eq!(
            serde_json::from_value::<Command>(expected).unwrap(),
            command
        );
    }
}

#[test]
fn events_and_state_keep_their_shape() {
    let track = Track {
        id: TrackId(7),
        uri: MediaUri("/music/a.flac".into()),
        title: "Кукушка".into(),
        artists: vec!["Кино".into()],
        album: None,
        album_artist: None,
        duration_ms: Some(400_000),
        track_no: Some(4),
        disc_no: None,
        recording_mbid: None,
        isrc: None,
        replay_gain: ReplayGain::default(),
    };
    let state = PlayerState {
        status: PlaybackStatus::Playing,
        current: Some(track),
        position_ms: 1000,
        queue_position: Some((0, 3)),
        repeat: RepeatMode::Off,
        shuffle: false,
        volume: 1.0,
    };
    assert_eq!(
        to_json(&Event::State(Box::new(state))),
        json!({"type": "state", "data": {
            "status": "playing",
            "current": {
                "id": 7, "uri": "/music/a.flac", "title": "Кукушка", "artists": ["Кино"],
                "album": null, "album_artist": null, "duration_ms": 400000,
                "track_no": 4, "disc_no": null, "recording_mbid": null, "isrc": null,
                "replay_gain": {"track_gain_db": null, "track_peak": null,
                                "album_gain_db": null, "album_peak": null}
            },
            "position_ms": 1000, "queue_position": [0, 3],
            "repeat": "off", "shuffle": false, "volume": 1.0
        }})
    );
    assert_eq!(
        to_json(&Event::Scrobbling(ScrobbleStatus {
            pending: 2,
            state: ScrobbleState::NotLinked
        })),
        json!({"type": "scrobbling", "data": {"pending": 2, "state": "not_linked"}})
    );
    assert_eq!(
        to_json(&Event::ScrobbLinkCode {
            user_code: "WDJB-MJHT".into(),
            verification_uri_complete: "https://scrobb.ru/link?code=WDJB-MJHT".into(),
        }),
        json!({"type": "scrobb_link_code", "data": {
            "user_code": "WDJB-MJHT",
            "verification_uri_complete": "https://scrobb.ru/link?code=WDJB-MJHT"
        }})
    );
    assert_eq!(
        to_json(&Event::Error("нет файла".into())),
        json!({"type": "error", "data": "нет файла"})
    );
    assert_eq!(
        to_json(&CoreError::MissingPort("http")),
        json!({"type": "missing_port", "data": "http"})
    );
}

#[test]
fn history_records_serialize_flat() {
    let record = PlayRecord {
        id: PlayId("0192f000-0000-7000-8000-000000000001".into()),
        started_at_ms: 1_760_000_000_000,
        ended_at_ms: Some(1_760_000_200_000),
        played_ms: 200_000,
        outcome: Outcome::Skipped,
        scrobbled: true,
        track: TrackSnapshot {
            title: "Кукушка".into(),
            artists: vec!["Кино".into()],
            ..TrackSnapshot::default()
        },
        track_id: Some(TrackId(7)),
        device_id: "d".into(),
        origin: "player".into(),
    };
    let value = to_json(&record);
    assert_eq!(value["id"], "0192f000-0000-7000-8000-000000000001");
    assert_eq!(value["outcome"], "skipped");
    assert_eq!(value["track"]["artists"], json!(["Кино"]));
    assert_eq!(value["track"]["ext_ids"], json!({}));
    assert_eq!(value["track_id"], 7);
}

#[test]
fn playlists_serialize_for_the_window() {
    use player_core::{ImportReport, PlaylistView, ResolvedItem};
    use player_playlists::formats::Format;
    use player_playlists::{ItemId, Playlist, PlaylistId, PlaylistItem};

    let playlist = Playlist {
        id: PlaylistId("019a1b2c-0000-7000-8000-000000000001".into()),
        title: "Вечер".into(),
        description: None,
        origin: Some("import:m3u".into()),
        created_at_ms: 1,
        updated_at_ms: 2,
    };
    let view = PlaylistView {
        playlist: playlist.clone(),
        items: vec![ResolvedItem {
            item: PlaylistItem {
                id: ItemId("i1".into()),
                position: "V".into(),
                added_at_ms: 3,
                location: None,
                track: TrackSnapshot {
                    title: "Кукушка".into(),
                    artists: vec!["Кино".into()],
                    ..TrackSnapshot::default()
                },
            },
            track_id: None,
        }],
    };
    let playlist_json = json!({
        "id": "019a1b2c-0000-7000-8000-000000000001", "title": "Вечер", "description": null,
        "origin": "import:m3u", "created_at_ms": 1, "updated_at_ms": 2
    });
    assert_eq!(
        to_json(&view),
        json!({
            "playlist": playlist_json,
            "items": [{
                "id": "i1", "position": "V", "added_at_ms": 3, "location": null,
                "track": {"title": "Кукушка", "artists": ["Кино"], "album": null, "album_artist": null,
                          "duration_ms": null, "recording_mbid": null, "ext_ids": {}},
                "track_id": null
            }]
        })
    );
    assert_eq!(
        to_json(&ImportReport {
            playlist,
            total: 3,
            matched: 2
        }),
        json!({"playlist": playlist_json, "total": 3, "matched": 2})
    );
    assert_eq!(to_json(&Format::M3u), json!("m3u"));
    assert_eq!(
        serde_json::from_value::<Format>(json!("jspf")).unwrap(),
        Format::Jspf
    );
    assert_eq!(
        to_json(&CoreError::Playlist("нет плейлиста".into())),
        json!({"type": "playlist", "data": "нет плейлиста"})
    );
}
