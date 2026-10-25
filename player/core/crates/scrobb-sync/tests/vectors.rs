//! Сверка с языконезависимыми векторами `contracts/match-key/v1.json`:
//! их же проходит бэкенд, поэтому ключ трека у плеера и сервера один.

use scrobb_sync::TrackKey;
use serde::Deserialize;

const V1: &str = include_str!("../../../../../contracts/match-key/v1.json");

#[derive(Deserialize)]
struct Vectors {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    artist: String,
    title: String,
    key: Key,
}

#[derive(Deserialize)]
struct Key {
    artist: String,
    title: String,
}

#[test]
fn match_key_v1() {
    let vectors: Vectors = serde_json::from_str(V1).expect("contracts/match-key/v1.json");
    assert!(vectors.cases.len() >= 18);
    for case in vectors.cases {
        let key = TrackKey::new(&case.artist, &case.title, None);
        assert_eq!(
            (key.artist.as_str(), key.title.as_str()),
            (case.key.artist.as_str(), case.key.title.as_str()),
            "{}",
            case.name
        );
    }
}
