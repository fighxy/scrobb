//! Адаптер порта `AudioOutput` на [cpal]: WASAPI (Windows), CoreAudio (macOS,
//! iOS), AAudio (Android), ALSA/PipeWire/PulseAudio (Linux).
//!
//! `cpal::Stream` можно передавать между потоками не на всех платформах,
//! поэтому поток вывода живёт в отдельном служебном потоке, а адаптер
//! управляет им командами. Колбэк устройства не выделяет память: сэмплы
//! движка идут через заранее выделенный буфер.
//!
//! Ограничения первой версии:
//! - частота вывода равна частоте трека; если устройство её не поддерживает,
//!   `open` возвращает ошибку (ресемплинг — следующий шаг);
//! - если у устройства меньше каналов, чем у трека, берутся первые каналы
//!   (даунмикс — позже); моно размножается на все каналы устройства.
//!
//! На Linux для сборки нужны заголовки ALSA (`libasound2-dev`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, SupportedStreamConfig};
use player_ports::{AudioOutput, PcmFormat, PcmSource, PortError};

/// Сколько кадров движка обрабатывать за один проход колбэка.
const SCRATCH_FRAMES: usize = 4096;

enum Control {
    Open(PcmFormat, Box<dyn PcmSource>, Sender<Result<(), PortError>>),
    Start(Sender<Result<(), PortError>>),
    Stop,
    Shutdown,
}

/// Вывод на устройство по умолчанию через cpal.
pub struct CpalOutput {
    control: Sender<Control>,
    thread: Option<JoinHandle<()>>,
    errors: Arc<AtomicU64>,
}

impl CpalOutput {
    /// Создать адаптер; устройство открывается в [`AudioOutput::open`].
    pub fn new() -> Self {
        let (control, rx) = mpsc::channel();
        let errors = Arc::new(AtomicU64::new(0));
        let thread_errors = errors.clone();
        let thread = thread::Builder::new()
            .name("player-output-cpal".into())
            .spawn(move || run(rx, thread_errors))
            .expect("служебный поток вывода запускается");
        Self {
            control,
            thread: Some(thread),
            errors,
        }
    }

    /// Сколько ошибок сообщило устройство за время работы (например, отключили наушники).
    pub fn stream_errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    fn request(
        &self,
        make: impl FnOnce(Sender<Result<(), PortError>>) -> Control,
    ) -> Result<(), PortError> {
        let (reply, rx) = mpsc::channel();
        self.control
            .send(make(reply))
            .map_err(|_| PortError::Other("служебный поток вывода остановлен".into()))?;
        rx.recv()
            .map_err(|_| PortError::Other("служебный поток вывода остановлен".into()))?
    }
}

impl Default for CpalOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioOutput for CpalOutput {
    fn open(&mut self, format: PcmFormat, source: Box<dyn PcmSource>) -> Result<(), PortError> {
        self.request(|reply| Control::Open(format, source, reply))
    }

    fn start(&mut self) -> Result<(), PortError> {
        self.request(Control::Start)
    }

    fn stop(&mut self) {
        let _ = self.control.send(Control::Stop);
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(rx: Receiver<Control>, errors: Arc<AtomicU64>) {
    let mut stream: Option<cpal::Stream> = None;
    for control in rx {
        match control {
            Control::Open(format, source, reply) => {
                stream = None;
                let opened = build(format, source, errors.clone()).map(|s| stream = Some(s));
                let _ = reply.send(opened);
            }
            Control::Start(reply) => {
                let started = match &stream {
                    Some(s) => s.play().map_err(other),
                    None => Err(PortError::Other("вывод не открыт".into())),
                };
                let _ = reply.send(started);
            }
            Control::Stop => stream = None,
            Control::Shutdown => break,
        }
    }
}

fn other(e: impl std::fmt::Display) -> PortError {
    PortError::Other(e.to_string())
}

fn build(
    format: PcmFormat,
    source: Box<dyn PcmSource>,
    errors: Arc<AtomicU64>,
) -> Result<cpal::Stream, PortError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| PortError::Other("нет устройства вывода".into()))?;
    let config = choose_config(&device, format)?;
    match config.sample_format() {
        SampleFormat::F32 => build_typed::<f32>(&device, &config, format, source, errors),
        SampleFormat::F64 => build_typed::<f64>(&device, &config, format, source, errors),
        SampleFormat::I16 => build_typed::<i16>(&device, &config, format, source, errors),
        SampleFormat::I32 => build_typed::<i32>(&device, &config, format, source, errors),
        SampleFormat::U16 => build_typed::<u16>(&device, &config, format, source, errors),
        SampleFormat::I8 => build_typed::<i8>(&device, &config, format, source, errors),
        SampleFormat::U8 => build_typed::<u8>(&device, &config, format, source, errors),
        other => Err(PortError::Other(format!(
            "формат сэмплов {other} не поддерживается"
        ))),
    }
}

/// Конфигурация устройства с частотой трека: сначала то же число каналов, затем `f32`.
fn choose_config(
    device: &cpal::Device,
    format: PcmFormat,
) -> Result<SupportedStreamConfig, PortError> {
    let rate = format.sample_rate;
    device
        .supported_output_configs()
        .map_err(other)?
        .filter(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
        .max_by_key(|r| {
            (
                r.channels() == format.channels,
                r.sample_format() == SampleFormat::F32,
                r.channels() >= format.channels,
            )
        })
        .map(|r| r.with_sample_rate(rate))
        .ok_or_else(|| {
            PortError::Other(format!(
                "устройство не поддерживает {rate} Гц, а ресемплинга пока нет"
            ))
        })
}

fn build_typed<T>(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    format: PcmFormat,
    mut source: Box<dyn PcmSource>,
    errors: Arc<AtomicU64>,
) -> Result<cpal::Stream, PortError>
where
    T: SizedSample + FromSample<f32>,
{
    let src_ch = usize::from(format.channels.max(1));
    let dev_ch = usize::from(config.channels().max(1));
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * src_ch];
    let data = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        let frames = out.len() / dev_ch;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SCRATCH_FRAMES);
            let buf = &mut scratch[..n * src_ch];
            source.fill(buf);
            let dst = &mut out[done * dev_ch..(done + n) * dev_ch];
            for (src, dst) in buf.chunks_exact(src_ch).zip(dst.chunks_exact_mut(dev_ch)) {
                for (d, sample) in dst.iter_mut().enumerate() {
                    let s = match src_ch {
                        1 => src[0],
                        _ => src.get(d).copied().unwrap_or(0.0),
                    };
                    *sample = T::from_sample(s);
                }
            }
            done += n;
        }
    };
    let error = move |_| {
        errors.fetch_add(1, Ordering::Relaxed);
    };
    device
        .build_output_stream::<T, _, _>(config.config(), data, error, None)
        .map_err(other)
}
