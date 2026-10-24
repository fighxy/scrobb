//! Декодер одного трека на Symphonia.
//!
//! Задержку и паддинг кодера (gapless) Symphonia обрезает сама там, где формат
//! это поддерживает: FLAC, MP3 с заголовком LAME, Vorbis, WAV, ALAC. Для AAC в
//! MP4 обрезки пока нет — это ограничение Symphonia 0.6.

use std::io::{Read, Seek, SeekFrom};

use player_ports::{MediaStream, PcmFormat};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

/// Ошибка открытия или декодирования.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(e: impl std::fmt::Display) -> DecodeError {
    DecodeError(e.to_string())
}

/// Поток порта `FileAccess` как источник для Symphonia.
struct Source {
    inner: Box<dyn MediaStream>,
    len: Option<u64>,
}

impl Source {
    fn new(mut inner: Box<dyn MediaStream>) -> Self {
        let len = inner.seek(SeekFrom::End(0)).ok();
        let rewound = inner.seek(SeekFrom::Start(0)).is_ok();
        Self {
            inner,
            len: len.filter(|_| rewound),
        }
    }
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for Source {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl MediaSource for Source {
    fn is_seekable(&self) -> bool {
        self.len.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

/// Открытый трек: демультиплексор, декодер и формат PCM на выходе.
pub struct TrackDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    format: PcmFormat,
    /// Сколько кадров отбросить после перемотки, чтобы попасть точно в позицию.
    skip_frames: u64,
}

impl TrackDecoder {
    /// Открыть поток; `extension` (`flac`, `mp3`…) ускоряет определение формата.
    pub fn open(
        stream: Box<dyn MediaStream>,
        extension: Option<&str>,
    ) -> Result<Self, DecodeError> {
        let mss = MediaSourceStream::new(
            Box::new(Source::new(stream)),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }
        let reader = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(err)?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError("в файле нет звуковой дорожки".into()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| DecodeError("неизвестный кодек".into()))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(err)?;
        let params = decoder.codec_params();
        let format = PcmFormat {
            sample_rate: params
                .sample_rate
                .ok_or_else(|| DecodeError("неизвестна частота".into()))?,
            channels: params
                .channels
                .as_ref()
                .map(|c| c.count() as u16)
                .ok_or_else(|| DecodeError("неизвестно число каналов".into()))?,
        };
        Ok(Self {
            reader,
            decoder,
            track_id,
            time_base,
            format,
            skip_frames: 0,
        })
    }

    /// Формат PCM на выходе.
    pub fn format(&self) -> PcmFormat {
        self.format
    }

    /// Декодировать следующий пакет и дописать сэмплы в `out` (чередование каналов).
    /// `Ok(false)` — трек закончился.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool, DecodeError> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(false);
                }
                Err(e) => return Err(err(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(buf) => buf,
                // Повреждённый пакет пропускаем, как делают все плееры.
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(e) => return Err(err(e)),
            };
            if buf.spec().rate() != self.format.sample_rate
                || buf.spec().channels().count() != usize::from(self.format.channels)
            {
                return Err(DecodeError("формат изменился посреди трека".into()));
            }
            let channels = usize::from(self.format.channels);
            let frames = buf.frames();
            let skip = (self.skip_frames as usize).min(frames);
            self.skip_frames -= skip as u64;
            if skip == frames {
                continue;
            }
            let start = out.len();
            out.resize(start + frames * channels, 0.0);
            buf.copy_to_slice_interleaved::<f32, _>(&mut out[start..]);
            out.drain(start..start + skip * channels);
            return Ok(true);
        }
    }

    /// Точная перемотка: возвращает позицию в кадрах, с которой продолжится вывод.
    pub fn seek(&mut self, position_ms: u64) -> Result<u64, DecodeError> {
        let rate = u64::from(self.format.sample_rate);
        let target_frame = position_ms * rate / 1000;
        let time = Time::try_from_secs_f64(target_frame as f64 / rate as f64)
            .ok_or_else(|| DecodeError("неверная позиция".into()))?;
        let seeked = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(err)?;
        self.decoder.reset();
        let actual_frame = self.ts_to_frames(seeked.actual_ts);
        let required_frame = self.ts_to_frames(seeked.required_ts);
        self.skip_frames = required_frame.saturating_sub(actual_frame);
        Ok(required_frame)
    }

    /// Метка времени дорожки → кадры, с округлением до ближайшего.
    fn ts_to_frames(&self, ts: Timestamp) -> u64 {
        let ts = u128::try_from(ts.get().max(0)).unwrap_or(0);
        let Some(tb) = self.time_base else {
            return ts as u64;
        };
        let num = ts * u128::from(tb.numer.get()) * u128::from(self.format.sample_rate);
        let den = u128::from(tb.denom.get());
        ((num + den / 2) / den) as u64
    }
}
