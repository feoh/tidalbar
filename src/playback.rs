use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::{Builder, NamedTempFile};
use thiserror::Error;

use crate::models::MediaItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioQuality {
    Preview,
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayableSource {
    Url(String),
    DashManifest(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayableResource {
    pub source: PlayableSource,
    pub quality: AudioQuality,
}

#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("full-track playback is disabled pending written permission from TIDAL")]
    FullTrackUnavailable,
}

pub trait MediaResolver {
    fn resolve(&self, item: &MediaItem) -> Result<PlayableResource, ResolverError>;
}

#[derive(Debug, Default)]
pub struct PreviewResolver;

impl MediaResolver for PreviewResolver {
    fn resolve(&self, item: &MediaItem) -> Result<PlayableResource, ResolverError> {
        item.preview_url
            .as_ref()
            .map(|uri| PlayableResource {
                source: PlayableSource::Url(uri.clone()),
                quality: AudioQuality::Preview,
            })
            .ok_or(ResolverError::FullTrackUnavailable)
    }
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("mpv could not be started; install mpv and ensure it is on PATH: {0}")]
    Start(#[source] std::io::Error),
    #[error("could not connect to mpv IPC at {path}: {source}")]
    Connect {
        path: String,
        source: std::io::Error,
    },
    #[error("could not encode an mpv command: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("could not send a command to mpv: {0}")]
    Command(#[source] std::io::Error),
    #[error("could not stage a DASH manifest for mpv: {0}")]
    Manifest(#[source] std::io::Error),
}

pub trait AudioEngine {
    fn play(&mut self, resource: &PlayableResource) -> Result<(), PlaybackError>;
    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError>;
    fn stop(&mut self) -> Result<(), PlaybackError>;
}

#[derive(Default)]
pub struct MpvEngine {
    child: Option<Child>,
    ipc: Option<Box<dyn Write + Send>>,
    ipc_path: Option<String>,
    // Keep the current manifest until mpv has finished loading it. Keep the
    // previous one until a later play, since IPC writes do not await loadfile.
    current_manifest: Option<NamedTempFile>,
    previous_manifest: Option<NamedTempFile>,
}

impl MpvEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_started(&mut self) -> Result<(), PlaybackError> {
        let running = self.child.as_mut().is_some_and(|child| {
            child
                .try_wait()
                .map(|status| status.is_none())
                .unwrap_or(false)
        });
        if running && self.ipc.is_some() {
            return Ok(());
        }

        self.cleanup();
        let ipc_path = ipc_path();
        remove_ipc_file(&ipc_path);
        let ipc_argument = format!("--input-ipc-server={ipc_path}");
        let mut child = Command::new("mpv")
            .args([
                "--idle=yes",
                "--no-video",
                "--no-terminal",
                "--really-quiet",
                // DASH MPDs staged locally reference HTTPS segments. FFmpeg's
                // default nested-protocol whitelist would reject those URLs.
                "--demuxer-lavf-o=protocol_whitelist=[file,crypto,data,https,tcp,tls]",
                ipc_argument.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(PlaybackError::Start)?;

        let mut last_error = std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "mpv IPC endpoint was not created",
        );
        for _ in 0..100 {
            match connect_ipc(&ipc_path) {
                Ok(ipc) => {
                    self.child = Some(child);
                    self.ipc = Some(ipc);
                    self.ipc_path = Some(ipc_path);
                    return Ok(());
                }
                Err(error) => last_error = error,
            }
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                return Err(PlaybackError::Start(std::io::Error::other(
                    "mpv exited before its IPC endpoint became available",
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }

        let _ = child.kill();
        let _ = child.wait();
        Err(PlaybackError::Connect {
            path: ipc_path,
            source: last_error,
        })
    }

    fn command(&mut self, command: Value) -> Result<(), PlaybackError> {
        self.ensure_started()?;
        let mut payload = command_payload(command)?;
        payload.push(b'\n');
        let ipc = self.ipc.as_mut().ok_or_else(|| {
            PlaybackError::Command(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mpv IPC connection is unavailable",
            ))
        })?;
        ipc.write_all(&payload)
            .and_then(|()| ipc.flush())
            .map_err(PlaybackError::Command)
    }

    fn cleanup(&mut self) {
        self.ipc = None;
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.current_manifest = None;
        self.previous_manifest = None;
        if let Some(path) = self.ipc_path.take() {
            remove_ipc_file(&path);
        }
    }
}

impl AudioEngine for MpvEngine {
    fn play(&mut self, resource: &PlayableResource) -> Result<(), PlaybackError> {
        let new_manifest = match &resource.source {
            PlayableSource::Url(_) => None,
            PlayableSource::DashManifest(xml) => Some(stage_manifest(xml)?),
        };
        let path = match (&resource.source, new_manifest.as_ref()) {
            (PlayableSource::Url(uri), _) => uri.clone(),
            (PlayableSource::DashManifest(_), Some(file)) => {
                file.path().to_string_lossy().into_owned()
            }
            _ => unreachable!("DASH manifest must have been staged"),
        };
        self.command(json!(["loadfile", path, "replace"]))?;
        self.previous_manifest = self.current_manifest.take();
        self.current_manifest = new_manifest;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError> {
        self.command(json!(["set_property", "pause", paused]))
    }

    fn stop(&mut self) -> Result<(), PlaybackError> {
        if self.child.is_none() {
            return Ok(());
        }
        self.command(json!(["stop"]))?;
        self.current_manifest = None;
        self.previous_manifest = None;
        Ok(())
    }
}

impl Drop for MpvEngine {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn stage_manifest(xml: &str) -> Result<NamedTempFile, PlaybackError> {
    let mut file = Builder::new()
        .prefix("tidalbar-")
        .suffix(".mpd")
        .tempfile()
        .map_err(PlaybackError::Manifest)?;
    file.write_all(xml.as_bytes())
        .map_err(PlaybackError::Manifest)?;
    file.flush().map_err(PlaybackError::Manifest)?;
    Ok(file)
}

fn command_payload(command: Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&json!({ "command": command }))
}

#[cfg(unix)]
fn ipc_path() -> String {
    std::env::temp_dir()
        .join(format!("tidalbar-mpv-{}.sock", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
fn ipc_path() -> String {
    format!(r"\\.\pipe\tidalbar-mpv-{}", std::process::id())
}

#[cfg(not(any(unix, windows)))]
fn ipc_path() -> String {
    format!("tidalbar-mpv-{}", std::process::id())
}

#[cfg(unix)]
fn connect_ipc(path: &str) -> std::io::Result<Box<dyn Write + Send>> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    drain_responses(stream.try_clone()?);
    Ok(Box::new(stream))
}

#[cfg(windows)]
fn connect_ipc(path: &str) -> std::io::Result<Box<dyn Write + Send>> {
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    drain_responses(pipe.try_clone()?);
    Ok(Box::new(pipe))
}

#[cfg(not(any(unix, windows)))]
fn connect_ipc(_path: &str) -> std::io::Result<Box<dyn Write + Send>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "mpv IPC is unsupported on this platform",
    ))
}

fn drain_responses(mut reader: impl Read + Send + 'static) {
    thread::spawn(move || {
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
}

#[cfg(unix)]
fn remove_ipc_file(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(not(unix))]
fn remove_ipc_file(_path: &str) {}

#[cfg(test)]
mod tests {
    use crate::models::MediaKind;

    use super::*;

    #[test]
    fn preview_resolver_rejects_items_without_official_preview_urls() {
        let item = MediaItem::new("1", "Track", "Artist", MediaKind::Track);

        let error = PreviewResolver.resolve(&item).expect_err("must reject");

        assert_eq!(
            error.to_string(),
            "full-track playback is disabled pending written permission from TIDAL"
        );
    }

    #[test]
    fn preview_resolver_preserves_official_preview_url() {
        let mut item = MediaItem::new("1", "Track", "Artist", MediaKind::Track);
        item.preview_url = Some("https://example.test/preview.flac".to_owned());

        let resource = PreviewResolver.resolve(&item).expect("preview resolves");

        assert_eq!(
            resource.source,
            PlayableSource::Url("https://example.test/preview.flac".to_owned())
        );
        assert_eq!(resource.quality, AudioQuality::Preview);
    }

    #[test]
    fn dash_manifest_uses_a_temporary_mpd_file() {
        let file = stage_manifest("<MPD/>").expect("stage");
        assert_eq!(
            file.path().extension().and_then(|ext| ext.to_str()),
            Some("mpd")
        );
        assert_eq!(
            std::fs::read_to_string(file.path()).expect("read"),
            "<MPD/>"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = file.as_file().metadata().expect("metadata").permissions();
            assert_eq!(permissions.mode() & 0o077, 0, "manifest must be private");
        }
        let path = file.path().to_owned();
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn mpv_commands_use_json_ipc() {
        let payload = command_payload(json!(["loadfile", "https://example.test/a b", "replace"]))
            .expect("command encodes");
        let decoded: Value = serde_json::from_slice(&payload).expect("valid JSON");

        assert_eq!(
            decoded,
            json!({"command": ["loadfile", "https://example.test/a b", "replace"]})
        );
    }
}
