//! Движок целиком: декодирование файлов из `fixtures/`, кольцевой буфер и
//! аудиопоток. Вместо звуковой карты — [`ManualOutput`]: тест сам забирает
//! сэмплы, как это делал бы колбэк устройства.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_domain::{MediaUri, ReplayGain, TrackId};
use player_engine::{
    Engine, EngineConfig, EngineEvent, EngineTrack, GainConfig, OutputRate, ReplayGainMode,
};
use player_ports::{
    AudioOutput, FileAccess, FileEntry, MediaStream, PcmFormat, PcmSource, PortError,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
/// Кадров в `full.wav`: 0,6 с при 44,1 кГц.
const FULL_FRAMES: usize = 26_460;

/// Файлы из `fixtures/` в памяти.
struct Fixtures(HashMap<String, Vec<u8>>);

impl Fixtures {
    fn load() -> Arc<Self> {
        let files = [
            "a.flac",
            "b.flac",
            "a.mp3",
            "b.mp3",
            "full.flac",
            "c48k.flac",
            "mono8k.flac",
        ]
        .into_iter()
        .map(|name| {
            let bytes = std::fs::read(format!("{FIXTURES}/{name}")).expect(name);
            (name.to_owned(), bytes)
        })
        .collect();
        Arc::new(Self(files))
    }
}

impl FileAccess for Fixtures {
    fn list(&self, _: &MediaUri) -> Result<Vec<FileEntry>, PortError> {
        Err(PortError::Unsupported)
    }

    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError> {
        let bytes = self
            .0
            .get(&uri.0)
            .ok_or(PortError::Other(format!("нет файла {}", uri.0)))?;
        Ok(Box::new(Cursor::new(bytes.clone())))
    }
}

/// Вывод, из которого тест сам тянет сэмплы.
#[derive(Clone, Default)]
struct ManualOutput {
    source: Arc<Mutex<Option<Box<dyn PcmSource>>>>,
    formats: Arc<Mutex<Vec<PcmFormat>>>,
    /// Устройство умеет только эту частоту (`None` — любую).
    only_rate: Option<u32>,
}

impl AudioOutput for ManualOutput {
    fn sample_rate_for(&mut self, wanted: PcmFormat) -> u32 {
        self.only_rate.unwrap_or(wanted.sample_rate)
    }

    fn open(&mut self, format: PcmFormat, source: Box<dyn PcmSource>) -> Result<(), PortError> {
        self.formats.lock().unwrap().push(format);
        *self.source.lock().unwrap() = Some(source);
        Ok(())
    }

    fn start(&mut self) -> Result<(), PortError> {
        Ok(())
    }

    fn stop(&mut self) {}
}

impl ManualOutput {
    /// Один «колбэк устройства» на `samples` сэмплов: вернуть то, что пришло из потока.
    fn pull(&self, samples: usize) -> Vec<f32> {
        let mut out = vec![0.0; samples];
        let taken = match self.source.lock().unwrap().as_mut() {
            Some(source) => source.fill(&mut out),
            None => 0,
        };
        out.truncate(taken);
        out
    }

    /// Тянуть, пока не наберётся `samples` сэмплов (данные приходят из рабочего потока).
    fn collect(&self, samples: usize) -> Vec<f32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::with_capacity(samples);
        while got.len() < samples {
            assert!(
                Instant::now() < deadline,
                "собрано {} из {samples}",
                got.len()
            );
            let chunk = self.pull((samples - got.len()).min(1024));
            if chunk.is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
            got.extend(chunk);
        }
        got
    }
}

fn track(id: u64, file: &str) -> EngineTrack {
    EngineTrack {
        id: TrackId(id),
        uri: MediaUri(file.into()),
        replay_gain: ReplayGain::default(),
    }
}

fn engine(output: &ManualOutput) -> (Engine, Receiver<EngineEvent>) {
    engine_with(output, OutputRate::FollowTrack)
}

