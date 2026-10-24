//! Проиграть файлы подряд без пауз на устройстве по умолчанию:
//!
//! ```sh
//! cargo run --release -p player-output-cpal --example play -- a.flac b.flac c.mp3
//! # всё на одной частоте, переходы без пауз даже между 44,1 и 48 кГц:
//! cargo run --release -p player-output-cpal --example play -- --rate=48000 a.flac b.flac
//! ```

use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use player_domain::{MediaUri, ReplayGain, TrackId};
use player_engine::{Engine, EngineConfig, EngineEvent, EngineTrack, OutputRate};
use player_output_cpal::CpalOutput;
use player_ports::{FileAccess, FileEntry, MediaStream, PortError};

/// Файлы с локального диска.
struct LocalFiles;

impl FileAccess for LocalFiles {
    fn list(&self, _: &MediaUri) -> Result<Vec<FileEntry>, PortError> {
        Err(PortError::Unsupported)
    }

    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError> {
        File::open(&uri.0)
            .map(|f| Box::new(f) as Box<dyn MediaStream>)
            .map_err(|e| PortError::Other(format!("{}: {e}", uri.0)))
    }
}

fn main() {
    let mut config = EngineConfig::default();
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.strip_prefix("--rate=").map(str::parse) {
            Some(Ok(rate)) => config.output_rate = OutputRate::Fixed(rate),
            Some(Err(_)) => {
                eprintln!("неверная частота: {arg}");
                std::process::exit(2);
            }
            None => paths.push(arg),
        }
    }
    if paths.is_empty() {
        eprintln!("использование: play [--rate=48000] <файл> [<файл>…]");
        std::process::exit(2);
    }
    let track = |i: usize| {
        paths.get(i).map(|path| EngineTrack {
            id: TrackId(i as u64),
            uri: MediaUri(path.clone()),
            replay_gain: ReplayGain::default(),
        })
    };

    let (events, rx) = mpsc::channel();
    let engine = Engine::new(
        Box::new(CpalOutput::new()),
        Arc::new(LocalFiles),
        config,
        events,
    );
    engine.play(track(0).unwrap());
    engine.set_next(track(1));

    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(EngineEvent::TrackStarted { track: id, gapless }) => {
                let how = if gapless { " (без паузы)" } else { "" };
                println!("\n▶ {}{how}", paths[id.0 as usize]);
            }
            Ok(EngineEvent::Preloading { track: id }) => engine.set_next(track(id.0 as usize + 1)),
            Ok(EngineEvent::Error { track: id, message }) => {
                eprintln!("\nошибка: {message}");
                // Битый трек пропускаем: если ничего не звучит — играем следующий.
                let following = id.and_then(|id| track(id.0 as usize + 1));
                match (engine.playhead(), following) {
                    (None, Some(t)) => engine.play(t),
                    (None, None) => break,
                    (Some(_), t) => engine.set_next(t),
                }
            }
            Ok(EngineEvent::Seeked { .. }) => {}
            Ok(EngineEvent::Finished) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(p) = engine.playhead() {
                    print!(
                        "\r  {}:{:02}  ",
                        p.position_ms / 60_000,
                        p.position_ms / 1000 % 60
                    );
                    let _ = std::io::stdout().flush();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    println!();
}
