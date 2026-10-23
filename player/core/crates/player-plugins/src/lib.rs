//! Хост плагинов (заготовка): WebAssembly-компоненты в песочнице с явными
//! разрешениями. Не участвуют в обработке звука в реальном времени. На iOS —
//! только встроенные в сборку плагины и интерпретатор без JIT
//! (`docs/player-architecture.md` §6).

/// Что умеет плагин.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    /// Источник метаданных.
    Metadata,
    /// Источник текстов песен.
    Lyrics,
    /// Ещё одна цель скробблинга.
    ScrobbleTarget,
    /// Импорт из другого плеера.
    Importer,
}

/// Разрешение плагина.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// Сеть, только к этому домену.
    Network(String),
    /// Чтение библиотеки.
    ReadLibrary,
}

/// Описание плагина.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    /// Идентификатор.
    pub id: String,
    /// Тип.
    pub kind: PluginKind,
    /// Разрешения, которые подтверждает пользователь.
    pub permissions: Vec<Permission>,
}
