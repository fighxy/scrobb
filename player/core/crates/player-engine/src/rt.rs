//! Связь потока движка с аудиопотоком.
//!
//! Два кольцевых буфера без блокировок (rtrb): сэмплы и заголовки порций.
//! Поток движка пишет сэмплы порции, затем её заголовок; аудиопоток читает
//! заголовок и ровно столько сэмплов. Всё остальное — атомики в [`Shared`].
//!
//! Перемотка и смена трека не трогают буфер со стороны записи: движок
//! увеличивает поколение, а аудиопоток сам отбрасывает порции старых поколений.
//! В аудиопотоке нет блокировок, выделения памяти и ввода-вывода.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use player_ports::{PcmFormat, PcmSource};
use rtrb::{Consumer, Producer, RingBuffer};

/// Заголовок порции сэмплов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Поколение: порции поколений меньше `Shared::flush_before` отбрасываются.
    pub generation: u32,
    /// Сегмент — непрерывный кусок одного трека (после перемотки — новый сегмент).
    pub segment: u32,
    /// Число кадров в порции.
    pub frames: u32,
    /// Начать порцию с плавного нарастания громкости (после перемотки).
    pub fade_in: bool,
}

/// Длительность плавного изменения громкости.
const RAMP_SECONDS: f32 = 0.005;
/// Ёмкость очереди заголовков.
const MAX_CHUNKS: usize = 4096;
const FRAME_BITS: u32 = 40;
const FRAME_MASK: u64 = (1 << FRAME_BITS) - 1;
const NO_PLAYHEAD: u64 = u64::MAX;

/// Состояние, общее для управляющего кода, потока движка и аудиопотока.
pub(crate) struct Shared {
    /// Порции поколений меньше этого значения устарели.
    pub flush_before: AtomicU32,
    /// Пауза.
    pub paused: AtomicBool,
    volume_bits: AtomicU32,
    /// Что сейчас звучит: сегмент (24 бита) и кадр внутри сегмента (40 бит).
    playhead: AtomicU64,
    /// Сколько раз аудиопотоку не хватило данных (включая конец очереди).
    pub underruns: AtomicU64,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            flush_before: AtomicU32::new(0),
            paused: AtomicBool::new(false),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            playhead: AtomicU64::new(NO_PLAYHEAD),
            underruns: AtomicU64::new(0),
        }
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Сегмент и кадр внутри него, которые сейчас звучат.
    pub fn playhead(&self) -> Option<(u32, u64)> {
        let v = self.playhead.load(Ordering::Acquire);
        (v != NO_PLAYHEAD).then_some(((v >> FRAME_BITS) as u32, v & FRAME_MASK))
    }

    pub fn clear_playhead(&self) {
        self.playhead.store(NO_PLAYHEAD, Ordering::Release);
    }

    fn set_playhead(&self, segment: u32, frames: u64) {
        let v = (u64::from(segment & 0xFF_FFFF) << FRAME_BITS) | (frames & FRAME_MASK);
        self.playhead.store(v, Ordering::Release);
    }

    fn target_gain(&self) -> f32 {
        if self.paused.load(Ordering::Relaxed) {
            0.0
        } else {
            self.volume()
        }
    }
}

/// Номер сегмента, который помещается в позицию воспроизведения.
pub(crate) fn segment_id(n: u32) -> u32 {
    n & 0xFF_FFFF
}

/// Пара буферов для потока в формате `format` с запасом на `buffer_ms`.
pub(crate) fn channel(
    format: PcmFormat,
    buffer_ms: u32,
    shared: Arc<Shared>,
) -> (Writer, RtSource) {
    let channels = usize::from(format.channels.max(1));
    let frames = (u64::from(format.sample_rate) * u64::from(buffer_ms.max(100)) / 1000) as usize;
    let (samples_tx, samples_rx) = RingBuffer::new(frames * channels);
    let (chunks_tx, chunks_rx) = RingBuffer::new(MAX_CHUNKS);
    let gain = shared.target_gain();
    let writer = Writer {
        samples: samples_tx,
        chunks: chunks_tx,
        channels,
    };
    let source = RtSource {
        samples: samples_rx,
        chunks: chunks_rx,
        shared,
        channels,
        chunk: None,
        remaining: 0,
        segment: None,
        played: 0,
        gain,
        step: 1.0 / (format.sample_rate as f32 * RAMP_SECONDS).max(1.0),
    };
    (writer, source)
}

