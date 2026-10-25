//! Обложки для окна: схема `cover://localhost/<id>/<s|l>` (на Windows —
//! `http://cover.localhost/…`) и цвет обложки для фона.
//!
//! Картинку находит ядро (`player_library::covers`: встроенная или из папки),
//! здесь — уменьшение до квадрата, JPEG и кэш в памяти. Окно просит обложку
//! у первого трека альбома, поэтому на альбом выходит одна выборка, а
//! одинаковый адрес WebView берёт из своего кэша.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, RgbImage};
use player_domain::TrackId;
use player_library::TrackStore;
use player_ports::FileAccess;
use tauri::http::{Response, StatusCode, header};

/// Схема адресов обложек.
pub const SCHEME: &str = "cover";
/// Потоков, которые готовят обложки: столько картинок раскодируется одновременно.
const WORKERS: usize = 3;
const JPEG_QUALITY: u8 = 85;

/// Размер обложки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    /// Строки списка и плитки: 128 px.
    Small,
    /// «Сейчас играет» и шапка альбома: 640 px.
    Large,
}

impl Size {
    fn px(self) -> u32 {
        match self {
            Size::Small => 128,
            Size::Large => 640,
        }
    }
}

/// Готовые обложки и цвета; `None` в кэше — «обложки нет», чтобы не искать снова.
pub struct Covers {
    tracks: Arc<dyn TrackStore>,
    files: Arc<dyn FileAccess>,
    cache: Mutex<Cache>,
    jobs: Mutex<mpsc::Sender<Job>>,
}

type Job = Box<dyn FnOnce() + Send>;
type Image = Option<Arc<Vec<u8>>>;

struct Cache {
    small: Lru<TrackId, Image>,
    large: Lru<TrackId, Image>,
    colors: Lru<TrackId, Option<String>>,
}

impl Covers {
    /// Сервис обложек с пулом потоков `cover-worker`.
    pub fn new(tracks: Arc<dyn TrackStore>, files: Arc<dyn FileAccess>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let rx = rx.clone();
            thread::Builder::new()
                .name("cover-worker".into())
                .spawn(move || {
                    while let Ok(job) = { lock(&rx).recv() } {
                        job();
                    }
                })
                .expect("поток обложек запускается");
        }
        Arc::new(Self {
            tracks,
            files,
            cache: Mutex::new(Cache {
                small: Lru::new(512),
                large: Lru::new(32),
                colors: Lru::new(1024),
            }),
            jobs: Mutex::new(tx),
        })
    }

    /// Выполнить в пуле обложек, не занимая поток окна.
    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        let _ = lock(&self.jobs).send(Box::new(job));
    }

    /// Обложка трека в JPEG или `None`, если её нет.
    pub fn image(&self, id: TrackId, size: Size) -> Image {
        if let Some(hit) = self.cached(id, size) {
            return hit;
        }
        let image = self
            .source(id)
            .map(|img| Arc::new(jpeg(&square(&img, size.px()))));
        let mut cache = lock(&self.cache);
        match size {
            Size::Small => cache.small.put(id, image.clone()),
            Size::Large => cache.large.put(id, image.clone()),
        }
        image
    }

    /// Цвет обложки для фона под белым текстом (`#rrggbb`) или `None`.
    pub fn color(&self, id: TrackId) -> Option<String> {
        if let Some(hit) = lock(&self.cache).colors.get(&id) {
            return hit;
        }
        let color = self.source(id).map(|img| hex(accent(&square(&img, 32))));
        lock(&self.cache).colors.put(id, color.clone());
        color
    }

    /// Забыть всё: после «Обновить» обложки могли смениться.
    pub fn clear(&self) {
        let mut cache = lock(&self.cache);
        cache.small.clear();
        cache.large.clear();
        cache.colors.clear();
    }

    /// Ответ на запрос `cover://localhost/<id>/<s|l>`.
    pub fn respond(&self, path: &str) -> Response<Vec<u8>> {
        let Some((id, size)) = parse_path(path) else {
            return status(StatusCode::BAD_REQUEST);
        };
        match self.image(id, size) {
            Some(jpeg) => Response::builder()
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "max-age=600")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(jpeg.as_ref().clone())
                .expect("заголовки корректны"),
            None => status(StatusCode::NOT_FOUND),
        }
    }

    fn cached(&self, id: TrackId, size: Size) -> Option<Image> {
        let mut cache = lock(&self.cache);
        match size {
            Size::Small => cache.small.get(&id),
            Size::Large => cache.large.get(&id),
        }
    }

    /// Раскодированная картинка обложки трека.
    fn source(&self, id: TrackId) -> Option<DynamicImage> {
        let track = self.tracks.track(id)?;
        let picture = player_library::covers::find(self.files.as_ref(), &track.uri).ok()??;
        image::load_from_memory(&picture.data).ok()
    }
}

