//! "Now playing" for any player that talks to Windows' System Media Transport
//! Controls (Spotify, browsers, Yandex Music, VLC, ...). No per-service OAuth.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// AppUserModelID of the player, e.g. `Spotify.exe`.
    pub source: String,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
    /// Position extrapolated to "now"; `None` when the player reports no timeline.
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> windows::core::Result<R> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn media_now_playing() -> Result<Option<NowPlaying>, String> {
    blocking(win::now_playing).await
}

/// Album art of the current track as a `data:` URL.
#[tauri::command]
pub async fn media_thumbnail() -> Result<Option<String>, String> {
    blocking(win::thumbnail).await
}

#[tauri::command]
pub async fn media_control(action: String) -> Result<(), String> {
    blocking(move || win::control(&action)).await
}

pub mod win {
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::prelude::{Engine, BASE64_STANDARD};
    use windows::core::{Error, Interface, Result};
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
        ColorManagementMode, ExifOrientationMode,
    };
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::{DataReader, IRandomAccessStream};
    use windows::Win32::Foundation::E_INVALIDARG;

    use super::NowPlaying;

    /// 100ns ticks between 1601-01-01 (WinRT epoch) and 1970-01-01.
    const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;

    fn manager() -> Result<Manager> {
        static MANAGER: OnceLock<Manager> = OnceLock::new();
        if let Some(m) = MANAGER.get() {
            return Ok(m.clone());
        }
        let m = Manager::RequestAsync()?.join()?;
        Ok(MANAGER.get_or_init(|| m).clone())
    }

    fn session() -> Result<Option<Session>> {
        // No active player surfaces as an error from GetCurrentSession.
        Ok(manager()?.GetCurrentSession().ok())
    }

    fn now_ticks() -> i64 {
        let since_unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        since_unix.as_nanos() as i64 / 100 + EPOCH_DIFF_TICKS
    }

    pub fn now_playing() -> Result<Option<NowPlaying>> {
        let Some(s) = session()? else { return Ok(None) };
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        let info = s.GetPlaybackInfo()?;
        let controls = info.Controls()?;
        let playing = info.PlaybackStatus()? == Status::Playing;

        let timeline = s.GetTimelineProperties()?;
        let duration = timeline.EndTime()?.Duration - timeline.StartTime()?.Duration;
        let (position_ms, duration_ms) = if duration > 0 {
            let mut pos = timeline.Position()?.Duration;
            // Players only report position occasionally; extrapolate to now.
            if playing {
                pos += (now_ticks() - timeline.LastUpdatedTime()?.UniversalTime).max(0);
            }
            let pos = pos.clamp(0, duration);
            (Some(pos as u64 / 10_000), Some(duration as u64 / 10_000))
        } else {
            (None, None)
        };

        Ok(Some(NowPlaying {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            album: props.AlbumTitle()?.to_string(),
            source: s.SourceAppUserModelId()?.to_string(),
            playing,
            can_prev: controls.IsPreviousEnabled()?,
            can_next: controls.IsNextEnabled()?,
            position_ms,
            duration_ms,
        }))
    }

    pub fn thumbnail() -> Result<Option<String>> {
        let Some(s) = session()? else { return Ok(None) };
        let Ok(reference) = s.TryGetMediaPropertiesAsync()?.join()?.Thumbnail() else {
            return Ok(None);
        };
        let stream = reference.OpenReadAsync()?.join()?;
        let size = stream.Size()? as u32;
        if size == 0 {
            return Ok(None);
        }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.join()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;

        let mime = stream.ContentType().map(|m| m.to_string()).unwrap_or_default();
        let mime = if mime.is_empty() { "image/png".to_string() } else { mime };
        Ok(Some(format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes))))
    }

    /// Album art centre-cropped to a `size`×`size` square, as premultiplied BGRA.
    pub fn cover_pixels(size: u32) -> Result<Option<Vec<u8>>> {
        let Some(s) = session()? else { return Ok(None) };
        let Ok(reference) = s.TryGetMediaPropertiesAsync()?.join()?.Thumbnail() else {
            return Ok(None);
        };
        let stream: IRandomAccessStream = reference.OpenReadAsync()?.join()?.cast()?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
        let (w, h) = (decoder.OrientedPixelWidth()?.max(1), decoder.OrientedPixelHeight()?.max(1));
        // Scale the short side to `size`, then crop the middle.
        let short = w.min(h);
        let (sw, sh) = ((w * size).div_ceil(short), (h * size).div_ceil(short));
        let transform = BitmapTransform::new()?;
        transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
        transform.SetScaledWidth(sw)?;
        transform.SetScaledHeight(sh)?;
        transform.SetBounds(BitmapBounds { X: (sw - size) / 2, Y: (sh - size) / 2, Width: size, Height: size })?;
        let data = decoder
            .GetPixelDataTransformedAsync(
                BitmapPixelFormat::Bgra8,
                BitmapAlphaMode::Premultiplied,
                &transform,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::DoNotColorManage,
            )?
            .join()?;
        Ok(Some(data.DetachPixelData()?.to_vec()))
    }

    pub fn control(action: &str) -> Result<()> {
        let Some(s) = session()? else { return Ok(()) };
        let op = match action {
            "toggle" => s.TryTogglePlayPauseAsync()?,
            "next" => s.TrySkipNextAsync()?,
            "prev" => s.TrySkipPreviousAsync()?,
            _ => return Err(Error::from(E_INVALIDARG)),
        };
        op.join()?;
        Ok(())
    }
}
