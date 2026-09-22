use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use thiserror::Error;

pub const MAX_SKIN_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SkinError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid skin: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DragArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Panel {
    pub rect: Rect,
    pub image: String,
    #[serde(default = "default_clip")]
    pub clip: String,
    #[serde(default)]
    pub drag_areas: Vec<DragArea>,
}

#[allow(dead_code)]
fn default_clip() -> String {
    "auto-alpha".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visualizer {
    pub panel: String,
    pub rect: Rect,
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    "bars".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinManifest {
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub panels: serde_json::Map<String, serde_json::Value>,
    /// Sprite UI v2: block plates + anchors (optional on v1).
    #[serde(default)]
    pub blocks: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub faders: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub visualizer: Option<Visualizer>,
}

pub struct LoadedSkin {
    pub manifest: SkinManifest,
    /// (relative path, bytes)
    pub assets: Vec<(String, Vec<u8>)>,
}

impl LoadedSkin {
    pub fn asset(&self, rel: &str) -> Option<&[u8]> {
        self.assets
            .iter()
            .find(|(p, _)| p == rel)
            .map(|(_, b)| b.as_slice())
    }
}

fn safe_entry_name(name: &str) -> bool {
    let n = name.replace('\\', "/");
    if n.starts_with('/') || n.contains(':') {
        return false;
    }
    // Reject any path segment that is exactly ".."
    !n.split('/').any(|seg| seg == "..")
}

pub fn parse_skin_zip(bytes: &[u8]) -> Result<LoadedSkin, SkinError> {
    if bytes.len() as u64 > MAX_SKIN_BYTES {
        return Err(SkinError::Invalid("skin too large".into()));
    }
    let cursor = Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)?;
    let mut manifest: Option<SkinManifest> = None;
    let mut assets = Vec::new();
    let mut total_uncompressed: u64 = 0;

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        let name = file.name().to_string();
        if !safe_entry_name(&name) {
            return Err(SkinError::Invalid(format!("unsafe path {name}")));
        }
        // Cap each entry before allocating — zip-bomb guard.
        let declared = file.size();
        if declared > MAX_SKIN_BYTES {
            return Err(SkinError::Invalid("entry too large".into()));
        }
        if total_uncompressed + declared > MAX_SKIN_BYTES {
            return Err(SkinError::Invalid("uncompressed skin too large".into()));
        }
        let mut buf = Vec::with_capacity(declared.min(1 << 20) as usize);
        let max_read = MAX_SKIN_BYTES.saturating_sub(total_uncompressed);
        let mut limited = file.take(max_read);
        limited.read_to_end(&mut buf)?;
        if buf.len() as u64 > max_read {
            return Err(SkinError::Invalid("uncompressed skin too large".into()));
        }
        total_uncompressed += buf.len() as u64;
        if name == "skin.json" {
            let m: SkinManifest = serde_json::from_slice(&buf)?;
            if m.format_version != 1 && m.format_version != 2 {
                return Err(SkinError::Invalid(format!(
                    "unsupported formatVersion {}",
                    m.format_version
                )));
            }
            if m.id.is_empty() || m.name.is_empty() {
                return Err(SkinError::Invalid("id and name required".into()));
            }
            if m.format_version == 2 {
                let has_blocks = m.blocks.as_ref().map(|b| !b.is_empty()).unwrap_or(false);
                let has_faders = m.faders.as_ref().map(|f| !f.is_empty()).unwrap_or(false);
                if !has_blocks {
                    return Err(SkinError::Invalid("v2 skin requires blocks".into()));
                }
                if !has_faders {
                    return Err(SkinError::Invalid("v2 skin requires faders".into()));
                }
            }
            manifest = Some(m);
        } else {
            assets.push((name, buf));
        }
    }

    let manifest = manifest.ok_or_else(|| SkinError::Invalid("missing skin.json".into()))?;
    Ok(LoadedSkin { manifest, assets })
}

pub fn parse_skin_path(path: &Path) -> Result<LoadedSkin, SkinError> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_SKIN_BYTES {
        return Err(SkinError::Invalid("skin file too large".into()));
    }
    let bytes = std::fs::read(path)?;
    parse_skin_zip(&bytes)
}

#[allow(dead_code)]
pub fn default_skin_manifest() -> SkinManifest {
    SkinManifest {
        format_version: 1,
        id: "misima-hybrid-default".into(),
        name: "Misima Hybrid Default".into(),
        author: "Misima".into(),
        panels: serde_json::Map::new(),
        blocks: None,
        faders: None,
        visualizer: Some(Visualizer {
            panel: "main".into(),
            rect: Rect {
                x: 280.0,
                y: 80.0,
                w: 520.0,
                h: 160.0,
            },
            mode: "bars".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default();
            for (name, data) in files {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn rejects_path_traversal() {
        let manifest = br#"{"formatVersion":1,"id":"x","name":"x","panels":{}}"#;
        let bytes = build_zip(&[("skin.json", manifest), ("../evil.png", b"nope")]);
        assert!(parse_skin_zip(&bytes).is_err());
    }

    #[test]
    fn rejects_v2_without_faders() {
        let manifest = br#"{"formatVersion":2,"id":"x","name":"x","blocks":{"a":{}},"faders":[]}"#;
        let bytes = build_zip(&[("skin.json", manifest)]);
        assert!(parse_skin_zip(&bytes).is_err());
    }

    #[test]
    fn loads_valid_v2() {
        let manifest = br#"{"formatVersion":2,"id":"x","name":"x","blocks":{"a":{"image":"a.png"}},"faders":[{"id":"volume"}]}"#;
        let bytes = build_zip(&[("skin.json", manifest), ("a.png", &[1, 2, 3])]);
        let skin = parse_skin_zip(&bytes).expect("v2");
        assert_eq!(skin.manifest.format_version, 2);
    }

    #[test]
    fn loads_valid_skin() {
        let manifest = br#"{"formatVersion":1,"id":"demo","name":"Demo","panels":{}}"#;
        let bytes = build_zip(&[
            ("skin.json", manifest),
            ("assets/a.png", &[0x89, b'P', b'N', b'G']),
        ]);
        let skin = parse_skin_zip(&bytes).expect("valid");
        assert_eq!(skin.manifest.id, "demo");
        assert!(skin.asset("assets/a.png").is_some());
    }

    #[test]
    fn rejects_missing_manifest() {
        let bytes = build_zip(&[("readme.txt", b"hi")]);
        assert!(parse_skin_zip(&bytes).is_err());
    }
}