/// `/<id>/<s|l>` → трек и размер.
fn parse_path(path: &str) -> Option<(TrackId, Size)> {
    let (id, size) = path.trim_start_matches('/').split_once('/')?;
    let size = match size {
        "s" => Size::Small,
        "l" => Size::Large,
        _ => return None,
    };
    Some((TrackId(id.parse().ok()?), size))
}

/// Квадрат из середины картинки стороной не больше `px` (мелкие не растягиваются).
fn square(img: &DynamicImage, px: u32) -> RgbImage {
    let side = img.width().min(img.height());
    let x = (img.width() - side) / 2;
    let y = (img.height() - side) / 2;
    let px = px.min(side).max(1);
    img.crop_imm(x, y, side, side)
        .thumbnail_exact(px, px)
        .to_rgb8()
}

fn jpeg(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(img)
        .expect("JPEG в память кодируется");
    out
}

/// Цвет обложки: среднее, где насыщенные пиксели весят больше серых, —
/// затем затемнение, чтобы белый текст поверх читался по WCAG AA (≥ 4.5:1).
fn accent(img: &RgbImage) -> [u8; 3] {
    let mut sum = [0.0f32; 3];
    let mut total = 0.0;
    for p in img.pixels() {
        let [r, g, b] = p.0.map(|c| f32::from(c) / 255.0);
        let (_, s, l) = hsl([r, g, b]);
        // Чёрное, белое и серое почти не голосуют; яркие средние тона — главные.
        let weight = 0.02 + s * (1.0 - (2.0 * l - 1.0).abs());
        sum[0] += r * weight;
        sum[1] += g * weight;
        sum[2] += b * weight;
        total += weight;
    }
    if total == 0.0 {
        return [0x2a, 0x2a, 0x2e];
    }
    readable(sum.map(|c| c / total))
}

/// Тот же оттенок, светлота в пределах 0.14–0.42 и не светлее, чем позволяет
/// контраст с белым.
fn readable(rgb: [f32; 3]) -> [u8; 3] {
    let (h, s, l) = hsl(rgb);
    let mut l = l.clamp(0.14, 0.42);
    loop {
        let color = from_hsl(h, s, l);
        if contrast_with_white(color) >= 4.5 || l <= 0.0 {
            return color.map(|c| (c * 255.0).round() as u8);
        }
        l = (l - 0.01).max(0.0);
    }
}

fn contrast_with_white(rgb: [f32; 3]) -> f32 {
    1.05 / (luminance(rgb) + 0.05)
}

/// Относительная яркость по WCAG.
fn luminance(rgb: [f32; 3]) -> f32 {
    let [r, g, b] = rgb.map(|c| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s.min(1.0), l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m].map(|v| v.clamp(0.0, 1.0))
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .expect("пустой ответ собирается")
}

/// Кэш с вытеснением давно не нужного. Ёмкость — сотни записей, поэтому
/// линейный поиск самой старой при вытеснении дешевле отдельной структуры.
struct Lru<K, V> {
    capacity: usize,
    tick: u64,
    map: HashMap<K, (V, u64)>,
}

