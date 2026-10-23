//! Метаданные (заготовка): MusicBrainz, AcoustID-отпечатки, обложки,
//! синхронные тексты, исправление «кракозябр» cp1251 в старых тегах
//! (`docs/local-players-research.md` §4, п. 5).
//!
//! Сеть — через порт `HttpTransport`; сами источники описываются данными,
//! чтобы новые можно было добавлять плагинами.

/// Откуда берутся метаданные.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataSource {
    /// Теги файла.
    Tags,
    /// MusicBrainz.
    MusicBrainz,
    /// AcoustID по отпечатку звука.
    AcoustId,
    /// Плагин.
    Plugin,
}
