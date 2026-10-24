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

    /// Сколько треков в очереди.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Пуста ли очередь.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Куда перейти после позиции `i` с учётом режима повтора.
    fn next_index(&self, i: usize) -> Option<usize> {
        match self.repeat {
            RepeatMode::One => Some(i),
            _ if i + 1 < self.items.len() => Some(i + 1),
            RepeatMode::All => Some(0),
            RepeatMode::Off => None,
        }
    }

    /// Перейти к следующему треку после окончания текущего или по кнопке.
    pub fn advance(&mut self) -> Option<TrackId> {
        let i = self.current?;
        self.current = self.next_index(i);
        self.current()
    }

    /// Трек через `steps` переходов вперёд, не сдвигая очередь:
    /// что передать движку для перехода без паузы.
    pub fn peek(&self, steps: usize) -> Option<TrackId> {
        let mut i = self.current?;
        for _ in 0..steps {
            i = self.next_index(i)?;
        }
        Some(self.items[i])
    }

    /// Сколько переходов вперёд до трека `id`, начиная с 1; `None` — впереди его нет.
    pub fn steps_to(&self, id: TrackId) -> Option<usize> {
        (1..=self.items.len()).find(|&steps| self.peek(steps) == Some(id))
    }

    /// Движок сам перешёл к треку `id` (без паузы): сдвинуть очередь до него.
    /// Если `id` — текущий или его нет впереди, очередь не меняется.
    pub fn advance_to(&mut self, id: TrackId) -> bool {
        if self.current() == Some(id) {
            return true;
        }
        match self.steps_to(id) {
            Some(steps) => {
                for _ in 0..steps {
                    self.advance();
                }
                true
            }
            None => false,
        }
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

    #[test]
    fn peek_does_not_move_and_respects_repeat() {
        let q = queue(RepeatMode::All);
        assert_eq!(q.peek(1), Some(TrackId(2)));
        assert_eq!(q.peek(2), Some(TrackId(1)));
        assert_eq!(q.current(), Some(TrackId(1)));
        assert_eq!(queue(RepeatMode::Off).peek(2), None);
        assert_eq!(queue(RepeatMode::One).peek(5), Some(TrackId(1)));
    }

    #[test]
    fn advance_to_skips_over_tracks() {
        let mut q = Queue::new(vec![TrackId(1), TrackId(2), TrackId(3)], 0);
        assert!(q.advance_to(TrackId(3)));
        assert_eq!(q.position(), Some((2, 3)));
        assert!(!q.advance_to(TrackId(1)));
        assert_eq!(q.current(), Some(TrackId(3)));
    }
}
