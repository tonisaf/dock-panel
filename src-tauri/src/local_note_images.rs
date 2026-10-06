use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::http::{Request, Response};

pub const MAX_BYTES: usize = 20 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Image {
    pub id: String,
    pub name: String,
}
fn format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else {
        None
    }
}
pub fn safe_id(id: &str) -> bool {
    let Some((hash, ext)) = id.split_once('.') else {
        return false;
    };
    hash.len() == 64
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && ["png", "jpg", "gif", "webp"].contains(&ext)
}
pub fn directory() -> Result<PathBuf, String> {
    Ok(crate::local_notes::storage_path()?
        .parent()
        .ok_or("Нет папки хранения")?
        .join("local-note-images"))
}
pub fn import_bytes(dir: &Path, bytes: &[u8], name: &str) -> Result<Image, String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("Изображение должно быть не больше 20 МБ".into());
    }
    let ext = format(bytes).ok_or("Поддерживаются PNG, JPEG, GIF и WebP")?;
    let hash = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let id = format!("{hash}.{ext}");
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(&id);
    if std::fs::read(&path).ok().as_deref() != Some(bytes) {
        use std::io::Write;
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let temp = dir.join(format!(
            "{hash}-{}.tmp",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ));
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
            drop(f);
            let _ = std::fs::remove_file(&temp);
            return Err(e.to_string());
        }
        drop(f);
        if let Err(e) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(e.to_string());
        }
    }
    Ok(Image {
        id,
        name: name.chars().take(200).collect(),
    })
}
#[tauri::command]
pub async fn local_note_import_image(
    data: Option<String>,
    path: Option<String>,
    name: Option<String>,
) -> Result<Image, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, name) = if let Some(path) = path {
            let p = PathBuf::from(path);
            let mut f = std::fs::File::open(&p).map_err(|e| e.to_string())?;
            use std::io::Read;
            if !f.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("Перетащите файл изображения".into());
            }
            let mut bytes = Vec::new();
            (&mut f)
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            (
                bytes,
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            let data = data.ok_or("Нет изображения")?;
            if data.len() > MAX_BYTES * 4 / 3 + 8 {
                return Err("Изображение превышает 20 МБ".into());
            }
            (
                STANDARD.decode(data).map_err(|e| e.to_string())?,
                name.unwrap_or_else(|| "Из буфера".into()),
            )
        };
        import_bytes(&directory()?, &bytes, &name)
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn response(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let id = request.uri().path().trim_start_matches('/');
    let bytes = if safe_id(id) {
        directory()
            .ok()
            .and_then(|d| std::fs::read(d.join(id)).ok())
            .filter(|b| b.len() <= MAX_BYTES)
    } else {
        None
    };
    let mime = match id.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    Response::builder()
        .status(if bytes.is_some() { 200 } else { 404 })
        .header("Content-Type", mime)
        .header("X-Content-Type-Options", "nosniff")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "max-age=31536000, immutable")
        .body(bytes.unwrap_or_default())
        .expect("static headers")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copies_and_deduplicates_images_on_disk() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!("dock-note-images-{:?}", random));
        let bytes=STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jv1kAAAAASUVORK5CYII=").unwrap();
        let a = import_bytes(&dir, &bytes, "Image.png").unwrap();
        let b = import_bytes(&dir, &bytes, "Clipboard.png").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(std::fs::read(dir.join(&a.id)).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(import_bytes(&dir, b"<svg></svg>", "Image.png").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paths_and_formats_are_restricted() {
        assert!(!safe_id("../notes.sqlite"));
        assert!(!safe_id(&format!("{}.svg", "a".repeat(64))));
        assert!(safe_id(&format!("{}.png", "a".repeat(64))));
        assert_eq!(format(b"GIF89a"), Some("gif"));
        assert_eq!(format(b"<svg/>"), None);
    }
}