impl<K: Hash + Eq + Clone, V: Clone> Lru<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tick: 0,
            map: HashMap::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        self.tick += 1;
        let entry = self.map.get_mut(key)?;
        entry.1 = self.tick;
        Some(entry.0.clone())
    }

    fn put(&mut self, key: K, value: V) {
        self.tick += 1;
        if self.map.len() >= self.capacity
            && !self.map.contains_key(&key)
            && let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.map.remove(&oldest);
        }
        self.map.insert(key, (value, self.tick));
    }

    fn clear(&mut self) {
        self.map.clear();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::LocalFiles;
    use image::{ImageFormat, Rgb};
    use player_domain::{MediaUri, Track};
    use player_library::MemoryLibrary;
    use std::path::PathBuf;

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(rgb))
    }

    fn rgb(hex: &str) -> [f32; 3] {
        let n = u32::from_str_radix(&hex[1..], 16).unwrap();
        [n >> 16, (n >> 8) & 0xff, n & 0xff].map(|c| c as f32 / 255.0)
    }

    #[test]
    fn accent_keeps_hue_and_is_readable_under_white_text() {
        for (name, color) in [
            ("красный", [0xe7, 0x4c, 0x3c]),
            ("жёлтый", [0xff, 0xee, 0x58]),
            ("белый", [0xff, 0xff, 0xff]),
            ("чёрный", [0, 0, 0]),
        ] {
            let out = accent(&solid(8, 8, color)).map(|c| f32::from(c) / 255.0);
            assert!(contrast_with_white(out) >= 4.5, "{name}: {out:?}");
            let (_, _, l) = hsl(out);
            assert!(l >= 0.13, "{name} не уходит в чёрный: {l}");
        }
        let [r, g, b] = accent(&solid(8, 8, [0xe7, 0x4c, 0x3c]));
        assert!(r > g && r > b, "красная обложка — красноватый фон");
        let [r, g, b] = accent(&solid(8, 8, [0x80, 0x80, 0x80]));
        assert!(r == g && g == b, "серая — нейтральный");
    }

    #[test]
    fn saturated_detail_outvotes_grey_background() {
        // 90 % серого, 10 % синего: фон должен получиться синеватым.
        let mut img = solid(10, 10, [0x77, 0x77, 0x77]);
        for x in 0..10 {
            img.put_pixel(x, 0, Rgb([0x1e, 0x60, 0xff]));
        }
        let [r, g, b] = accent(&img);
        assert!(b > r + 20 && b > g, "синий перевесил: {r} {g} {b}");
    }

    #[test]
    fn square_crops_the_middle_and_does_not_upscale() {
        // Слева и справа красные поля, в середине квадрат зелёного.
        let mut wide = solid(300, 100, [255, 0, 0]);
        for x in 100..200 {
            for y in 0..100 {
                wide.put_pixel(x, y, Rgb([0, 255, 0]));
            }
        }
        let out = square(&DynamicImage::ImageRgb8(wide), 64);
        assert_eq!(out.dimensions(), (64, 64));
        assert!(out.pixels().all(|p| p.0[1] > 200 && p.0[0] < 50));
        let small = square(&DynamicImage::ImageRgb8(solid(40, 40, [0; 3])), 128);
        assert_eq!(small.dimensions(), (40, 40));
        let decoded =
            image::load_from_memory_with_format(&jpeg(&small), ImageFormat::Jpeg).unwrap();
        assert_eq!(decoded.width(), 40);
    }

    #[test]
    fn parses_cover_paths() {
        assert_eq!(parse_path("/12/s"), Some((TrackId(12), Size::Small)));
        assert_eq!(parse_path("/12/l"), Some((TrackId(12), Size::Large)));
        assert_eq!(parse_path("12/s"), Some((TrackId(12), Size::Small)));
        for bad in ["/12", "/12/x", "/abc/s", "/-1/s", "/"] {
            assert_eq!(parse_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut lru = Lru::new(2);
        lru.put(1, "a");
        lru.put(2, "b");
        assert_eq!(lru.get(&1), Some("a"));
        lru.put(3, "c");
        assert_eq!(lru.get(&2), None, "вытеснен давно не нужный");
        assert_eq!((lru.get(&1), lru.get(&3)), (Some("a"), Some("c")));
        lru.put(3, "c2");
        assert_eq!(lru.map.len(), 2, "замена не вытесняет");
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../core/crates/player-testkit/fixtures")
            .join(name)
    }

    fn track(id: u64, path: &std::path::Path) -> Track {
        Track {
            id: TrackId(id),
            uri: MediaUri(path.to_string_lossy().into_owned()),
            title: "t".into(),
            artists: Vec::new(),
            album: None,
            album_artist: None,
            duration_ms: None,
            track_no: None,
            disc_no: None,
            recording_mbid: None,
            isrc: None,
            replay_gain: Default::default(),
        }
    }

    #[test]
    fn serves_embedded_cover_and_404_without_one() {
        let memory = Arc::new(MemoryLibrary::new());
        memory.insert(track(1, &fixture("covered.flac")));
        memory.insert(track(2, &fixture("tagged.flac")));
        let covers = Covers::new(memory, Arc::new(LocalFiles));

        let ok = covers.respond("/1/s");
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_TYPE], "image/jpeg");
        let img = image::load_from_memory(ok.body()).unwrap();
        // Встроенная картинка 64×64 меньше 128 — не растягивается.
        assert_eq!((img.width(), img.height()), (64, 64));
        // Цвет фикстуры — 0xc0392b: фон красноватый и читаемый.
        let color = covers.color(TrackId(1)).unwrap();
        let [r, g, b] = rgb(&color);
        assert!(
            r > g && r > b && contrast_with_white([r, g, b]) >= 4.5,
            "{color}"
        );

        assert_eq!(covers.respond("/2/s").status(), StatusCode::NOT_FOUND);
        assert_eq!(covers.respond("/3/l").status(), StatusCode::NOT_FOUND);
        assert_eq!(covers.color(TrackId(2)), None);
        assert_eq!(covers.respond("/1/x").status(), StatusCode::BAD_REQUEST);
    }
}
