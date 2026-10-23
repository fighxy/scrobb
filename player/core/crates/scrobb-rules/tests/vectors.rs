//! Сверка с языконезависимыми векторами `contracts/scrobble-rules/v1.json`.

use scrobb_rules::{ListenTracker, Thresholds, required_play_ms};
use serde::Deserialize;

const V1: &str = include_str!("../../../../../contracts/scrobble-rules/v1.json");

#[derive(Deserialize)]
struct Vectors {
    thresholds: Thresholds,
    required_play_ms: Vec<RequiredCase>,
    sessions: Vec<Session>,
}

#[derive(Deserialize)]
struct RequiredCase {
    name: String,
    duration_ms: Option<u64>,
    expected: Option<u64>,
}

#[derive(Deserialize)]
struct Session {
    name: String,
    duration_ms: Option<u64>,
    events: Vec<Event>,
    scrobbles: Vec<Expected>,
}

#[derive(Deserialize)]
struct Event {
    at_ms: u64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Expected {
    at_ms: u64,
    played_ms: u64,
}

fn vectors() -> Vectors {
    serde_json::from_str(V1).expect("contracts/scrobble-rules/v1.json")
}

#[test]
fn required_play_ms_matches_contract() {
    let v = vectors();
    for case in &v.required_play_ms {
        assert_eq!(
            required_play_ms(case.duration_ms, &v.thresholds),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn sessions_match_contract() {
    let v = vectors();
    for s in &v.sessions {
        let mut tracker: Option<ListenTracker> = None;
        let mut got = Vec::new();
        for e in &s.events {
            match (e.kind.as_str(), tracker.as_mut()) {
                ("start", _) => {
                    tracker = Some(ListenTracker::start(
                        s.duration_ms,
                        0,
                        e.at_ms,
                        &v.thresholds,
                    ))
                }
                ("pause", Some(t)) => t.pause(e.at_ms),
                ("resume", Some(t)) => t.resume(e.at_ms),
                ("stop", Some(t)) => t.stop(e.at_ms),
                ("seek" | "tick", Some(_)) => {}
                (kind, _) => panic!("{}: неизвестное событие или нет start: {kind}", s.name),
            }
            if let Some(hit) = tracker.as_mut().and_then(|t| t.poll(e.at_ms)) {
                got.push(Expected {
                    at_ms: e.at_ms,
                    played_ms: hit.played_ms,
                });
            }
        }
        assert_eq!(got, s.scrobbles, "{}", s.name);
    }
}
