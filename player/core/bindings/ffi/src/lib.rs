//! Привязки ядра для нативных оболочек (заготовка).
//!
//! План (`docs/player-architecture.md` §5.1, вариант B): UniFFI генерирует
//! Swift для iOS, iPadOS и macOS и Kotlin (через JNA) для Android и
//! JVM-десктопа на Windows и Linux. Наружу выходит только фасад
//! `player-core`: команды, снимки состояния, события, возможности;
//! [`player_core::Observer`] и порты становятся callback-интерфейсами,
//! которые реализуют оболочки.

pub use player_core::{Capabilities, Command, Core, CoreConfig, CoreError, Event, Observer, Ports};
