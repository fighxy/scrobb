//! Управляющий объект движка и его рабочий поток.
//!
//! [`Engine`] — тонкая ручка: команды уходят в рабочий поток по каналу, а
//! пауза, громкость и позиция — через атомики, без ожидания. Рабочий поток
//! декодирует вперёд на `buffer_ms`, держит следующий трек для gapless и
//! переоткрывает вывод, если у следующего трека другой формат.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use player_domain::{MediaUri, ReplayGain, TrackId};
use player_ports::{AudioOutput, FileAccess, PcmFormat, PortError};

use crate::decoder::{DecodeError, TrackDecoder};
use crate::gain::{self, GainConfig};
use crate::resample::Resampler;
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
    /// Движок взял `track` из списка следующих ([`Engine::set_upcoming`]) и начал
    /// его готовить: пора передать треки, которые идут за ним.
    /// Приходит раньше, чем трек зазвучит, — на `buffer_ms`.
    Preloading {
        /// Трек.
        track: TrackId,
    },
    /// Перемотка выполнена: всё, что выводится дальше, — с этой позиции.
    Seeked {
        /// Трек.
        track: TrackId,
        /// Точная позиция, на которую попала перемотка.
        position_ms: u64,
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

/// Событие движка с эпохой. Эпоха растёт при каждом [`Engine::play`] и
/// [`Engine::stop`]: события прошлых эпох относятся к тому, что уже отменено
/// (например, трек начался, а пользователь тут же нажал «назад»), и их стоит
/// отбрасывать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineMessage {
    /// Эпоха, в которой возникло событие.
    pub epoch: u64,
    /// Событие.
    pub event: EngineEvent,
}

/// Что звучит сейчас.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playhead {
    /// Трек.
    pub track: TrackId,
    /// Позиция в треке.
    pub position_ms: u64,
}

/// Частота вывода.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputRate {
    /// Частота трека, если устройство её поддерживает, иначе — частота устройства
    /// с ресемплингом. Трек с другой частотой переоткрывает вывод: переход не бесшовный,
    /// зато звук без пересчёта (для bit-perfect). Для десктопа.
    #[default]
    FollowTrack,
    /// Всегда эта частота: остальное ресемплируется, переходы бесшовные при любых
    /// частотах треков. Для телефонов (обычно 48 000).
    Fixed(u32),
}

/// Настройки движка.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// ReplayGain.
    pub gain: GainConfig,
    /// Сколько декодировать вперёд.
    pub buffer_ms: u32,
    /// Частота вывода.
    pub output_rate: OutputRate,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            gain: GainConfig::default(),
            buffer_ms: 2_000,
            output_rate: OutputRate::default(),
        }
    }
}

enum Command {
    Play {
        track: EngineTrack,
        epoch: u64,
    },
    SetUpcoming {
        after: Option<TrackId>,
        tracks: Vec<EngineTrack>,
    },
    Seek(u64),
    Stop {
        epoch: u64,
    },
    SetGain(GainConfig),
    Shutdown,
}

