//! Управляющий объект движка и его рабочий поток.
//!
//! [`Engine`] — тонкая ручка: команды уходят в рабочий поток по каналу, а
//! пауза, громкость и позиция — через атомики, без ожидания. Рабочий поток
//! декодирует вперёд на `buffer_ms`, держит следующий трек для gapless и
//! переоткрывает вывод, если у следующего трека другой формат.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use player_domain::{MediaUri, ReplayGain, TrackId};
use player_ports::{AudioOutput, FileAccess, PcmFormat, PortError};

use crate::decoder::{DecodeError, TrackDecoder};
use crate::gain::{self, GainConfig};
use crate::rt::{self, Chunk, Shared, Writer};

/// Как часто рабочий поток просыпается, когда буфер полон.
const POLL: Duration = Duration::from_millis(10);
/// Не дробить запись: ждать, пока освободится хотя бы столько кадров.
const MIN_WRITE_FRAMES: usize = 2048;
/// Сколько последних сегментов помнить для расчёта позиции.
const KEEP_SEGMENTS: usize = 16;

/// Трек для движка.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineTrack {
    /// Идентификатор в библиотеке.
    pub id: TrackId,
    /// Файл; открывается через порт `FileAccess`.
    pub uri: MediaUri,
    /// ReplayGain из тегов.
    pub replay_gain: ReplayGain,
}

/// Событие движка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    /// Трек зазвучал. `gapless` — без паузы сразу после предыдущего.
    TrackStarted {
        /// Трек.
        track: TrackId,
        /// Переход без паузы.
        gapless: bool,
    },
    /// Движок начал готовить `track` из [`Engine::set_next`]: пора передать следующий.
    /// Приходит раньше, чем трек зазвучит, — на `buffer_ms`.
    Preloading {
        /// Трек.
        track: TrackId,
    },
    /// Очередь доиграла: следующего трека нет.
    Finished,
    /// Трек не открылся или не декодируется; движок переходит к следующему.
    Error {
        /// Трек, если ошибка связана с ним.
        track: Option<TrackId>,
        /// Описание.
        message: String,
    },
}

/// Что звучит сейчас.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playhead {
    /// Трек.
    pub track: TrackId,
    /// Позиция в треке.
    pub position_ms: u64,
}

/// Настройки движка.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// ReplayGain.
    pub gain: GainConfig,
    /// Сколько декодировать вперёд.
    pub buffer_ms: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            gain: GainConfig::default(),
            buffer_ms: 2_000,
        }
    }
}

enum Command {
    Play(EngineTrack),
    SetNext(Option<EngineTrack>),
    Seek(u64),
    Stop,
    SetGain(GainConfig),
    Shutdown,
}

/// Непрерывный кусок одного трека в буфере.
struct Segment {
    id: u32,
    track: EngineTrack,
    start_frame: u64,
    sample_rate: u32,
    /// `Some(gapless)` — сообщить о начале трека, когда сегмент зазвучит.
    announce: Option<bool>,
}

type Segments = Arc<Mutex<VecDeque<Segment>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Звуковой движок.
pub struct Engine {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    segments: Segments,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    /// Запустить движок. События приходят в `events`.
    pub fn new(
        output: Box<dyn AudioOutput>,
        files: Arc<dyn FileAccess>,
        config: EngineConfig,
        events: Sender<EngineEvent>,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let shared = Arc::new(Shared::new());
        let segments: Segments = Arc::default();
        let worker = Worker {
            commands: rx,
            events,
            files,
            config,
            output,
            shared: shared.clone(),
            segments: segments.clone(),
            stream: None,
            generation: 0,
            next_segment: 0,
            current: None,
            next: None,
            pending: Vec::new(),
            pending_offset: 0,
            reopen: None,
            finishing: false,
            last_segment: None,
        };
        let worker = thread::Builder::new()
            .name("player-engine".into())
            .spawn(move || worker.run())
            .expect("поток движка запускается");
        Self {
            commands,
            shared,
            segments,
            worker: Some(worker),
        }
    }

