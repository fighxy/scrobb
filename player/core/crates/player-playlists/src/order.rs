//! Дробные ключи порядка: строка из цифр base-62 (`0-9A-Za-z`, по возрастанию
//! ASCII) — это дробь `0.d₁d₂…`. Элементы сортируются по строке, а новый ключ
//! всегда можно взять между двумя соседями, не трогая остальных. Поэтому
//! вставка и перестановка — одна запись, а правки с двух устройств
//! сливаются без перенумерации (`docs/playlists.md`).
//!
//! Ключ никогда не кончается на `0`: иначе у одной дроби было бы две записи
//! («V» и «V0»), и между ними не нашлось бы места.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: u8 = 62;
/// До стольких цифр ключи равномерного ряда считаются в `u128` (62²¹ < 2¹²⁸).
const MAX_WIDE: usize = 21;

fn digit(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'A'..=b'Z' => c - b'A' + 10,
        b'a'..=b'z' => c - b'a' + 36,
        _ => 0,
    }
}

fn digits(key: &str) -> Vec<u8> {
    key.bytes().map(digit).collect()
}

fn encode(digits: &[u8]) -> String {
    let end = digits.iter().rposition(|&d| d != 0).map_or(0, |i| i + 1);
    digits[..end]
        .iter()
        .map(|&d| DIGITS[usize::from(d)] as char)
        .collect()
}

/// Ключ строго между `before` и `after`; `None` — край списка.
/// Требует `before < after`.
pub fn between(before: Option<&str>, after: Option<&str>) -> String {
    let a = before.map(digits).unwrap_or_default();
    let mut b = after.map(digits);
    debug_assert!(
        b.as_ref().is_none_or(|b| encode(&a) < encode(b)),
        "ключи порядка перепутаны"
    );
    let mut out = Vec::new();
    for i in 0.. {
        let da = a.get(i).copied().unwrap_or(0);
        // Верхняя граница за краем — «единица», то есть цифра 62.
        let db = b.as_ref().map_or(BASE, |b| b.get(i).copied().unwrap_or(0));
        if da == db {
            out.push(da);
            continue;
        }
        if db - da > 1 {
            out.push(da + (db - da) / 2);
            break;
        }
        // Соседние цифры: берём цифру `a`, дальше верхняя граница не мешает.
        out.push(da);
        b = None;
    }
    encode(&out)
}

/// `n` ключей подряд строго между `before` и `after`, равномерно: для
/// импорта и добавления пачкой, чтобы ключи оставались короткими.
pub fn spread(before: Option<&str>, after: Option<&str>, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let a = before.map(digits).unwrap_or_default();
    let b = after.map(digits);
    let shortest = a.len().max(b.as_ref().map_or(0, Vec::len)).max(1);
    for width in shortest..=MAX_WIDE {
        let lo = wide(&a, width);
        let hi = b
            .as_ref()
            .map_or(62u128.pow(width as u32), |b| wide(b, width));
        let gap = hi - lo;
        if gap > n as u128 {
            let step = gap / (n as u128 + 1);
            return (1..=n as u128)
                .map(|k| encode(&narrow(lo + step * k, width)))
                .collect();
        }
    }
    // Очень глубоко между соседями — по одному.
    let mut keys = Vec::with_capacity(n);
    let mut prev = before.map(str::to_owned);
    for _ in 0..n {
        let key = between(prev.as_deref(), after);
        prev = Some(key.clone());
        keys.push(key);
    }
    keys
}

/// Ключ как целое число `width` цифр (недостающие — нули).
fn wide(digits: &[u8], width: usize) -> u128 {
    (0..width).fold(0, |acc, i| {
        acc * 62 + u128::from(digits.get(i).copied().unwrap_or(0))
    })
}

fn narrow(mut value: u128, width: usize) -> Vec<u8> {
    let mut out = vec![0; width];
    for d in out.iter_mut().rev() {
        *d = (value % 62) as u8;
        value /= 62;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Простой ГПСЧ без зависимостей: тестам нужна только повторяемость.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }

    fn assert_sorted(keys: &[String]) {
        for w in keys.windows(2) {
            assert!(w[0] < w[1], "{} !< {}", w[0], w[1]);
        }
        assert!(keys.iter().all(|k| !k.is_empty() && !k.ends_with('0')));
    }

    #[test]
    fn between_edges_and_neighbours() {
        assert_eq!(between(None, None), "V");
        let first = between(None, Some("V"));
        let last = between(Some("V"), None);
        assert_sorted(&[first.clone(), "V".into(), last.clone()]);
        // Соседние цифры: ключ уходит на разряд глубже.
        let deep = between(Some("V"), Some("W"));
        assert_sorted(&["V".into(), deep.clone(), "W".into()]);
        assert_eq!(deep, "VV");
        assert_sorted(&["Vz".into(), between(Some("Vz"), Some("W")), "W".into()]);
        assert_sorted(&["V5".into(), between(Some("V5"), Some("W")), "W".into()]);
        assert_sorted(&["1".into(), between(Some("1"), Some("11")), "11".into()]);
    }

    #[test]
    fn random_inserts_keep_order_and_stay_unique() {
        let mut rng = Lcg(42);
        let mut keys: Vec<String> = Vec::new();
        for _ in 0..3000 {
            let at = rng.below(keys.len() + 1);
            let before = at.checked_sub(1).map(|i| keys[i].as_str());
            let after = keys.get(at).map(String::as_str);
            let key = between(before, after);
            keys.insert(at, key);
        }
        assert_sorted(&keys);
        let longest = keys.iter().map(String::len).max().unwrap();
        assert!(longest < 40, "ключи не разрастаются: {longest}");
    }

    #[test]
    fn spread_is_even_short_and_fits_between() {
        let keys = spread(None, None, 10_000);
        assert_eq!(keys.len(), 10_000);
        assert_sorted(&keys);
        assert!(keys.iter().all(|k| k.len() <= 3));
        let middle = spread(Some(&keys[10]), Some(&keys[11]), 500);
        assert_sorted(&[vec![keys[10].clone()], middle, vec![keys[11].clone()]].concat());
        assert!(spread(Some("V"), None, 0).is_empty());
        // Хвост после последнего — тоже по порядку.
        let tail = spread(Some(keys.last().unwrap()), None, 3);
        assert!(tail.iter().all(|k| k > keys.last().unwrap()));
    }

    #[test]
    fn spread_falls_back_when_the_gap_is_very_deep() {
        let a = "V".repeat(30);
        let b = format!("{}W", "V".repeat(29));
        let keys = spread(Some(&a), Some(&b), 5);
        assert_sorted(&[vec![a.clone()], keys, vec![b]].concat());
    }
}