fn engine_with(output: &ManualOutput, output_rate: OutputRate) -> (Engine, Receiver<EngineEvent>) {
    let (tx, rx) = mpsc::channel();
    let config = EngineConfig {
        gain: GainConfig {
            mode: ReplayGainMode::Off,
            ..GainConfig::default()
        },
        buffer_ms: 500,
        output_rate,
    };
    (
        Engine::new(Box::new(output.clone()), Fixtures::load(), config, tx),
        rx,
    )
}

/// Устройство, которое умеет только 48 кГц.
fn output_48k() -> ManualOutput {
    ManualOutput {
        only_rate: Some(48_000),
        ..ManualOutput::default()
    }
}

/// Исходный сигнал фикстур в момент `t`: 440 Гц слева, 660 Гц справа.
fn signal(t: f64) -> [f32; 2] {
    let s = |f: f64| (0.5 * (2.0 * std::f64::consts::PI * f * t).sin()) as f32;
    [s(440.0), s(660.0)]
}

/// Наибольшее отклонение кадров `played[from..to]` от сигнала на 48 кГц, начиная с `t0`.
fn max_error_48k(played: &[f32], t0: f64, from: usize, to: usize) -> f32 {
    (from..to)
        .map(|i| {
            let [l, r] = signal(t0 + i as f64 / 48_000.0);
            (played[i * 2] - l).abs().max((played[i * 2 + 1] - r).abs())
        })
        .fold(0.0, f32::max)
}

fn wait_event(rx: &Receiver<EngineEvent>, pred: impl Fn(&EngineEvent) -> bool) -> EngineEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("событие не пришло");
        if pred(&event) {
            return event;
        }
    }
}

/// Эталон: сэмплы `full.wav`.
fn reference() -> Vec<f32> {
    read_wav("full.wav")
}

/// 16-битный WAV, прочитанный без Symphonia, чтобы эталон не зависел от проверяемого декодера.
fn read_wav(name: &str) -> Vec<f32> {
    let wav = std::fs::read(format!("{FIXTURES}/{name}")).unwrap();
    let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
    wav[data..]
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect()
}

#[test]
fn flac_tracks_join_sample_exact() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "a.flac"));
    engine.set_next(Some(track(2, "b.flac")));
    let played = output.collect(FULL_FRAMES * 2);
    assert!(
        played == reference(),
        "склейка a.flac + b.flac отличается от исходного сигнала"
    );
    assert_eq!(
        wait_event(&rx, |e| matches!(
            e,
            EngineEvent::TrackStarted {
                track: TrackId(2),
                ..
            }
        )),
        EngineEvent::TrackStarted {
            track: TrackId(2),
            gapless: true
        }
    );
    wait_event(&rx, |e| *e == EngineEvent::Finished);
    assert!(
        output.pull(64).is_empty(),
        "после конца очереди — только тишина"
    );
}

#[test]
fn late_next_track_still_joins_without_gap() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "a.flac"));
    // a.flac (0,25 с) целиком помещается в буфер: декодер дойдёт до конца раньше,
    // чем узнает о следующем треке. Пока буфер звучит, ещё не поздно.
    std::thread::sleep(Duration::from_millis(100));
    engine.set_next(Some(track(2, "b.flac")));
    wait_event(&rx, |e| *e == EngineEvent::Preloading { track: TrackId(2) });
    assert!(output.collect(FULL_FRAMES * 2) == reference());
}

/// «Играть» — забирать сэмплы, как устройство, — пока не придёт нужное событие.
fn play_until(
    output: &ManualOutput,
    rx: &Receiver<EngineEvent>,
    pred: impl Fn(&EngineEvent) -> bool,
) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut played = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "событие не пришло");
        while let Ok(event) = rx.try_recv() {
            if pred(&event) {
                return played;
            }
        }
        let chunk = output.pull(1024);
        if chunk.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
        }
        played.extend(chunk);
    }
}

