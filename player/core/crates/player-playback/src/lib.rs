//! Очередь воспроизведения и переходы по ней.
//!
//! Чистая логика: собирается под wasm. Перемешивание, несколько очередей
//! (как в Musicolet) и восстановление позиции — следующие шаги.

use player_domain::{RepeatMode, TrackId};

/// Очередь воспроизведения.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    items: Vec<TrackId>,
    current: Option<usize>,
    /// Режим повтора.
    pub repeat: RepeatMode,
}

impl Queue {
    /// Очередь из треков; играть начнёт с `start`.
    pub fn new(items: Vec<TrackId>, start: usize) -> Self {
        let current = (start < items.len()).then_some(start);
        Self {
            items,
            current,
            repeat: RepeatMode::Off,
        }
    }

    /// Текущий трек.
    pub fn current(&self) -> Option<TrackId> {
        self.current.map(|i| self.items[i])
    }

    /// Позиция текущего трека и длина очереди.
    pub fn position(&self) -> Option<(usize, usize)> {
        self.current.map(|i| (i, self.items.len()))
    }

    /// Перейти к следующему треку после окончания текущего или по кнопке.
    pub fn advance(&mut self) -> Option<TrackId> {
        let i = self.current?;
        self.current = match self.repeat {
            RepeatMode::One => Some(i),
            _ if i + 1 < self.items.len() => Some(i + 1),
            RepeatMode::All => Some(0),
            RepeatMode::Off => None,
        };
        self.current()
    }

    /// Предыдущий трек.
    pub fn back(&mut self) -> Option<TrackId> {
        let i = self.current?;
        self.current = Some(match (i, self.repeat) {
            (0, RepeatMode::All) => self.items.len() - 1,
            (0, _) => 0,
            _ => i - 1,
        });
        self.current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(repeat: RepeatMode) -> Queue {
        Queue {
            repeat,
            ..Queue::new(vec![TrackId(1), TrackId(2)], 0)
        }
    }

    #[test]
    fn stops_at_end_without_repeat() {
        let mut q = queue(RepeatMode::Off);
        assert_eq!(q.advance(), Some(TrackId(2)));
        assert_eq!(q.advance(), None);
    }

    #[test]
    fn wraps_with_repeat_all() {
        let mut q = queue(RepeatMode::All);
        q.advance();
        assert_eq!(q.advance(), Some(TrackId(1)));
        assert_eq!(q.back(), Some(TrackId(2)));
    }
}
