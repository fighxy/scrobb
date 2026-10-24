//! Общее для тестов фасада: наблюдатель-самописец, треки, «проигрывание»
//! до нужного состояния.

#![allow(dead_code)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use player_core::{Command, Core, Event, Observer, ScrobbleStatus};
use player_domain::{MediaUri, PlaybackStatus, PlayerState, ReplayGain, Track, TrackId};
use player_engine::{EngineConfig, GainConfig, ReplayGainMode};
use player_testkit::ManualOutput;

/// Наблюдатель, который запоминает события.
#[derive(Default)]
pub struct Recorder(Mutex<Vec<Event>>);

impl Observer for Recorder {
    fn on_event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

impl Recorder {
    /// Сколько событий уже пришло: отметка, с которой ждать новых.
    pub fn mark(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// Было ли после отметки состояние, подходящее под `pred`.
    pub fn seen_since(&self, mark: usize, pred: &impl Fn(&PlayerState) -> bool) -> bool {
        self.0.lock().unwrap()[mark..]
            .iter()
            .any(|e| matches!(e, Event::State(s) if pred(s)))
    }

    pub fn errors(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Error(message) => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn states(&self) -> Vec<PlayerState> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::State(state) => Some((**state).clone()),
                _ => None,
            })
            .collect()
    }
}

pub fn track(id: u64, file: &str) -> Track {
    Track {
        id: TrackId(id),
        uri: MediaUri(file.into()),
        title: file.into(),
        artists: vec!["Тест".into()],
        album: None,
        album_artist: None,
        duration_ms: None,
        track_no: None,
        disc_no: None,
        recording_mbid: None,
        replay_gain: ReplayGain::default(),
    }
}

pub fn play(ids: &[u64], start: usize) -> Command {
    Command::PlayQueue {
        tracks: ids.iter().map(|&id| TrackId(id)).collect(),
        start,
    }
}

/// «Играть» — забирать звук, как устройство, — пока состояние не станет нужным.
#[track_caller]
pub fn play_until(
    core: &Core,
    output: &ManualOutput,
    pred: impl Fn(&PlayerState) -> bool,
) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut played = Vec::new();
    loop {
        if pred(&core.state()) {
            return played;
        }
        assert!(
            Instant::now() < deadline,
            "состояние не наступило: {:?}",
            core.state()
        );
        let chunk = output.pull(1024);
        if chunk.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
        }
        played.extend(chunk);
    }
}

/// «Играть», пока после отметки `mark` не придёт подходящее состояние. Смотрим
/// историю, а не текущее состояние: короткий трек может доиграть раньше, чем
/// тест успеет его увидеть. Звук берётся по куску, с паузой на обработку событий.
#[track_caller]
pub fn play_until_seen(
    output: &ManualOutput,
    recorder: &Recorder,
    mark: usize,
    pred: impl Fn(&PlayerState) -> bool,
) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut played = Vec::new();
    while !recorder.seen_since(mark, &pred) {
        assert!(
            Instant::now() < deadline,
            "состояние не наступило; последнее: {:?}",
            recorder.states().last().map(|s| (
                s.status,
                s.current.as_ref().map(|t| t.id),
                s.queue_position
            ))
        );
        played.extend(output.pull(1024));
        // Дать потоку событий ядра догнать звук: тест «играет» быстрее реального
        // времени, и без паузы успел бы прокрутить трек раньше, чем о нём узнает ядро.
        let catch_up = Instant::now() + Duration::from_millis(20);
        while Instant::now() < catch_up && !recorder.seen_since(mark, &pred) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    played
}

pub fn playing(id: u64) -> impl Fn(&PlayerState) -> bool {
    move |s| s.status == PlaybackStatus::Playing && s.current.as_ref().is_some_and(|t| t.id.0 == id)
}

pub fn stopped(s: &PlayerState) -> bool {
    s.status == PlaybackStatus::Stopped
}

impl Recorder {
    /// Все события по порядку.
    pub fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }

    /// Все состояния отправки в Scrobb по порядку.
    pub fn scrobbling(&self) -> Vec<ScrobbleStatus> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Scrobbling(status) => Some(*status),
                _ => None,
            })
            .collect()
    }
}

/// Движок для тестов: без ReplayGain, с коротким буфером.
pub fn engine_config() -> EngineConfig {
    EngineConfig {
        gain: GainConfig {
            mode: ReplayGainMode::Off,
            ..GainConfig::default()
        },
        buffer_ms: 500,
        ..EngineConfig::default()
    }
}

/// Ждать, пока условие не выполнится (события приходят из потоков ядра).
#[track_caller]
pub fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "не дождались: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Запустить очередь и «играть», пока не зазвучит трек `first`.
pub fn start(core: &Core, output: &ManualOutput, recorder: &Recorder, ids: &[u64], first: u64) {
    let mark = recorder.mark();
    core.dispatch(play(ids, 0)).unwrap();
    play_until_seen(output, recorder, mark, playing(first));
}
