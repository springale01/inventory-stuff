use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// UTF-8 text storage. Saves replace the destination only after writing succeeds.
pub struct TextFile {
    path: PathBuf,
}

impl TextFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn load_file(&self) -> io::Result<String> {
        tokio::fs::read_to_string(&self.path).await
    }

    /// Creates missing parent directories and uses a temporary file in the same
    /// directory. Once started, the replacement finishes even if the caller drops
    /// the future. Concurrent saves use distinct temporary files; the last wins.
    pub async fn save(&self, value: impl AsRef<str>) -> io::Result<()> {
        let path = self.path.clone();
        let contents = value.as_ref().as_bytes().to_vec();
        tokio::task::spawn_blocking(move || replace_file(&path, &contents))
            .await
            .map_err(io::Error::other)?
    }
}

struct TemporaryFile(Option<PathBuf>);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Expected a file path"))?;
    let (mut temporary, mut file) = loop {
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let candidate = parent.join(temp_name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (TemporaryFile(Some(candidate)), file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    // Close before cleanup/rename, including on write failure (important on Windows).
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    drop(file);
    written?;
    std::fs::rename(temporary.0.as_ref().expect("temporary path exists"), path)?;
    temporary.0 = None;
    // No directory fsync is promised.
    Ok(())
}
