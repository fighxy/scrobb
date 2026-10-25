//! Гибридные логические часы и регистр «последняя запись побеждает».

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Гибридные логические часы (HLC): упорядочивают правки с разных устройств
/// даже при расхождении системных часов. Метка всегда больше всех, что
/// устройство уже видело, и близка к настоящему времени.
///
/// В JSON и в базах — строка `<мс Unix, 13 цифр>-<счётчик, 10 цифр>-<узел, 8 hex>`,
/// например `1761384000000-0000000002-1a2b3c4d`. Ширина постоянная, поэтому
/// строки сравниваются так же, как метки: сервер может брать максимум в SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hlc {
    /// Физическое время, миллисекунды Unix.
    pub wall_ms: u64,
    /// Счётчик для событий в одну миллисекунду.
    pub counter: u32,
    /// Идентификатор устройства — разрешает ничьи.
    pub node: u32,
}

impl Hlc {
    /// Часы устройства `node`, ещё не видевшие ни одной правки.
    pub fn new(node: u32) -> Self {
        Self {
            wall_ms: 0,
            counter: 0,
            node,
        }
    }

    /// Следующая метка для локальной правки.
    #[must_use]
    pub fn tick(self, now_ms: u64) -> Self {
        if now_ms > self.wall_ms {
            Self {
                wall_ms: now_ms,
                counter: 0,
                node: self.node,
            }
        } else {
            self.after(self.wall_ms, self.counter)
        }
    }

    /// Часы после того, как устройство увидело чужую метку `remote`: следующая
    /// локальная правка будет позже неё, даже если здесь часы отстают.
    #[must_use]
    pub fn observe(self, remote: Hlc, now_ms: u64) -> Self {
        let wall_ms = self.wall_ms.max(remote.wall_ms).max(now_ms);
        match (wall_ms == self.wall_ms, wall_ms == remote.wall_ms) {
            (true, true) => self.after(wall_ms, self.counter.max(remote.counter)),
            (true, false) => self.after(wall_ms, self.counter),
            (false, true) => self.after(wall_ms, remote.counter),
            (false, false) => Self {
                wall_ms,
                counter: 0,
                node: self.node,
            },
        }
    }

    /// Метка этого узла сразу после `(wall_ms, counter)`. Счётчик кончился
    /// (чужая метка бывает любой) — следующая миллисекунда.
    fn after(self, wall_ms: u64, counter: u32) -> Self {
        let (wall_ms, counter) = match counter.checked_add(1) {
            Some(c) => (wall_ms, c),
            None => (wall_ms + 1, 0),
        };
        Self {
            wall_ms,
            counter,
            node: self.node,
        }
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:013}-{:010}-{:08x}",
            self.wall_ms, self.counter, self.node
        )
    }
}

/// Строка не похожа на метку HLC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadHlc(pub String);

impl fmt::Display for BadHlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "не метка HLC: {:?}", self.0)
    }
}

impl std::error::Error for BadHlc {}