/// Непрерывный кусок одного трека в буфере.
struct Segment {
    id: u32,
    track: EngineTrack,
    start_frame: u64,
    sample_rate: u32,
    /// Эпоха, в которой сегмент записан: с ней и сообщается о начале трека.
    epoch: u64,
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
    epoch: AtomicU64,
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
        events: Sender<EngineMessage>,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let shared = Arc::new(Shared::new());
        let segments: Segments = Arc::default();
        let worker = Worker {
            commands: rx,
            events,
            epoch: 0,
            files,
            config,
            output,
            shared: shared.clone(),
            segments: segments.clone(),
            stream: None,
            generation: 0,
            next_segment: 0,
            current: None,
            upcoming: VecDeque::new(),
            decoding: None,
            decoded: Vec::new(),
            pending: Vec::new(),
            pending_offset: 0,
            pending_header: Chunk {
                generation: 0,
                segment: 0,
                frames: 0,
                fade_in: false,
            },
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
            epoch: AtomicU64::new(0),
            shared,
            segments,
            worker: Some(worker),
        }
    }

    /// Играть трек с начала; следующие треки сбрасываются. Возвращает новую эпоху.
    pub fn play(&self, track: EngineTrack) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.send(Command::Play { track, epoch });
        epoch
    }

    /// Треки, которые идут за `after`, по порядку, для переходов без паузы.
    ///
    /// Список применяется, только если движок сейчас готовит именно `after`:
    /// ответ, опоздавший к уже ушедшему вперёд движку, отбрасывается. Если трек
    /// из списка не открылся, движок сразу берёт следующий — без ожидания.
    /// Если текущий трек уже декодирован, но ещё звучит, список подхватывается
    /// без паузы.
    pub fn set_upcoming(&self, after: TrackId, tracks: Vec<EngineTrack>) {
        self.send(Command::SetUpcoming {
            after: Some(after),
            tracks,
        });
    }

    /// Один следующий трек без привязки к текущему (для простых сценариев).
    pub fn set_next(&self, track: Option<EngineTrack>) {
        self.send(Command::SetUpcoming {
            after: None,
            tracks: track.into_iter().collect(),
        });
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

    /// Остановить и очистить очередь. Возвращает новую эпоху.
    pub fn stop(&self) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.send(Command::Stop { epoch });
        epoch
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
    /// Формат, в котором работает вывод.
    format: PcmFormat,
    /// Формат, который у вывода просили (до согласования частоты с устройством).
    requested: PcmFormat,
    writer: Writer,
}

/// Трек, который сейчас декодируется.
struct Active {
    track: EngineTrack,
    decoder: TrackDecoder,
    segment: u32,
    gain: f32,
    fade_in: bool,
    /// Частота, в которой трек попадает в буфер.
    out_rate: u32,
    /// Есть, если частота трека отличается от частоты вывода.
    resampler: Option<Resampler>,
}