/// Сторона записи; принадлежит потоку движка.
pub(crate) struct Writer {
    samples: Producer<f32>,
    chunks: Producer<Chunk>,
    channels: usize,
}

impl Writer {
    /// Сколько кадров можно записать прямо сейчас.
    pub fn free_frames(&self) -> usize {
        if self.chunks.slots() == 0 {
            0
        } else {
            self.samples.slots() / self.channels
        }
    }

    /// Записать начало `data` одной порцией; вернуть число записанных кадров.
    pub fn write(&mut self, data: &[f32], header: Chunk) -> usize {
        let frames = (data.len() / self.channels).min(self.free_frames());
        if frames == 0 {
            return 0;
        }
        let n = frames * self.channels;
        let chunk = self
            .samples
            .write_chunk_uninit(n)
            .expect("места хватает: проверено через free_frames");
        chunk.fill_from_iter(data[..n].iter().copied());
        let header = Chunk {
            frames: frames as u32,
            ..header
        };
        self.chunks
            .push(header)
            .expect("место в очереди заголовков проверено через free_frames");
        frames
    }

    /// Ёмкость буфера в кадрах.
    pub fn capacity_frames(&self) -> usize {
        self.samples.buffer().capacity() / self.channels
    }

    /// Аудиопоток всё доиграл: оба буфера пусты.
    pub fn is_drained(&self) -> bool {
        self.samples.slots() == self.samples.buffer().capacity()
            && self.chunks.slots() == self.chunks.buffer().capacity()
    }
}

/// Сторона аудиопотока: отдаётся адаптеру вывода как [`PcmSource`].
pub(crate) struct RtSource {
    samples: Consumer<f32>,
    chunks: Consumer<Chunk>,
    shared: Arc<Shared>,
    channels: usize,
    chunk: Option<Chunk>,
    remaining: u32,
    segment: Option<u32>,
    played: u64,
    gain: f32,
    step: f32,
}

impl RtSource {
    /// Выбросить `frames` кадров из буфера сэмплов.
    fn discard(&mut self, frames: u32) {
        let n = (frames as usize * self.channels).min(self.samples.slots());
        if let Ok(chunk) = self.samples.read_chunk(n) {
            chunk.commit_all();
        }
    }

    /// Следующая актуальная порция; устаревшие отбрасываются. Поколение
    /// перечитывается на каждой порции: сброс посреди колбэка обрывает старый
    /// звук сразу после текущей порции, а не в следующем колбэке.
    fn next_chunk(&mut self) -> Option<Chunk> {
        while let Ok(chunk) = self.chunks.pop() {
            if chunk.generation < self.shared.flush_before.load(Ordering::Acquire) {
                self.discard(chunk.frames);
                continue;
            }
            return Some(chunk);
        }
        None
    }
}

/// Сдвинуть громкость на один кадр к цели.
/// Последний шаг попадает точно в цель, без остатка от округления.
fn step_gain(gain: &mut f32, step: f32, target: f32) {
    let delta = target - *gain;
    if delta.abs() <= step * 1.001 {
        *gain = target;
    } else {
        *gain += step.copysign(delta);
    }
}

