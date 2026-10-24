//! Ресемплинг на rubato: синхронный FFT-ресемплер для фиксированной пары частот.
//!
//! Работает потоком: принимает куски любого размера, копит вход до размера
//! блока rubato и отдаёт выход. Задержку фильтра обрезает в начале, а в конце
//! ([`Resampler::flush`]) дожимает хвост так, чтобы выход был ровно
//! `ceil(входные кадры × out / in)` кадров. Результат не зависит от того, какими
//! кусками пришёл вход, — поэтому треки одной частоты, пропущенные через один
//! ресемплер подряд, склеиваются так же, как один длинный файл.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler as _};

use crate::decoder::DecodeError;

/// Размер входного блока rubato, кадров.
const CHUNK_FRAMES: usize = 1024;

/// Потоковый ресемплер одной пары частот.
pub struct Resampler {
    inner: Fft<f32>,
    channels: usize,
    in_rate: u32,
    out_rate: u32,
    /// Накопленный вход, меньше одного блока (чередование каналов).
    input: Vec<f32>,
    /// Выход одного блока.
    output: Vec<f32>,
    /// Сколько выходных кадров задержки фильтра ещё отбросить.
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl Resampler {
    /// Ресемплер из `in_rate` в `out_rate` для `channels` каналов.
    pub fn new(in_rate: u32, out_rate: u32, channels: u16) -> Result<Self, DecodeError> {
        let channels = usize::from(channels.max(1));
        let inner = Fft::<f32>::new(
            in_rate as usize,
            out_rate as usize,
            CHUNK_FRAMES,
            channels,
            FixedSync::Input,
        )
        .map_err(|e| DecodeError(format!("ресемплер {in_rate} → {out_rate} Гц: {e}")))?;
        let output = vec![0.0; inner.output_frames_max() * channels];
        let skip = inner.output_delay();
        Ok(Self {
            inner,
            channels,
            in_rate,
            out_rate,
            input: Vec::with_capacity(CHUNK_FRAMES * channels * 2),
            output,
            skip,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Пара частот: вход и выход.
    pub fn rates(&self) -> (u32, u32) {
        (self.in_rate, self.out_rate)
    }

    /// Входной кадр → выходной (для позиции после перемотки).
    pub fn to_output_frame(&self, frame: u64) -> u64 {
        frame * u64::from(self.out_rate) / u64::from(self.in_rate)
    }

    /// Обработать кусок входа и дописать готовый выход в `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let ch = self.channels;
        self.frames_in += (input.len() / ch) as u64;
        self.input.extend_from_slice(input);
        let need = self.inner.input_frames_next();
        let mut offset = 0;
        while self.input.len() / ch - offset >= need {
            let produced = self.run(offset, None);
            offset += need;
            self.emit(produced, out, None);
        }
        self.input.drain(..offset * ch);
    }

    /// Конец потока: дописать хвост и подготовиться к новому потоку.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let ch = self.channels;
        let expected =
            (self.frames_in * u64::from(self.out_rate)).div_ceil(u64::from(self.in_rate));
        let need = self.inner.input_frames_next();
        // Задержка фильтра — не больше нескольких блоков; ограничение страхует от зацикливания.
        let mut rounds = 0;
        while self.frames_out < expected && rounds < 64 {
            let real = (self.input.len() / ch).min(need);
            self.input.resize(need * ch, 0.0);
            let produced = self.run(0, Some(real));
            self.input.clear();
            self.emit(produced, out, Some(expected));
            rounds += 1;
        }
        self.reset();
    }

    /// Забыть всё (после перемотки): следующий вход — начало нового потока.
    pub fn reset(&mut self) {
        self.inner.reset();
        self.input.clear();
        self.skip = self.inner.output_delay();
        self.frames_in = 0;
        self.frames_out = 0;
    }

    /// Один блок rubato; вернуть число выходных кадров в `self.output`.
    fn run(&mut self, offset: usize, partial: Option<usize>) -> usize {
        let ch = self.channels;
        let input = InterleavedSlice::new(&self.input[..], ch, self.input.len() / ch)
            .expect("длина входа кратна числу каналов");
        let frames_out = self.output.len() / ch;
        let mut output = InterleavedSlice::new_mut(&mut self.output[..], ch, frames_out)
            .expect("длина выхода кратна числу каналов");
        let indexing = Indexing {
            input_offset: offset,
            partial_len: partial,
            ..Indexing::new()
        };
        let (_, produced) = self
            .inner
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .expect("размеры буферов соответствуют требованиям ресемплера");
        produced
    }

    /// Дописать выход блока, отбросив задержку и не выходя за `limit` кадров всего.
    fn emit(&mut self, produced: usize, out: &mut Vec<f32>, limit: Option<u64>) {
        let ch = self.channels;
        let start = self.skip.min(produced);
        self.skip -= start;
        let mut n = produced - start;
        if let Some(limit) = limit {
            n = n.min(limit.saturating_sub(self.frames_out) as usize);
        }
        out.extend_from_slice(&self.output[start * ch..(start + n) * ch]);
        self.frames_out += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Стерео-синус: 440 Гц слева, 660 Гц справа.
    fn sine(rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f32 / rate as f32;
                [
                    0.5 * (2.0 * PI * 440.0 * t).sin(),
                    0.5 * (2.0 * PI * 660.0 * t).sin(),
                ]
            })
            .collect()
    }

    fn resample(input: &[f32], from: u32, to: u32, piece: usize) -> Vec<f32> {
        let mut r = Resampler::new(from, to, 2).unwrap();
        let mut out = Vec::new();
        for chunk in input.chunks(piece * 2) {
            r.process(chunk, &mut out);
        }
        r.flush(&mut out);
        out
    }

    #[test]
    fn output_length_matches_ratio() {
        let input = sine(44_100, 26_460);
        assert_eq!(resample(&input, 44_100, 48_000, 1000).len(), 28_800 * 2);
        let input = sine(44_100, 11_113);
        // 11113 × 160 / 147 = 12095,8 → округление вверх.
        assert_eq!(resample(&input, 44_100, 48_000, 333).len(), 12_096 * 2);
    }

    #[test]
    fn result_does_not_depend_on_piece_size() {
        let input = sine(44_100, 20_000);
        let a = resample(&input, 44_100, 48_000, 1152);
        let b = resample(&input, 44_100, 48_000, 37);
        assert!(a == b);
    }

    #[test]
    fn keeps_pitch_and_alignment_both_ways() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000)] {
            let frames = from as usize / 2;
            let out = resample(&sine(from, frames), from, to, 512);
            let expected = sine(to, out.len() / 2);
            // Края отбрасываем: там у любого фильтра переходный процесс.
            let edge = 2 * 1000;
            let err = out[edge..out.len() - edge]
                .iter()
                .zip(&expected[edge..expected.len() - edge])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(err < 1e-3, "{from} → {to}: отклонение {err}");
        }
    }

    #[test]
    fn reset_starts_a_fresh_stream() {
        let input = sine(44_100, 5_000);
        let mut r = Resampler::new(44_100, 48_000, 2).unwrap();
        let mut junk = Vec::new();
        r.process(&sine(44_100, 3_000), &mut junk);
        r.reset();
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.flush(&mut out);
        assert!(out == resample(&input, 44_100, 48_000, 5_000));
    }
}
