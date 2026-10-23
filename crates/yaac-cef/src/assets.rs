//! Files behind `app://ui/`. The app supplies them; the engine only serves them.
//!
//! Release builds serve the UI only from the copy compiled into the binary (`embed-ui`). Development
//! builds may read a directory (`DirAssets`, `YAAC_UI_DIR`), which is not compiled into release.

use std::path::{Component, Path};

#[cfg(any(debug_assertions, feature = "dev-tools"))]
use std::path::PathBuf;

pub struct Asset {
    pub mime: &'static str,
    pub body: Vec<u8>,
}

/// Serves `app://ui/<path>`. `path` has no leading slash. `None` answers 404.
pub trait UiAssets: Send + Sync {
    fn asset(&self, path: &str) -> Option<Asset>;

    /// True if HTML served from here gets the UI's Content-Security-Policy. The placeholder pages
    /// of the harness keep their inline scripts, so they answer false.
    fn enforces_csp(&self) -> bool {
        true
    }
}

/// Files from a directory. Development builds only.
#[cfg(any(debug_assertions, feature = "dev-tools"))]
pub struct DirAssets {
    root: PathBuf,
}

#[cfg(any(debug_assertions, feature = "dev-tools"))]
impl DirAssets {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[cfg(any(debug_assertions, feature = "dev-tools"))]
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

/// The UI compiled into the binary (`embed-ui`, release builds). The bundle script builds `ui/dist`
/// before a release build.
#[cfg(all(feature = "embed-ui", not(debug_assertions)))]
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]
struct UiDist;

#[cfg(all(feature = "embed-ui", not(debug_assertions)))]
struct Embedded;

#[cfg(all(feature = "embed-ui", not(debug_assertions)))]
impl UiAssets for Embedded {
    fn asset(&self, path: &str) -> Option<Asset> {
        let relative = sanitize(path)?;
        let name = relative.to_str()?;
        let file = UiDist::get(name)?;
        Some(Asset {
            mime: mime_for(Path::new(name)),
            body: file.data.into_owned(),
        })
    }
}

/// The UI copy compiled into this binary, if there is one.
#[cfg(all(feature = "embed-ui", not(debug_assertions)))]
pub fn embedded_ui() -> Option<std::sync::Arc<dyn UiAssets>> {
    Some(std::sync::Arc::new(Embedded))
}

/// Files compiled into the binary, served without the UI's CSP. For the harness placeholder only.
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

    fn enforces_csp(&self) -> bool {
        false
    }
}

/// Turns a request path into a relative file path. Only plain components are accepted: no `..`,
/// no root, no drive, no hidden or empty component. The empty path is `index.html`.
pub(crate) fn sanitize(path: &str) -> Option<std::path::PathBuf> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(std::path::PathBuf::from("index.html"));
    }
    let mut out = std::path::PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_str()?;
                if text.starts_with('.') || text.contains('\\') {
                    return None;
                }
                out.push(part);
            }
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(any(
    debug_assertions,
    feature = "dev-tools",
    all(feature = "embed-ui", not(debug_assertions))
))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_plain_relative_paths() {
        assert_eq!(
            sanitize("sidebar.html"),
            Some(PathBuf::from("sidebar.html"))
        );
        assert_eq!(
            sanitize("/assets/app.js"),
            Some(PathBuf::from("assets/app.js"))
        );
        assert_eq!(sanitize(""), Some(PathBuf::from("index.html")));
        assert_eq!(sanitize("/"), Some(PathBuf::from("index.html")));
    }

    #[test]
    fn sanitize_refuses_traversal_hidden_and_odd_components() {
        assert_eq!(sanitize("../secret"), None);
        assert_eq!(sanitize("a/../../secret"), None);
        assert_eq!(sanitize("/etc/passwd/../x"), None);
        assert_eq!(sanitize(".env"), None);
        assert_eq!(sanitize("a/.git/config"), None);
        assert_eq!(sanitize("a\\b.html"), None);
    }

    #[test]
    fn mime_types_for_the_asset_kinds_the_ui_ships() {
        assert_eq!(mime_for(Path::new("a.js")), "text/javascript");
        assert_eq!(mime_for(Path::new("a.mjs")), "text/javascript");
        assert_eq!(mime_for(Path::new("a.css")), "text/css");
        assert_eq!(mime_for(Path::new("a.HTML")), "text/html");
        assert_eq!(mime_for(Path::new("a.bin")), "application/octet-stream");
    }

    static FILES: &[(&str, &str, &[u8])] = &[("sidebar.html", "text/html", b"<p>")];

    #[test]
    fn embedded_placeholders_answer_without_csp() {
        let assets = EmbeddedAssets::new(FILES);
        assert!(!assets.enforces_csp());
        assert!(assets.asset("sidebar.html").is_some());
        assert!(assets.asset("missing.html").is_none());
        assert!(assets.asset("../sidebar.html").is_none());
    }
}