    /// Играть трек с начала; следующий трек сбрасывается.
    pub fn play(&self, track: EngineTrack) {
        self.send(Command::Play(track));
    }

    /// Следующий трек для перехода без паузы. Если текущий уже декодирован,
    /// но ещё звучит, следующий всё равно подхватывается без паузы.
    pub fn set_next(&self, track: Option<EngineTrack>) {
        self.send(Command::SetNext(track));
    }

    /// Пауза с плавным затуханием.
    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    /// Продолжить.
    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::Relaxed);
    }

    /// На паузе ли.
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    /// Перемотка в звучащем треке.
    pub fn seek(&self, position_ms: u64) {
        self.send(Command::Seek(position_ms));
    }

    /// Остановить и очистить очередь.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Громкость 0.0–1.0 (линейный множитель амплитуды).
    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    /// Настройки ReplayGain; действуют на ещё не декодированные данные.
    pub fn set_gain(&self, gain: GainConfig) {
        self.send(Command::SetGain(gain));
    }

    /// Что звучит сейчас.
    pub fn playhead(&self) -> Option<Playhead> {
        let (segment, frames) = self.shared.playhead()?;
        let segments = lock(&self.segments);
        let s = segments.iter().find(|s| s.id == segment)?;
        Some(Playhead {
            track: s.track.id,
            position_ms: (s.start_frame + frames) * 1000 / u64::from(s.sample_rate),
        })
    }

    /// Сколько раз выводу не хватило данных (включая тишину после конца очереди).
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    fn send(&self, command: Command) {
        // Поток движка живёт, пока жив `Engine`, поэтому отправка не падает.
        let _ = self.commands.send(command);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Открытый вывод и сторона записи его буфера.
struct Stream {
    format: PcmFormat,
    writer: Writer,
}

/// Трек, который сейчас декодируется.
struct Active {
    track: EngineTrack,
    decoder: TrackDecoder,
    segment: u32,
    gain: f32,
    fade_in: bool,
}

struct Worker {
    commands: Receiver<Command>,
    events: Sender<EngineEvent>,
    files: Arc<dyn FileAccess>,
    config: EngineConfig,
    output: Box<dyn AudioOutput>,
    shared: Arc<Shared>,
    segments: Segments,
    stream: Option<Stream>,
    generation: u32,
    next_segment: u32,
    current: Option<Active>,
    next: Option<EngineTrack>,
    /// Декодированные, но ещё не записанные в буфер сэмплы текущего трека.
    pending: Vec<f32>,
    pending_offset: usize,
    /// Следующий трек другого формата: ждёт, пока доиграет буфер.
    reopen: Option<Active>,
    /// Последний трек декодирован; ждём, пока доиграет буфер.
    finishing: bool,
    last_segment: Option<u32>,
}

impl Worker {
    fn run(mut self) {
        loop {
            let busy = self.can_write();
            let command = if busy {
                self.commands.try_recv().map_err(|e| match e {
                    mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                    mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                })
            } else {
                self.commands.recv_timeout(POLL)
            };
            match command {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(command) => {
                    self.handle(command);
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.report();
            self.after_drain();
            if busy {
                self.produce();
            }
        }
        self.output.stop();
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play(track) => {
                self.flush();
                self.next = None;
                self.shared.paused.store(false, Ordering::Relaxed);
                match self.open(track) {
                    Ok(active) => self.start(active, 0, Some(false)),
                    Err((track, e)) => self.error(Some(track), e),
                }
            }
            Command::SetNext(track) => {
                self.next = track;
                if self.finishing && self.next.is_some() {
                    self.finishing = false;
                    self.track_ended();
                }
            }
            Command::Seek(position_ms) => self.seek(position_ms),
            Command::Stop => {
                self.flush();
                self.next = None;
                lock(&self.segments).clear();
                self.shared.clear_playhead();
                self.last_segment = None;
            }
            Command::SetGain(gain) => self.config.gain = gain,
            Command::Shutdown => {}
        }
    }

    /// Отбросить всё, что ещё не прозвучало.
    fn flush(&mut self) {
        self.generation += 1;
        self.shared
            .flush_before
            .store(self.generation, Ordering::Release);
        self.current = None;
        self.reopen = None;
        self.finishing = false;
        self.pending.clear();
        self.pending_offset = 0;
    }

    fn open(&mut self, track: EngineTrack) -> Result<Active, (TrackId, DecodeError)> {
        let extension = track
            .uri
            .0
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| ext.len() <= 5 && !ext.contains('/'));
        let decoder = self
            .files
            .open(&track.uri)
            .map_err(|e| DecodeError(port_error(e)))
            .and_then(|stream| TrackDecoder::open(stream, extension))
            .map_err(|e| (track.id, e))?;
        let segment = rt::segment_id(self.next_segment);
        self.next_segment = self.next_segment.wrapping_add(1);
        Ok(Active {
            gain: gain::factor(&track.replay_gain, &self.config.gain),
            track,
            decoder,
            segment,
            fade_in: false,
        })
    }

    /// Сделать трек текущим; при другом формате — переоткрыть вывод.
    fn start(&mut self, active: Active, start_frame: u64, announce: Option<bool>) {
        let format = active.decoder.format();
        if self.stream.as_ref().map(|s| s.format) != Some(format) {
            if self.stream.take().is_some() {
                self.output.stop();
            }
            let (writer, source) = rt::channel(format, self.config.buffer_ms, self.shared.clone());
            let opened = self
                .output
                .open(format, Box::new(source))
                .and_then(|()| self.output.start());
            if let Err(e) = opened {
                self.error(Some(active.track.id), DecodeError(port_error(e)));
                return;
            }
            self.stream = Some(Stream { format, writer });
        }
        self.register(&active, start_frame, announce);
        self.current = Some(active);
    }

    fn register(&self, active: &Active, start_frame: u64, announce: Option<bool>) {
        let mut segments = lock(&self.segments);
        if segments.len() == KEEP_SEGMENTS {
            segments.pop_front();
        }
        segments.push_back(Segment {
            id: active.segment,
            track: active.track.clone(),
            start_frame,
            sample_rate: active.decoder.format().sample_rate,
            announce,
        });
    }

    fn seek(&mut self, position_ms: u64) {
        let audible = self.shared.playhead().and_then(|(id, _)| {
            lock(&self.segments)
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.track.clone())
        });
        let Some(track) = audible.or_else(|| self.current.as_ref().map(|a| a.track.clone())) else {
            return;
        };
        // Декодер мог уйти вперёд, в следующий трек: тогда звучащий трек
        // открываем заново, а ушедший вперёд снова становится следующим.
        let current = self.current.take();
        let ahead = match &current {
            Some(a) if a.track != track => Some(a.track.clone()),
            _ => self.reopen.take().map(|a| a.track).or(self.next.take()),
        };
        let reusable = current.filter(|a| a.track == track);
        self.flush();
        self.next = ahead;
        let mut active = match reusable {
            Some(a) => a,
            None => match self.open(track) {
                Ok(a) => a,
                Err((track, e)) => return self.error(Some(track), e),
            },
        };
        match active.decoder.seek(position_ms) {
            Ok(frame) => {
                active.segment = rt::segment_id(self.next_segment);
                self.next_segment = self.next_segment.wrapping_add(1);
                active.fade_in = true;
                if self.stream.is_none() {
                    return self.start(active, frame, None);
                }
                self.register(&active, frame, None);
                self.current = Some(active);
            }
            Err(e) => self.error(Some(active.track.id), e),
        }
    }

    fn can_write(&self) -> bool {
        let Some(stream) = &self.stream else {
            return false;
        };
        let Some(active) = &self.current else {
            return false;
        };
        let channels = usize::from(active.decoder.format().channels.max(1));
        let pending_frames = (self.pending.len() - self.pending_offset) / channels;
        let needed = if pending_frames == 0 {
            MIN_WRITE_FRAMES
        } else {
            pending_frames.min(MIN_WRITE_FRAMES)
        };
        // Маленький буфер (низкая частота, малый buffer_ms) может быть меньше порога.
        stream.writer.free_frames() >= needed.min(stream.writer.capacity_frames() / 2).max(1)
    }

    /// Записать готовые сэмплы или декодировать следующий пакет.
    fn produce(&mut self) {
        let (Some(stream), Some(active)) = (self.stream.as_mut(), self.current.as_mut()) else {
            return;
        };
        if self.pending_offset < self.pending.len() {
            let header = Chunk {
                generation: self.generation,
                segment: active.segment,
                frames: 0,
                fade_in: active.fade_in,
            };
            let frames = stream
                .writer
                .write(&self.pending[self.pending_offset..], header);
            if frames > 0 {
                active.fade_in = false;
                self.pending_offset += frames * usize::from(stream.format.channels);
            }
            return;
        }
        self.pending.clear();
        self.pending_offset = 0;
        match active.decoder.decode_next(&mut self.pending) {
            Ok(true) => {
                if active.gain != 1.0 {
                    let gain = active.gain;
                    self.pending.iter_mut().for_each(|s| *s *= gain);
                }
            }
            Ok(false) => self.track_ended(),
            Err(e) => {
                let track = active.track.id;
                self.error(Some(track), e);
                self.track_ended();
            }
        }
    }

    /// Текущий трек декодирован до конца: подхватить следующий.
    fn track_ended(&mut self) {
        self.current = None;
        let Some(next) = self.next.take() else {
            self.finishing = true;
            return;
        };
        let _ = self.events.send(EngineEvent::Preloading { track: next.id });
        match self.open(next) {
            Ok(active)
                if self.stream.as_ref().map(|s| s.format) == Some(active.decoder.format()) =>
            {
                self.register(&active, 0, Some(true));
                self.current = Some(active);
            }
            Ok(active) => self.reopen = Some(active),
            Err((track, e)) => {
                self.error(Some(track), e);
                self.finishing = true;
            }
        }
    }

    /// Буфер доиграл: переоткрыть вывод под следующий трек или сообщить о конце.
    fn after_drain(&mut self) {
        if !self.stream.as_ref().is_none_or(|s| s.writer.is_drained()) {
            return;
        }
        if let Some(active) = self.reopen.take() {
            self.start(active, 0, Some(false));
        } else if self.finishing {
            self.finishing = false;
            let _ = self.events.send(EngineEvent::Finished);
        }
    }

    /// Сообщить о начале трека, когда его первый сегмент зазвучал.
    fn report(&mut self) {
        let Some((segment, _)) = self.shared.playhead() else {
            return;
        };
        if self.last_segment == Some(segment) {
            return;
        }
        self.last_segment = Some(segment);
        let started = lock(&self.segments)
            .iter()
            .find(|s| s.id == segment)
            .and_then(|s| s.announce.map(|gapless| (s.track.id, gapless)));
        if let Some((track, gapless)) = started {
            let _ = self
                .events
                .send(EngineEvent::TrackStarted { track, gapless });
        }
    }

    fn error(&self, track: Option<TrackId>, e: DecodeError) {
        let _ = self.events.send(EngineEvent::Error {
            track,
            message: e.0,
        });
    }
}

fn port_error(e: PortError) -> String {
    match e {
        PortError::Unsupported => "не поддерживается".into(),
        PortError::PermissionDenied => "нет доступа".into(),
        PortError::Other(message) => message,
    }
}
