use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

use thiserror::Error;

use crate::models::MediaItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioQuality {
    Preview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayableResource {
    pub uri: String,
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
                uri: uri.clone(),
                quality: AudioQuality::Preview,
            })
            .ok_or(ResolverError::FullTrackUnavailable)
    }
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("mpv could not be started; install mpv and ensure it is on PATH: {0}")]
    Start(#[source] std::io::Error),
    #[error("could not send a command to mpv: {0}")]
    Command(#[source] std::io::Error),
}

pub trait AudioEngine {
    fn play(&mut self, resource: &PlayableResource) -> Result<(), PlaybackError>;
    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError>;
    fn stop(&mut self) -> Result<(), PlaybackError>;
}

#[derive(Debug, Default)]
pub struct MpvEngine {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl MpvEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_started(&mut self) -> Result<(), PlaybackError> {
        if self.child.as_mut().is_some_and(|child| {
            child
                .try_wait()
                .map(|status| status.is_none())
                .unwrap_or(false)
        }) {
            return Ok(());
        }

        self.child = None;
        self.stdin = None;

        let mut child = Command::new("mpv")
            .args([
                "--idle=yes",
                "--no-video",
                "--really-quiet",
                "--input-terminal=yes",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(PlaybackError::Start)?;
        self.stdin = child.stdin.take();
        self.child = Some(child);
        Ok(())
    }

    fn command(&mut self, command: &str) -> Result<(), PlaybackError> {
        self.ensure_started()?;
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            PlaybackError::Command(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "mpv stdin is unavailable",
            ))
        })?;
        stdin
            .write_all(command.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(PlaybackError::Command)
    }
}

impl AudioEngine for MpvEngine {
    fn play(&mut self, resource: &PlayableResource) -> Result<(), PlaybackError> {
        let quoted_uri = serde_json::to_string(&resource.uri).expect("a string is always JSON");
        self.command(&format!("loadfile {quoted_uri} replace\n"))
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError> {
        self.command(if paused {
            "set pause yes\n"
        } else {
            "set pause no\n"
        })
    }

    fn stop(&mut self) -> Result<(), PlaybackError> {
        self.command("stop\n")
    }
}

impl Drop for MpvEngine {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

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

        assert_eq!(resource.uri, "https://example.test/preview.flac");
        assert_eq!(resource.quality, AudioQuality::Preview);
    }
}
