//! Настоящие папки на диске для тестов сканера и обложек: порт `FileAccess`
//! над файловой системой, как адаптер оболочки, и временная папка.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use player_domain::MediaUri;
use player_ports::{FileAccess, FileEntry, MediaStream, PortError};

/// Папки и файлы с диска — как адаптер оболочки.
#[derive(Debug, Default)]
pub struct Disk;

impl FileAccess for Disk {
    fn list(&self, dir: &MediaUri) -> Result<Vec<FileEntry>, PortError> {
        let read = fs::read_dir(&dir.0).map_err(|e| PortError::Other(e.to_string()))?;
        Ok(read
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some(FileEntry {
                    uri: MediaUri(e.path().to_string_lossy().into_owned()),
                    is_dir: e.file_type().ok()?.is_dir(),
                    size: meta.len(),
                    modified_ms: meta
                        .modified()
                        .ok()?
                        .duration_since(UNIX_EPOCH)
                        .ok()?
                        .as_millis() as i64,
                })
            })
            .collect())
    }

    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError> {
        fs::File::open(&uri.0)
            .map(|f| Box::new(f) as Box<dyn MediaStream>)
            .map_err(|e| PortError::Other(e.to_string()))
    }
}

/// Временная папка; удаляется вместе со значением.
#[derive(Debug)]
pub struct TempDir(pub PathBuf);

impl TempDir {
    /// Пустая папка; `name` различает тесты, id процесса — запуски.
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("player-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Файл с одним байтом по относительному пути, папки создаются.
    pub fn file(&self, path: &str) {
        self.write(path, b"x");
    }

    /// Файл с содержимым по относительному пути, папки создаются.
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Адрес самой папки.
    pub fn uri(&self) -> MediaUri {
        MediaUri(self.0.to_string_lossy().into_owned())
    }

    /// Адрес файла внутри.
    pub fn uri_of(&self, path: &str) -> MediaUri {
        MediaUri(self.0.join(path).to_string_lossy().into_owned())
    }

    /// Путь папки.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