#[test]
fn mp3_tracks_join_without_gap() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "a.mp3"));
    engine.set_next(Some(track(2, "b.mp3")));
    let played = play_until(&output, &rx, |e| *e == EngineEvent::Finished);
    // Задержка и паддинг кодера обрезаны: кадров ровно столько, сколько в исходнике.
    assert_eq!(played.len(), FULL_FRAMES * 2);
    // Эталон — та же склейка, раскодированная ffmpeg. С исходным сигналом MP3 у стыка
    // расходится и у ffmpeg: каждый кусок закодирован отдельно, это свойство файлов.
    let max_err = played
        .iter()
        .zip(read_wav("mp3_ffmpeg.wav"))
        .map(|(p, r)| (p - r).abs())
        .fold(0.0f32, f32::max);
    assert!(max_err <= 1.5 / 32768.0, "отклонение от ffmpeg {max_err}");
}

#[test]
fn seek_lands_on_exact_frame() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "full.flac"));
    output.collect(2048);
    engine.seek(250);
    // После события всё, что придёт в вывод, — уже с новой позиции.
    assert_eq!(
        wait_event(&rx, |e| matches!(e, EngineEvent::Seeked { .. })),
        EngineEvent::Seeked {
            track: TrackId(1),
            position_ms: 250
        }
    );
    // После перемотки 5 мс плавного нарастания (≈221 кадр); дальше — точно эталон.
    let ramp = 221 * 2;
    let after = output.collect(ramp + 2000);
    let start = 11_025 * 2;
    assert!(after[ramp..] == reference()[start + ramp..start + ramp + 2000]);
}

#[test]
fn pause_keeps_position_and_loses_no_audio() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "full.flac"));
    let mut played = output.collect(4000);
    engine.pause();
    // Затухание: ещё немного данных, потом тишина.
    loop {
        let chunk = output.pull(512);
        if chunk.is_empty() {
            break;
        }
        played.extend(chunk);
    }
    let paused_at = engine.playhead().unwrap().position_ms;
    std::thread::sleep(Duration::from_millis(30));
    assert!(output.pull(512).is_empty(), "на паузе буфер не тратится");
    assert_eq!(engine.playhead().unwrap().position_ms, paused_at);
    engine.resume();
    played.extend(play_until(&output, &rx, |e| *e == EngineEvent::Finished));
    // Ни один кадр не потерян и не повторён: затухание лишь приглушило кадры.
    assert_eq!(played.len(), FULL_FRAMES * 2);
}

#[test]
fn different_sample_rate_reopens_output() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "a.flac"));
    engine.set_next(Some(track(2, "c48k.flac")));
    play_until(&output, &rx, |e| {
        *e == EngineEvent::TrackStarted {
            track: TrackId(2),
            gapless: false,
        }
    });
    let rates: Vec<u32> = output
        .formats
        .lock()
        .unwrap()
        .iter()
        .map(|f| f.sample_rate)
        .collect();
    assert_eq!(rates, [44_100, 48_000]);
}

#[test]
fn volume_and_replay_gain_scale_samples() {
    let output = ManualOutput::default();
    let (engine, _rx) = engine(&output);
    engine.set_volume(0.5);
    engine.set_gain(GainConfig {
        mode: ReplayGainMode::Track,
        preamp_db: 0.0,
        prevent_clipping: false,
    });
    let mut t = track(1, "full.flac");
    t.replay_gain.track_gain_db = Some(-6.020_6); // ×0.5
    engine.play(t);
    let played = output.collect(2000);
    let expected = reference();
    for (p, r) in played.iter().zip(&expected) {
        assert!((p - r * 0.25).abs() < 1e-4, "{p} vs {}", r * 0.25);
    }
}

