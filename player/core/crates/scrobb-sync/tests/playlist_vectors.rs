//! Сверка с языконезависимыми векторами `contracts/playlist-sync/v1.json`:
//! их же проходит бэкенд, поэтому устройства и сервер сливают правки одинаково.

use scrobb_sync::Hlc;
use scrobb_sync::playlists::{ItemRecord, PlaylistRecord};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const V1: &str = include_str!("../../../../../contracts/playlist-sync/v1.json");

#[derive(Deserialize)]
struct Vectors {
    hlc: Vec<HlcCase>,
    playlists: Vec<Case>,
    items: Vec<Case>,
}

#[derive(Deserialize)]
struct HlcCase {
    text: String,
    wall_ms: u64,
    counter: u32,
    node: u32,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    versions: Vec<Value>,
    merged: Value,
}

fn vectors() -> Vectors {
    serde_json::from_str(V1).expect("contracts/playlist-sync/v1.json")
}

/// Все перестановки индексов `0..n`.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for rest in permutations(n - 1) {
        for at in 0..=rest.len() {
            let mut p = rest.clone();
            p.insert(at, n - 1);
            out.push(p);
        }
    }
    out
}

/// Слить версии в каждом порядке и сравнить с ожидаемым — в JSON, то есть
/// вместе с форматом (`null` и пропущенные поля).
fn check<R: DeserializeOwned + serde::Serialize + Clone>(
    cases: &[Case],
    merge: impl Fn(&mut R, R) -> bool,
) {
    for case in cases {
        let versions: Vec<R> = case
            .versions
            .iter()
            .map(|v| serde_json::from_value(v.clone()).expect(&case.name))
            .collect();
        for order in permutations(versions.len()) {
            let mut merged = versions[order[0]].clone();
            for &i in &order[1..] {
                merge(&mut merged, versions[i].clone());
            }
            assert_eq!(
                serde_json::to_value(&merged).unwrap(),
                case.merged,
                "{}: порядок {order:?}",
                case.name
            );
            for v in &versions {
                assert!(
                    !merge(&mut merged, v.clone()),
                    "{}: повторное слияние что-то изменило",
                    case.name
                );
            }
        }
    }
}

#[test]
fn hlc_text_form_v1() {
    let cases = vectors().hlc;
    for c in &cases {
        let hlc = Hlc {
            wall_ms: c.wall_ms,
            counter: c.counter,
            node: c.node,
        };
        assert_eq!(hlc.to_string(), c.text);
        assert_eq!(c.text.parse::<Hlc>().unwrap(), hlc);
    }
    assert!(cases.windows(2).all(|w| w[0].text < w[1].text));
}

#[test]
fn playlist_merge_v1() {
    let cases = vectors().playlists;
    assert!(cases.len() >= 7);
    check::<PlaylistRecord>(&cases, PlaylistRecord::merge);
}

#[test]
fn item_merge_v1() {
    let cases = vectors().items;
    assert!(cases.len() >= 4);
    check::<ItemRecord<Value>>(&cases, ItemRecord::merge);
}
