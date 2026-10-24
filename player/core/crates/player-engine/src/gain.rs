//! ReplayGain: множитель громкости трека по тегам.

use player_domain::ReplayGain;

/// Режим ReplayGain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReplayGainMode {
    /// Выключен.
    Off,
    /// По трекам; если нет значения трека — по альбому.
    #[default]
    Track,
    /// По альбомам; если нет значения альбома — по треку.
    Album,
}

/// Настройки ReplayGain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainConfig {
    /// Режим.
    pub mode: ReplayGainMode,
    /// Предусиление, дБ.
    pub preamp_db: f32,
    /// Не поднимать громкость выше пика (защита от клиппинга).
    pub prevent_clipping: bool,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self {
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }
}

/// Линейный множитель для трека. Без значений в тегах — 1.0 (громкость не меняется).
pub fn factor(rg: &ReplayGain, config: &GainConfig) -> f32 {
    let (gain, peak) = match config.mode {
        ReplayGainMode::Off => return 1.0,
        ReplayGainMode::Track => (
            rg.track_gain_db.or(rg.album_gain_db),
            rg.track_peak.or(rg.album_peak),
        ),
        ReplayGainMode::Album => (
            rg.album_gain_db.or(rg.track_gain_db),
            rg.album_peak.or(rg.track_peak),
        ),
    };
    let Some(gain) = gain else {
        return 1.0;
    };
    let linear = db_to_linear(gain + config.preamp_db);
    match peak {
        Some(peak) if config.prevent_clipping && peak > 0.0 => linear.min(1.0 / peak),
        _ => linear,
    }
}

/// Децибелы → линейный множитель.
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rg(track: Option<f32>, peak: Option<f32>, album: Option<f32>) -> ReplayGain {
        ReplayGain {
            track_gain_db: track,
            track_peak: peak,
            album_gain_db: album,
            album_peak: None,
        }
    }

    #[test]
    fn off_and_missing_tags_keep_volume() {
        let off = GainConfig {
            mode: ReplayGainMode::Off,
            ..GainConfig::default()
        };
        assert_eq!(factor(&rg(Some(-6.0), None, None), &off), 1.0);
        assert_eq!(factor(&ReplayGain::default(), &GainConfig::default()), 1.0);
    }

    #[test]
    fn applies_gain_with_fallback_to_album() {
        let f = factor(&rg(None, None, Some(-6.0)), &GainConfig::default());
        assert!((f - 0.501_187).abs() < 1e-5);
    }

    #[test]
    fn clipping_protection_limits_boost_by_peak() {
        let boosted = rg(Some(6.0), Some(0.8), None);
        assert_eq!(factor(&boosted, &GainConfig::default()), 1.25);
        let unprotected = GainConfig {
            prevent_clipping: false,
            ..GainConfig::default()
        };
        assert!((factor(&boosted, &unprotected) - 1.995_262).abs() < 1e-5);
    }
}