impl FromStr for Hlc {
    type Err = BadHlc;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadHlc(s.to_owned());
        let mut parts = s.split('-');
        let (Some(wall), Some(counter), Some(node), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        // Ширина обязательна: иначе строки перестанут сравниваться как метки.
        if wall.len() < 13 || counter.len() != 10 || node.len() != 8 {
            return Err(bad());
        }
        let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
        if !digits(wall) || !digits(counter) {
            return Err(bad());
        }
        Ok(Self {
            wall_ms: wall.parse().map_err(|_| bad())?,
            counter: counter.parse().map_err(|_| bad())?,
            node: u32::from_str_radix(node, 16).map_err(|_| bad())?,
        })
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Регистр «последняя запись побеждает».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lww<T> {
    /// Значение.
    pub value: T,
    /// Когда и где его записали.
    pub stamp: Hlc,
}

impl<T: Ord> Lww<T> {
    /// Слияние с правкой с другого устройства или сервера; `true` — значение
    /// сменилось. Побеждает большая метка; при равных метках (так не бывает,
    /// но слияние должно сходиться и тогда) — большее значение.
    pub fn merge(&mut self, other: Lww<T>) -> bool {
        let newer = (other.stamp, &other.value) > (self.stamp, &self.value);
        if newer {
            *self = other;
        }
        newer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hlc(wall_ms: u64, counter: u32, node: u32) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            node,
        }
    }

    #[test]
    fn tick_moves_forward_even_when_wall_clock_does_not() {
        let a = Hlc::new(7).tick(100);
        assert_eq!(a, hlc(100, 0, 7));
        let b = a.tick(100);
        let c = b.tick(50);
        assert_eq!((b, c), (hlc(100, 1, 7), hlc(100, 2, 7)));
        assert_eq!(c.tick(200), hlc(200, 0, 7));
    }

    #[test]
    fn observe_puts_next_edit_after_remote_one() {
        let local = hlc(100, 3, 1);
        let ahead = hlc(500, 4, 2);
        let seen = local.observe(ahead, 120);
        assert_eq!(seen, hlc(500, 5, 1), "чужие часы впереди");
        assert!(seen.tick(130) > ahead);
        assert_eq!(local.observe(hlc(90, 9, 2), 100), hlc(100, 4, 1));
        assert_eq!(local.observe(hlc(100, 8, 2), 100), hlc(100, 9, 1));
        assert_eq!(local.observe(hlc(90, 9, 2), 300), hlc(300, 0, 1));
        assert_eq!(
            local.observe(hlc(100, u32::MAX, 2), 100),
            hlc(101, 0, 1),
            "счётчик кончился — следующая миллисекунда"
        );
    }

    #[test]
    fn text_form_sorts_like_stamps() {
        let stamps = [
            hlc(0, 0, 0),
            hlc(1_761_384_000_000, 0, 0xffff_ffff),
            hlc(1_761_384_000_000, 2, 0x1a2b_3c4d),
            hlc(1_761_384_000_000, 10, 0),
            hlc(1_761_384_000_001, 0, 0),
        ];
        let texts: Vec<String> = stamps.iter().map(Hlc::to_string).collect();
        assert_eq!(texts[2], "1761384000000-0000000002-1a2b3c4d");
        let mut sorted = texts.clone();
        sorted.sort();
        assert_eq!(sorted, texts);
        for (s, t) in stamps.iter().zip(&texts) {
            assert_eq!(t.parse::<Hlc>().unwrap(), *s);
        }
        for bad in [
            "",
            "1761384000000-2-1a2b3c4d",
            "1761384000000-0000000002-1a2b3c4",
            "176138400000-0000000002-1a2b3c4d",
            "1761384000000-000000000x-1a2b3c4d",
            "1761384000000-0000000002-1a2b3c4d-1",
            "+761384000000-0000000002-1a2b3c4d",
        ] {
            assert!(bad.parse::<Hlc>().is_err(), "{bad:?}");
        }
        let json = serde_json::to_string(&stamps[2]).unwrap();
        assert_eq!(json, "\"1761384000000-0000000002-1a2b3c4d\"");
        assert_eq!(serde_json::from_str::<Hlc>(&json).unwrap(), stamps[2]);
    }

    #[test]
    fn later_stamp_wins_regardless_of_arrival_order() {
        let a = hlc(100, 0, 1);
        let b = a.tick(100);
        let mut reg = Lww { value: 5, stamp: b };
        assert!(!reg.merge(Lww { value: 9, stamp: a }));
        assert_eq!(reg.value, 5);
        let mut other = Lww { value: 9, stamp: a };
        assert!(other.merge(Lww { value: 5, stamp: b }));
        assert_eq!(other, reg);
        let mut tie = Lww { value: 1, stamp: a };
        tie.merge(Lww { value: 2, stamp: a });
        assert_eq!(tie.value, 2, "ничья меток — большее значение");
    }
}