impl PcmSource for RtSource {
    fn fill(&mut self, out: &mut [f32]) -> usize {
        let flush_before = self.shared.flush_before.load(Ordering::Acquire);
        if self.chunk.is_some_and(|c| c.generation < flush_before) {
            self.discard(self.remaining);
            self.chunk = None;
            self.remaining = 0;
        }
        let target = self.shared.target_gain();
        let ch = self.channels;
        let total_frames = out.len() / ch;
        let mut frame = 0;
        let mut consumed_any = false;

        'outer: while frame < total_frames {
            if self.remaining == 0 {
                let Some(chunk) = self.next_chunk() else {
                    if target > 0.0 {
                        self.shared.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                };
                if self.segment != Some(chunk.segment) {
                    self.segment = Some(chunk.segment);
                    self.played = 0;
                }
                if chunk.fade_in {
                    self.gain = 0.0;
                }
                self.chunk = Some(chunk);
                self.remaining = chunk.frames;
            }
            let run = (self.remaining as usize).min(total_frames - frame);
            let Ok(chunk) = self.samples.read_chunk(run * ch) else {
                break;
            };
            let (a, b) = chunk.as_slices();
            for i in 0..run {
                step_gain(&mut self.gain, self.step, target);
                if self.gain == 0.0 && target == 0.0 {
                    // Пауза: громкость уже на нуле — дальше буфер не тратим.
                    chunk.commit(i * ch);
                    self.remaining -= i as u32;
                    self.played += i as u64;
                    frame += i;
                    consumed_any |= i > 0;
                    break 'outer;
                }
                for c in 0..ch {
                    let idx = i * ch + c;
                    let s = if idx < a.len() {
                        a[idx]
                    } else {
                        b[idx - a.len()]
                    };
                    out[(frame + i) * ch + c] = s * self.gain;
                }
            }
            chunk.commit_all();
            self.remaining -= run as u32;
            self.played += run as u64;
            frame += run;
            consumed_any = true;
        }

        out[frame * ch..].fill(0.0);
        if consumed_any && let Some(segment) = self.segment {
            self.shared.set_playhead(segment, self.played);
        }
        frame * ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEREO: PcmFormat = PcmFormat {
        sample_rate: 1000,
        channels: 2,
    };

    fn header(generation: u32, segment: u32) -> Chunk {
        Chunk {
            generation,
            segment,
            frames: 0,
            fade_in: false,
        }
    }

    #[test]
    fn passes_samples_through_and_reports_playhead() {
        let shared = Arc::new(Shared::new());
        let (mut w, mut rt) = channel(STEREO, 1000, shared.clone());
        assert_eq!(w.write(&[0.1, 0.2, 0.3, 0.4], header(0, 7)), 2);
        let mut out = [9.0; 6];
        assert_eq!(rt.fill(&mut out), 4);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4, 0.0, 0.0]);
        assert_eq!(shared.playhead(), Some((7, 2)));
        assert!(w.is_drained());
    }

    #[test]
    fn flush_drops_stale_generations() {
        let shared = Arc::new(Shared::new());
        let (mut w, mut rt) = channel(STEREO, 1000, shared.clone());
        w.write(&[1.0; 8], header(0, 1));
        shared.flush_before.store(1, Ordering::Release);
        w.write(&[0.5, 0.5], header(1, 2));
        let mut out = [0.0; 4];
        assert_eq!(rt.fill(&mut out), 2);
        assert_eq!(out, [0.5, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn pause_fades_out_then_keeps_buffer() {
        let shared = Arc::new(Shared::new());
        let (mut w, mut rt) = channel(STEREO, 1000, shared.clone());
        w.write(&[1.0; 40], header(0, 1));
        shared.paused.store(true, Ordering::Relaxed);
        let mut out = [0.0; 40];
        let taken = rt.fill(&mut out);
        // Нарастание на 1000 Гц длится 5 кадров: 0.8, 0.6, 0.4, 0.2, затем тишина.
        assert_eq!(taken, 8);
        assert!((out[0] - 0.8).abs() < 1e-6 && out[8] == 0.0);
        assert_eq!(rt.fill(&mut out), 0);
        shared.paused.store(false, Ordering::Relaxed);
        assert_eq!(rt.fill(&mut out), 32);
    }
}