#[test]
fn missing_file_reports_error() {
    let output = ManualOutput::default();
    let (engine, rx) = engine(&output);
    engine.play(track(7, "missing.flac"));
    let event = wait_event(&rx, |e| matches!(e, EngineEvent::Error { .. }));
    assert!(matches!(
        event,
        EngineEvent::Error {
            track: Some(TrackId(7)),
            ..
        }
    ));
}

#[test]
fn small_buffer_does_not_stall() {
    let output = ManualOutput::default();
    let (tx, rx) = mpsc::channel();
    let config = EngineConfig {
        buffer_ms: 100, // 8 кГц × 0,1 с = 800 кадров — меньше порога записи
        ..EngineConfig::default()
    };
    let engine = Engine::new(Box::new(output.clone()), Fixtures::load(), config, tx);
    engine.play(track(1, "mono8k.flac"));
    let played = play_until(&output, &rx, |e| *e == EngineEvent::Finished);
    assert_eq!(played.len(), 4_000);
}

#[test]
fn resamples_when_device_lacks_track_rate() {
    let output = output_48k();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "full.flac"));
    let played = play_until(&output, &rx, |e| *e == EngineEvent::Finished);
    assert_eq!(
        *output.formats.lock().unwrap(),
        [PcmFormat {
            sample_rate: 48_000,
            channels: 2
        }]
    );
    // 0,6 с при 48 кГц.
    assert_eq!(played.len(), 28_800 * 2);
    // Высота тона и фаза сохранены; края с переходным процессом фильтра не считаем.
    let err = max_error_48k(&played, 0.0, 500, 28_300);
    assert!(err < 1e-3, "отклонение от сигнала {err}");
}

#[test]
fn resampled_split_tracks_equal_resampled_whole() {
    let collect = |tracks: &[&str]| {
        let output = output_48k();
        let (engine, rx) = engine(&output);
        engine.play(track(1, tracks[0]));
        engine.set_next(tracks.get(1).map(|f| track(2, f)));
        play_until(&output, &rx, |e| *e == EngineEvent::Finished)
    };
    let whole = collect(&["full.flac"]);
    let split = collect(&["a.flac", "b.flac"]);
    // Один непрерывный ресемплер на оба трека: стык неотличим от середины файла.
    assert!(split == whole);
}

#[test]
fn fixed_rate_joins_tracks_of_different_rates_without_gap() {
    let output = ManualOutput::default();
    let (engine, rx) = engine_with(&output, OutputRate::Fixed(48_000));
    engine.play(track(1, "a.flac")); // 44,1 кГц
    engine.set_next(Some(track(2, "c48k.flac"))); // 48 кГц
    let events = Mutex::new(Vec::new());
    let played = play_until(&output, &rx, |e| {
        events.lock().unwrap().push(e.clone());
        *e == EngineEvent::Finished
    });
    // Вывод не переоткрывался, переход бесшовный.
    assert_eq!(
        *output.formats.lock().unwrap(),
        [PcmFormat {
            sample_rate: 48_000,
            channels: 2
        }]
    );
    let started = EngineEvent::TrackStarted {
        track: TrackId(2),
        gapless: true,
    };
    assert!(events.lock().unwrap().contains(&started));
    // ceil(11113 × 160 / 147) = 12096 кадров первого трека + 0,2 с второго.
    assert_eq!(played.len(), (12_096 + 9_600) * 2);
}

#[test]
fn seek_with_resampling_lands_on_time() {
    let output = output_48k();
    let (engine, rx) = engine(&output);
    engine.play(track(1, "full.flac"));
    output.collect(2048);
    engine.seek(250);
    wait_event(&rx, |e| matches!(e, EngineEvent::Seeked { .. }));
    let after = output.collect(4000 * 2);
    // Нарастание громкости (5 мс) и переходный процесс фильтра пропускаем.
    let err = max_error_48k(&after, 0.25, 1000, 4000);
    assert!(err < 1e-3, "отклонение после перемотки {err}");
}
