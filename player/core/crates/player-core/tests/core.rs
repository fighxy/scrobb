//! Фасад целиком: команда оболочки → очередь → библиотека → движок → звук,
//! и обратно — события движка → состояние → наблюдатель.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_core::{Command, Core, CoreConfig, CoreError, Event, Observer, Ports};
use player_domain::{
    MediaUri, PlaybackStatus, PlayerState, RepeatMode, ReplayGain, Track, TrackId,
};
use player_engine::{EngineConfig, GainConfig, ReplayGainMode};
use player_library::MemoryLibrary;
use player_ports::Clock;
use player_testkit::{FULL_FRAMES, Fixtures, ManualOutput, read_wav};

struct FixedClock;

impl Clock for FixedClock {
    fn monotonic_ms(&self) -> u64 {
        0
    }

    fn unix_ms(&self) -> i64 {
        0
    }
}

/// Наблюдатель, который запоминает события.
#[derive(Default)]
struct Recorder(Mutex<Vec<Event>>);

impl Observer for Recorder {
    fn on_event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

impl Recorder {
    /// Сколько событий уже пришло: отметка, с которой ждать новых.
    fn mark(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// Было ли после отметки состояние, подходящее под `pred`.
    fn seen_since(&self, mark: usize, pred: &impl Fn(&PlayerState) -> bool) -> bool {
        self.0.lock().unwrap()[mark..]
            .iter()
            .any(|e| matches!(e, Event::State(s) if pred(s)))
    }

    fn errors(&self) -> Vec<String> {
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

    fn states(&self) -> Vec<PlayerState> {
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

fn track(id: u64, file: &str) -> Track {
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

fn core(output: &ManualOutput, tracks: &[(u64, &str)]) -> (Core, Arc<Recorder>) {
    let library = MemoryLibrary::new();
    for &(id, file) in tracks {
        library.insert(track(id, file));
    }
    let recorder = Arc::new(Recorder::default());
    let ports = Ports {
        clock: Arc::new(FixedClock),
        audio: Box::new(output.clone()),
        files: Fixtures::shared(),
        http: None,
        secrets: None,
        media_session: None,
    };
    let config = CoreConfig {
        engine: EngineConfig {
            gain: GainConfig {
                mode: ReplayGainMode::Off,
                ..GainConfig::default()
            },
            buffer_ms: 500,
            ..EngineConfig::default()
        },
    };
    let core = Core::new(ports, Arc::new(library), config, recorder.clone());
    (core, recorder)
}

fn play(ids: &[u64], start: usize) -> Command {
    Command::PlayQueue {
        tracks: ids.iter().map(|&id| TrackId(id)).collect(),
        start,
    }
}

/// «Играть» — забирать звук, как устройство, — пока состояние не станет нужным.
#[track_caller]
fn play_until(core: &Core, output: &ManualOutput, pred: impl Fn(&PlayerState) -> bool) -> Vec<f32> {
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
fn play_until_seen(
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

fn playing(id: u64) -> impl Fn(&PlayerState) -> bool {
    move |s| s.status == PlaybackStatus::Playing && s.current.as_ref().is_some_and(|t| t.id.0 == id)
}

fn stopped(s: &PlayerState) -> bool {
    s.status == PlaybackStatus::Stopped
}

#[test]
fn plays_queue_without_gaps() {
    let output = ManualOutput::default();
    let (core, recorder) = core(&output, &[(1, "a.flac"), (2, "b.flac")]);
    core.dispatch(play(&[1, 2], 0)).unwrap();
    let played = play_until(&core, &output, stopped);
    // Два куска одного сигнала склеились в исходник сэмпл в сэмпл.
    assert!(played == read_wav("full.wav"));
    let second = recorder
        .states()
        .into_iter()
        .find(|s| playing(2)(s))
        .expect("второй трек зазвучал");
    assert_eq!(second.queue_position, Some((1, 2)));
}

#[test]
fn next_previous_stop_and_resume() {
    let output = ManualOutput::default();
    let files = [(1, "full.flac"), (2, "full.flac"), (3, "full.flac")];
    let (core, recorder) = core(&output, &files);
    let mark = recorder.mark();
    core.dispatch(play(&[1, 2, 3], 0)).unwrap();
    play_until_seen(&output, &recorder, mark, playing(1));
    let mark = recorder.mark();
    core.dispatch(Command::Next).unwrap();
    play_until_seen(&output, &recorder, mark, playing(2));
    // В первые 3 секунды «назад» — предыдущий трек.
    let mark = recorder.mark();
    core.dispatch(Command::Previous).unwrap();
    play_until_seen(&output, &recorder, mark, playing(1));
    core.dispatch(Command::Stop).unwrap();
    assert_eq!(core.state().status, PlaybackStatus::Stopped);
    // Стоп асинхронный: звук, уже ушедший в вывод, доигрывает, дальше — тишина.
    play_until(&core, &output, |_| output.pull(512).is_empty());
    std::thread::sleep(Duration::from_millis(30));
    assert!(output.pull(512).is_empty(), "после остановки — тишина");
    // «Играть» после остановки — текущий трек очереди с начала.
    let mark = recorder.mark();
    core.dispatch(Command::Resume).unwrap();
    play_until_seen(&output, &recorder, mark, |s| {
        playing(1)(s) && s.queue_position == Some((0, 3))
    });
}

#[test]
fn next_at_end_of_queue_stops_on_last_track() {
    let output = ManualOutput::default();
    let (core, recorder) = core(&output, &[(1, "full.flac")]);
    let mark = recorder.mark();
    core.dispatch(play(&[1], 0)).unwrap();
    play_until_seen(&output, &recorder, mark, playing(1));
    core.dispatch(Command::Next).unwrap();
    let state = core.state();
    assert_eq!(state.status, PlaybackStatus::Stopped);
    assert_eq!(state.current.map(|t| t.id), Some(TrackId(1)));
}

#[test]
fn pause_resume_and_seek_update_state() {
    let output = ManualOutput::default();
    let (core, recorder) = core(&output, &[(1, "full.flac")]);
    let mark = recorder.mark();
    core.dispatch(play(&[1], 0)).unwrap();
    play_until_seen(&output, &recorder, mark, playing(1));
    core.dispatch(Command::Pause).unwrap();
    assert_eq!(core.state().status, PlaybackStatus::Paused);
    play_until(&core, &output, |_| output.pull(512).is_empty());
    core.dispatch(Command::Resume).unwrap();
    assert_eq!(core.state().status, PlaybackStatus::Playing);
    let mark = recorder.mark();
    core.dispatch(Command::Seek { position_ms: 250 }).unwrap();
    play_until_seen(&output, &recorder, mark, |s| s.position_ms >= 250);
    core.dispatch(Command::SetVolume(0.3)).unwrap();
    assert_eq!(core.state().volume, 0.3);
}

#[test]
fn repeat_one_loops_the_track() {
    let output = ManualOutput::default();
    let (core, _) = core(&output, &[(1, "a.flac")]);
    core.dispatch(Command::SetRepeat(RepeatMode::One)).unwrap();
    core.dispatch(play(&[1], 0)).unwrap();
    // a.flac — 11 113 кадров; три круга без остановки.
    let looped = output.collect(11_113 * 2 * 3);
    let a = &read_wav("full.wav")[..11_113 * 2];
    assert!(looped.chunks(11_113 * 2).all(|lap| lap == a));
    let state = core.state();
    assert_eq!(state.status, PlaybackStatus::Playing);
    assert_eq!(state.queue_position, Some((0, 1)));
}

#[test]
fn broken_and_unknown_tracks_are_skipped_without_gap() {
    let output = ManualOutput::default();
    // 4 — нет в библиотеке, 2 — есть, но файла нет.
    let (core, recorder) = core(
        &output,
        &[(1, "a.flac"), (2, "missing.flac"), (3, "b.flac")],
    );
    core.dispatch(play(&[4, 1, 2, 3], 0)).unwrap();
    let played = play_until(&core, &output, stopped);
    // Сломанный трек между a и b перешагнут без паузы: склейка — исходный сигнал.
    assert!(played == read_wav("full.wav"));
    assert_eq!(recorder.errors().len(), 2, "{:?}", recorder.errors());
    let last = recorder
        .states()
        .into_iter()
        .rfind(|s| s.current.is_some())
        .unwrap();
    assert_eq!(last.current.map(|t| t.id), Some(TrackId(3)));
    assert_eq!(last.queue_position, Some((3, 4)));
}

#[test]
fn invalid_queue_is_rejected() {
    let output = ManualOutput::default();
    let (core, _) = core(&output, &[]);
    assert_eq!(core.dispatch(play(&[], 0)), Err(CoreError::InvalidQueue));
    assert_eq!(core.dispatch(play(&[1], 1)), Err(CoreError::InvalidQueue));
}

#[test]
fn scrobbling_needs_network_and_secrets() {
    let output = ManualOutput::default();
    let (core, _) = core(&output, &[]);
    assert!(!core.capabilities().scrobbling);
    assert_eq!(
        core.dispatch(Command::LinkScrobbAccount {
            device_name: "Pixel".into()
        }),
        Err(CoreError::MissingPort("http"))
    );
}

/// Наблюдатель, который из колбэка сразу шлёт следующую команду.
struct Reentrant(Mutex<Option<Arc<Core>>>);

impl Observer for Reentrant {
    fn on_event(&self, _: Event) {
        let core = self.0.lock().unwrap().take();
        if let Some(core) = core {
            core.dispatch(Command::Next).unwrap();
        }
    }
}

#[test]
fn observer_may_dispatch_from_callback() {
    let output = ManualOutput::default();
    let library = MemoryLibrary::new();
    library.insert(track(1, "a.flac"));
    library.insert(track(2, "b.flac"));
    let observer = Arc::new(Reentrant(Mutex::new(None)));
    let ports = Ports {
        clock: Arc::new(FixedClock),
        audio: Box::new(output.clone()),
        files: Fixtures::shared(),
        http: None,
        secrets: None,
        media_session: None,
    };
    let core = Arc::new(Core::new(
        ports,
        Arc::new(library),
        CoreConfig::default(),
        observer.clone(),
    ));
    *observer.0.lock().unwrap() = Some(core.clone());
    core.dispatch(play(&[1, 2], 0)).unwrap();
    assert_eq!(core.state().queue_position, Some((1, 2)));
}

/// Наблюдатель-шлагбаум: пока закрыт, задерживает поток событий ядра — так
/// события движка копятся и приходят с опозданием, как под нагрузкой.
#[derive(Default)]
struct Gate {
    closed: Mutex<bool>,
    opened: std::sync::Condvar,
    recorder: Recorder,
}

impl Observer for Gate {
    fn on_event(&self, event: Event) {
        if std::thread::current().name() == Some("player-core-events") {
            let mut closed = self.closed.lock().unwrap();
            while *closed {
                closed = self.opened.wait(closed).unwrap();
            }
        }
        self.recorder.on_event(event);
    }
}

#[test]
fn late_events_of_cancelled_playback_are_ignored() {
    let output = ManualOutput::default();
    let library = MemoryLibrary::new();
    library.insert(track(1, "full.flac"));
    library.insert(track(2, "full.flac"));
    let gate = Arc::new(Gate::default());
    *gate.closed.lock().unwrap() = true;
    let ports = Ports {
        clock: Arc::new(FixedClock),
        audio: Box::new(output.clone()),
        files: Fixtures::shared(),
        http: None,
        secrets: None,
        media_session: None,
    };
    let core = Core::new(
        ports,
        Arc::new(library),
        CoreConfig::default(),
        gate.clone(),
    );
    core.dispatch(play(&[1, 2], 0)).unwrap();
    // Доиграть первый трек и начало второго: движок сообщит «трек 2 начался»,
    // но поток событий ядра стоит у шлагбаума.
    output.collect((FULL_FRAMES + 2_048) * 2);
    std::thread::sleep(Duration::from_millis(50));
    // Пользователь жмёт «назад», пока ядро ещё не узнало о треке 2.
    let mark = gate.recorder.mark();
    core.dispatch(Command::Previous).unwrap();
    *gate.closed.lock().unwrap() = false;
    gate.opened.notify_all();
    play_until_seen(&output, &gate.recorder, mark, playing(1));
    std::thread::sleep(Duration::from_millis(50));
    // Запоздавшее «трек 2 начался» из отменённого воспроизведения не сдвинуло очередь.
    let state = core.state();
    assert_eq!(state.current.map(|t| t.id), Some(TrackId(1)));
    assert_eq!(state.queue_position, Some((0, 2)));
}