struct Worker {
    commands: Receiver<Command>,
    events: Sender<EngineMessage>,
    /// Эпоха последней `play` или `stop`.
    epoch: u64,
    files: Arc<dyn FileAccess>,
    config: EngineConfig,
    output: Box<dyn AudioOutput>,
    shared: Arc<Shared>,
    segments: Segments,
    stream: Option<Stream>,
    generation: u32,
    next_segment: u32,
    current: Option<Active>,
    /// Следующие треки по порядку.
    upcoming: VecDeque<EngineTrack>,
    /// Трек, который движок готовит сейчас (или доготовил последним); к нему
    /// привязан список следующих.
    decoding: Option<TrackId>,
    /// Последний декодированный пакет (до ресемплинга).
    decoded: Vec<f32>,
    /// Готовые, но ещё не записанные в буфер сэмплы.
    pending: Vec<f32>,
    pending_offset: usize,
    /// Заголовок для `pending`: сегмент указан явно, потому что хвост ресемплера
    /// дописывается, когда текущим уже стал следующий трек.
    pending_header: Chunk,
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
            Command::Play { track, epoch } => {
                self.epoch = epoch;
                self.flush();
                self.upcoming.clear();
                self.decoding = Some(track.id);
                self.shared.paused.store(false, Ordering::Relaxed);
                match self.open(track) {
                    Ok(active) => self.start(active, 0, Some(false)),
                    Err((track, e)) => self.error(Some(track), e),
                }
            }
            Command::SetUpcoming { after, tracks } => {
                if after.is_some() && after != self.decoding {
                    return; // список устарел: движок уже ушёл дальше
                }
                self.upcoming = tracks.into();
                if self.finishing && !self.upcoming.is_empty() {
                    self.finishing = false;
                    self.track_ended();
                }
            }
            Command::Seek(position_ms) => self.seek(position_ms),
            Command::Stop { epoch } => {
                self.epoch = epoch;
                self.flush();
                self.upcoming.clear();
                self.decoding = None;
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
            out_rate: decoder.format().sample_rate,
            track,
            decoder,
            segment,
            fade_in: false,
            resampler: None,
        })
    }

    /// Формат, который просить у вывода для трека, и формат, который вывод даст.
    fn output_format(&mut self, src: PcmFormat) -> (PcmFormat, PcmFormat) {
        let wanted = match self.config.output_rate {
            OutputRate::FollowTrack => src,
            OutputRate::Fixed(rate) => PcmFormat {
                sample_rate: rate,
                ..src
            },
        };
        if let Some(stream) = self.stream.as_ref().filter(|s| s.requested == wanted) {
            return (wanted, stream.format);
        }
        let rate = self.output.sample_rate_for(wanted);
        (
            wanted,
            PcmFormat {
                sample_rate: rate,
                ..wanted
            },
        )
    }

    /// Подготовить трек к выводу на частоте `out_rate`: при необходимости — ресемплер.
    /// Подходящий ресемплер сохраняется вместе с состоянием.
    fn attach(&self, active: &mut Active, out_rate: u32) -> Result<(), DecodeError> {
        let src = active.decoder.format();
        active.out_rate = out_rate;
        if src.sample_rate == out_rate {
            active.resampler = None;
        } else if active
            .resampler
            .as_ref()
            .is_none_or(|r| r.rates() != (src.sample_rate, out_rate))
        {
            active.resampler = Some(Resampler::new(src.sample_rate, out_rate, src.channels)?);
        }
        Ok(())
    }

    /// Дописать хвост ресемплера закончившегося трека в его же сегмент.
    fn flush_resampler(&mut self, carry: Option<(Resampler, u32)>) {
        let Some((mut resampler, segment)) = carry else {
            return;
        };
        debug_assert!(
            self.pending_offset >= self.pending.len(),
            "хвост пишется после готовых данных"
        );
        self.pending.clear();
        self.pending_offset = 0;
        resampler.flush(&mut self.pending);
        self.pending_header = Chunk {
            generation: self.generation,
            segment,
            frames: 0,
            fade_in: false,
        };
    }

    /// Сделать трек текущим; при другом формате — переоткрыть вывод.
    fn start(&mut self, mut active: Active, start_frame: u64, announce: Option<bool>) {
        let (requested, format) = self.output_format(active.decoder.format());
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
            self.stream = Some(Stream {
                format,
                requested,
                writer,
            });
        }
        if let Err(e) = self.attach(&mut active, format.sample_rate) {
            return self.error(Some(active.track.id), e);
        }
        self.register(&active, start_frame, announce);
        self.current = Some(active);
    }

    /// Запомнить сегмент; `start_frame` — кадр трека в его родной частоте.
    fn register(&self, active: &Active, start_frame: u64, announce: Option<bool>) {
        let start_frame = active
            .resampler
            .as_ref()
            .map_or(start_frame, |r| r.to_output_frame(start_frame));
        let mut segments = lock(&self.segments);
        if segments.len() == KEEP_SEGMENTS {
            segments.pop_front();
        }
        segments.push_back(Segment {
            id: active.segment,
            track: active.track.clone(),
            start_frame,
            sample_rate: active.out_rate,
            epoch: self.epoch,
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
            _ => self.reopen.take().map(|a| a.track),
        };
        let reusable = current.filter(|a| a.track == track);
        self.flush();
        self.decoding = Some(track.id);
        if let Some(ahead) = ahead {
            self.upcoming.push_front(ahead);
        }
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
                if let Some(resampler) = &mut active.resampler {
                    resampler.reset();
                }
                let track = active.track.id;
                let position_ms = frame * 1000 / u64::from(active.decoder.format().sample_rate);
                let out = self.output_format(active.decoder.format()).1;
                if self.stream.as_ref().map(|s| s.format) != Some(out) {
                    self.start(active, frame, None);
                } else if let Err(e) = self.attach(&mut active, out.sample_rate) {
                    return self.error(Some(track), e);
                } else {
                    self.register(&active, frame, None);
                    self.current = Some(active);
                }
                if self.current.is_some() {
                    self.emit(EngineEvent::Seeked { track, position_ms });
                }
            }
            Err(e) => self.error(Some(active.track.id), e),
        }
    }

    fn can_write(&self) -> bool {
        let Some(stream) = &self.stream else {
            return false;
        };
        let channels = usize::from(stream.format.channels.max(1));
        let pending_frames = (self.pending.len() - self.pending_offset) / channels;
        if pending_frames == 0 && self.current.is_none() {
            return false;
        }
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
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        if self.pending_offset < self.pending.len() {
            let frames = stream
                .writer
                .write(&self.pending[self.pending_offset..], self.pending_header);
            if frames > 0 {
                self.pending_header.fade_in = false;
                self.pending_offset += frames * usize::from(stream.format.channels);
            }
            return;
        }
        self.pending.clear();
        self.pending_offset = 0;
        let Some(active) = self.current.as_mut() else {
            return;
        };
        self.decoded.clear();
        match active.decoder.decode_next(&mut self.decoded) {
            Ok(true) => {
                if active.gain != 1.0 {
                    let gain = active.gain;
                    self.decoded.iter_mut().for_each(|s| *s *= gain);
                }
                match active.resampler.as_mut() {
                    Some(resampler) => resampler.process(&self.decoded, &mut self.pending),
                    None => std::mem::swap(&mut self.decoded, &mut self.pending),
                }
                if !self.pending.is_empty() {
                    self.pending_header = Chunk {
                        generation: self.generation,
                        segment: active.segment,
                        frames: 0,
                        fade_in: active.fade_in,
                    };
                    active.fade_in = false;
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
        let mut carry = self
            .current
            .take()
            .and_then(|a| a.resampler.map(|r| (r, a.segment)));
        // Трек, который не открылся, пропускаем и сразу берём следующий из списка.
        let mut active = loop {
            let Some(next) = self.upcoming.pop_front() else {
                self.flush_resampler(carry);
                self.finishing = true;
                return;
            };
            self.decoding = Some(next.id);
            self.emit(EngineEvent::Preloading { track: next.id });
            match self.open(next) {
                Ok(active) => break active,
                Err((track, e)) => self.error(Some(track), e),
            }
        };
        let src = active.decoder.format();
        let out = self.output_format(src).1;
        if self.stream.as_ref().map(|s| s.format) != Some(out) {
            self.flush_resampler(carry);
            self.reopen = Some(active);
            return;
        }
        // Та же частота трека: продолжить тем же ресемплером — стык как внутри одного файла.
        if let Some((resampler, _)) =
            carry.take_if(|(r, _)| r.rates() == (src.sample_rate, out.sample_rate))
        {
            active.resampler = Some(resampler);
        }
        self.flush_resampler(carry);
        if let Err(e) = self.attach(&mut active, out.sample_rate) {
            self.error(Some(active.track.id), e);
            self.finishing = true;
            return;
        }
        self.register(&active, 0, Some(true));
        self.current = Some(active);
    }

    /// Буфер доиграл: переоткрыть вывод под следующий трек или сообщить о конце.
    fn after_drain(&mut self) {
        let written = self.pending_offset >= self.pending.len();
        if !written || !self.stream.as_ref().is_none_or(|s| s.writer.is_drained()) {
            return;
        }
        if let Some(active) = self.reopen.take() {
            self.start(active, 0, Some(false));
        } else if self.finishing {
            self.finishing = false;
            self.emit(EngineEvent::Finished);
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
            .and_then(|s| s.announce.map(|gapless| (s.track.id, gapless, s.epoch)));
        if let Some((track, gapless, epoch)) = started {
            let event = EngineEvent::TrackStarted { track, gapless };
            let _ = self.events.send(EngineMessage { epoch, event });
        }
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(EngineMessage {
            epoch: self.epoch,
            event,
        });
    }

    fn error(&self, track: Option<TrackId>, e: DecodeError) {
        self.emit(EngineEvent::Error {
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
