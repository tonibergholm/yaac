//! Files behind `app://ui/`. The app supplies them; the engine only serves them.

use std::path::{Component, Path, PathBuf};

pub struct Asset {
    pub mime: &'static str,
    pub body: Vec<u8>,
}

/// Serves `app://ui/<path>`. `path` has no leading slash. `None` answers 404.
pub trait UiAssets: Send + Sync {
    fn asset(&self, path: &str) -> Option<Asset>;
}

/// Files from a directory, for development (`YAAC_UI_DIR`). Refuses paths that are not plain
/// relative components, and symlinks that resolve outside the root.
pub struct DirAssets {
    root: PathBuf,
}

impl DirAssets {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl UiAssets for DirAssets {
    fn asset(&self, path: &str) -> Option<Asset> {
        let relative = sanitize(path)?;
        let root = self.root.canonicalize().ok()?;
        let full = root.join(relative).canonicalize().ok()?;
        if !full.starts_with(&root) || !full.is_file() {
            return None;
        }
        let body = std::fs::read(&full).ok()?;
        Some(Asset {
            mime: mime_for(&full),
            body,
        })
    }
}

/// Files compiled into the binary. Used by the harness and as the fallback sidebar.
pub struct EmbeddedAssets {
    files: &'static [(&'static str, &'static str, &'static [u8])],
}

impl EmbeddedAssets {
    /// `(path, mime, body)` triples.
    pub const fn new(files: &'static [(&'static str, &'static str, &'static [u8])]) -> Self {
        Self { files }
    }
}

impl UiAssets for EmbeddedAssets {
    fn asset(&self, path: &str) -> Option<Asset> {
        let relative = sanitize(path)?;
        let wanted = relative.to_str()?;
        self.files
            .iter()
            .find(|(name, _, _)| *name == wanted)
            .map(|(_, mime, body)| Asset {
                mime,
                body: body.to_vec(),
            })
    }
}

fn sanitize(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(PathBuf::from("index.html"));
    }
    let mut out = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    Some(out)
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}
