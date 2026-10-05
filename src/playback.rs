use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::{Builder, NamedTempFile};
use thiserror::Error;

use crate::models::{MediaItem, PlaybackProgress};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    Finished,
    Failed(String),
}

pub trait AudioEngine {
    fn play(&mut self, resource: &PlayableResource) -> Result<(), PlaybackError>;
    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError>;
    fn stop(&mut self) -> Result<(), PlaybackError>;
    fn poll_event(&mut self) -> Option<PlaybackEvent> {
        None
    }
    fn progress(&self) -> PlaybackProgress {
        PlaybackProgress::default()
    }
}

#[derive(Default)]
pub struct MpvEngine {
    child: Option<Child>,
    ipc: Option<Box<dyn Write + Send>>,
    ipc_path: Option<String>,
    events: Option<Receiver<Value>>,
    playback_events: PlaybackEvents,
    progress: PlaybackProgress,
    next_request_id: u64,
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
                Ok((ipc, events)) => {
                    self.child = Some(child);
                    self.ipc = Some(ipc);
                    self.events = Some(events);
                    self.ipc_path = Some(ipc_path);
                    for command in observe_commands() {
                        self.command(command)?;
                    }
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
        self.command_request(command, None)
    }

    fn command_request(
        &mut self,
        command: Value,
        request_id: Option<u64>,
    ) -> Result<(), PlaybackError> {
        self.ensure_started()?;
        let mut payload = match request_id {
            Some(id) => serde_json::to_vec(&json!({"command": command, "request_id": id}))?,
            None => command_payload(command)?,
        };
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
        self.events = None;
        self.playback_events = PlaybackEvents::default();
        self.progress = PlaybackProgress::default();
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
        self.ensure_started()?;
        // Queued positions belong to the replaced track; none of the queued
        // replies or end-file events can match the new load either.
        if let Some(events) = self.events.as_ref() {
            while events.try_recv().is_ok() {}
        }
        self.progress = PlaybackProgress::default();
        self.next_request_id += 2;
        let request_id = self.next_request_id;
        self.playback_events = PlaybackEvents::new(request_id);
        self.command_request(json!(["loadfile", path, "replace"]), Some(request_id))?;
        self.previous_manifest = self.current_manifest.take();
        self.current_manifest = new_manifest;
        // Older mpv versions do not return playlist_entry_id from loadfile.
        // The ordered property reply identifies this load, not a replaced track.
        self.command_request(
            json!(["get_property", "playlist/0/id"]),
            Some(request_id + 1),
        )?;
        self.set_paused(false)?;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), PlaybackError> {
        self.command(json!(["set_property", "pause", paused]))
    }

    fn poll_event(&mut self) -> Option<PlaybackEvent> {
        let events = self.events.as_ref()?;
        loop {
            match events.try_recv() {
                Ok(message) => {
                    if apply_progress(&mut self.progress, &message) {
                        continue;
                    }
                    if let Some(event) = self.playback_events.accept(&message) {
                        return Some(event);
                    }
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    let event = self.playback_events.fail("mpv disconnected".to_owned());
                    self.cleanup();
                    return event;
                }
            }
        }
    }

    fn progress(&self) -> PlaybackProgress {
        self.progress
    }

    fn stop(&mut self) -> Result<(), PlaybackError> {
        self.playback_events = PlaybackEvents::default();
        self.progress = PlaybackProgress::default();
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

const OBSERVED_PROPERTIES: [&str; 2] = ["time-pos", "duration"];

fn observe_commands() -> Vec<Value> {
    OBSERVED_PROPERTIES
        .iter()
        .zip(1..)
        .map(|(property, id)| json!(["observe_property", id, property]))
        .collect()
}

/// Returns whether the message was a position update. mpv reports an
/// unavailable property (for example between tracks) as null.
fn apply_progress(progress: &mut PlaybackProgress, message: &Value) -> bool {
    if message["event"] != "property-change" {
        return false;
    }
    let value = message["data"].as_f64().unwrap_or_default();
    match message["name"].as_str() {
        Some("time-pos") => progress.position_seconds = value,
        Some("duration") => progress.duration_seconds = value,
        _ => return false,
    }
    true
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

type IpcConnection = (Box<dyn Write + Send>, Receiver<Value>);

#[cfg(unix)]
fn connect_ipc(path: &str) -> std::io::Result<IpcConnection> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    let events = read_responses(stream.try_clone()?);
    Ok((Box::new(stream), events))
}

#[cfg(windows)]
fn connect_ipc(path: &str) -> std::io::Result<IpcConnection> {
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let events = read_responses(pipe.try_clone()?);
    Ok((Box::new(pipe), events))
}

#[cfg(not(any(unix, windows)))]
fn connect_ipc(_path: &str) -> std::io::Result<IpcConnection> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "mpv IPC is unsupported on this platform",
    ))
}

fn read_responses(reader: impl Read + Send + 'static) -> Receiver<Value> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // Ignore unrelated metadata and never log IPC payloads/stream URLs.
            let observed = message["event"] == "property-change"
                && message["name"]
                    .as_str()
                    .is_some_and(|name| OBSERVED_PROPERTIES.contains(&name));
            if (message.get("request_id").is_some() || message["event"] == "end-file" || observed)
                && sender.send(message).is_err()
            {
                break;
            }
        }
    });
    receiver
}

/// Correlate EOF with the current load. Replacement/stop events and replies from
/// an earlier load must never advance the new queue.
#[derive(Default)]
struct PlaybackEvents {
    request_id: Option<u64>,
    entry_id: Option<i64>,
    pending_ends: Vec<Value>,
}

impl PlaybackEvents {
    fn new(request_id: u64) -> Self {
        Self {
            request_id: Some(request_id),
            ..Self::default()
        }
    }

    fn fail(&mut self, message: String) -> Option<PlaybackEvent> {
        self.request_id.take()?;
        self.pending_ends.clear();
        Some(PlaybackEvent::Failed(message))
    }

    fn accept(&mut self, message: &Value) -> Option<PlaybackEvent> {
        let request_id = self.request_id?;
        if let Some(reply_id) = message["request_id"].as_u64() {
            if reply_id == request_id || reply_id == request_id + 1 {
                if message["error"].as_str() != Some("success") {
                    return self.fail("mpv could not load the track".to_owned());
                }
                let entry = if reply_id == request_id {
                    message["data"]["playlist_entry_id"].as_i64()
                } else {
                    message["data"].as_i64()
                };
                if let Some(entry) = entry {
                    self.entry_id = Some(entry);
                    let pending = std::mem::take(&mut self.pending_ends);
                    for end in pending {
                        if let Some(event) = self.accept(&end) {
                            return Some(event);
                        }
                    }
                }
            }
        } else if message["event"] == "end-file" {
            let Some(entry) = self.entry_id else {
                self.pending_ends.push(message.clone());
                return None;
            };
            if message["playlist_entry_id"].as_i64() != Some(entry) {
                return None;
            }
            match message["reason"].as_str() {
                Some("eof") => {
                    self.request_id = None;
                    return Some(PlaybackEvent::Finished);
                }
                Some("error") => {
                    return self.fail("mpv could not play the track · n skips it".to_owned());
                }
                _ => {}
            }
        }
        None
    }
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

    fn entry_reply(request_id: u64, entry: i64) -> Value {
        json!({"request_id": request_id + 1, "error": "success", "data": entry})
    }

    fn end(entry: i64, reason: &str) -> Value {
        json!({"event": "end-file", "playlist_entry_id": entry, "reason": reason})
    }

    #[test]
    fn eof_advances_once_and_only_for_the_current_load() {
        let mut events = PlaybackEvents::new(4);
        assert_eq!(events.accept(&entry_reply(2, 10)), None);
        assert_eq!(events.accept(&end(10, "eof")), None);
        assert_eq!(events.accept(&entry_reply(4, 11)), None);
        assert_eq!(events.accept(&end(10, "eof")), None);
        assert_eq!(events.accept(&end(11, "stop")), None);
        assert_eq!(
            events.accept(&end(11, "eof")),
            Some(PlaybackEvent::Finished)
        );
        assert_eq!(events.accept(&end(11, "eof")), None);
    }

    #[test]
    fn short_tracks_can_end_before_the_property_reply() {
        let mut events = PlaybackEvents::new(2);
        events.accept(&end(8, "eof"));
        assert_eq!(
            events.accept(&entry_reply(2, 8)),
            Some(PlaybackEvent::Finished)
        );
    }

    #[test]
    fn modern_loadfile_reply_identifies_the_track_before_the_probe_reply() {
        let mut events = PlaybackEvents::new(2);
        events.accept(
            &json!({"request_id": 2, "error": "success", "data": {"playlist_entry_id": 8}}),
        );
        assert_eq!(events.accept(&end(8, "eof")), Some(PlaybackEvent::Finished));
        assert_eq!(events.accept(&entry_reply(2, 8)), None);
    }

    #[test]
    fn load_and_decoder_errors_do_not_advance_the_queue() {
        let mut events = PlaybackEvents::new(2);
        assert!(matches!(
            events.accept(&json!({"request_id": 2, "error": "loading failed"})),
            Some(PlaybackEvent::Failed(_))
        ));
        assert_eq!(events.accept(&end(8, "eof")), None);
        let mut events = PlaybackEvents::new(4);
        events.accept(&entry_reply(4, 9));
        assert!(matches!(
            events.accept(&end(9, "error")),
            Some(PlaybackEvent::Failed(_))
        ));
        assert_eq!(events.accept(&end(9, "eof")), None);
        let mut events = PlaybackEvents::new(6);
        assert!(matches!(
            events.fail("disconnected".to_owned()),
            Some(PlaybackEvent::Failed(_))
        ));
        assert_eq!(events.fail("disconnected".to_owned()), None);
    }

    #[test]
    fn disconnected_engine_can_be_restarted_and_reports_failure_once() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut engine = MpvEngine::new();
        engine.events = Some(receiver);
        engine.playback_events = PlaybackEvents::new(2);
        assert_eq!(
            engine.poll_event(),
            Some(PlaybackEvent::Failed("mpv disconnected".to_owned()))
        );
        assert!(engine.events.is_none());
        assert_eq!(engine.poll_event(), None);
    }

    #[test]
    fn ipc_reader_filters_metadata_and_parses_newline_delimited_events() {
        let input = std::io::Cursor::new(concat!(
            "not json\n",
            "{\"event\":\"file-loaded\"}\n",
            "{\"event\":\"property-change\",\"name\":\"metadata\",\"data\":{}}\n",
            "{\"request_id\":3,\"error\":\"success\",\"data\":7}\n",
            "{\"event\":\"property-change\",\"id\":1,\"name\":\"time-pos\",\"data\":1.5}\n",
            "{\"event\":\"end-file\",\"playlist_entry_id\":7,\"reason\":\"eof\"}\n"
        ));
        let messages: Vec<_> = read_responses(input).iter().collect();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["request_id"], 3);
        assert_eq!(messages[1]["name"], "time-pos");
        assert_eq!(messages[2], end(7, "eof"));
    }

    fn property(name: &str, data: Value) -> Value {
        json!({"event": "property-change", "name": name, "data": data})
    }

    #[test]
    fn position_and_duration_updates_track_progress_and_null_clears_it() {
        let mut progress = PlaybackProgress::default();
        assert!(apply_progress(
            &mut progress,
            &property("time-pos", json!(30.0))
        ));
        assert!(apply_progress(
            &mut progress,
            &property("duration", json!(120.0))
        ));
        assert_eq!(progress.ratio(), 0.25);
        assert_eq!(progress.whole_seconds(), (30, 120));
        assert!(apply_progress(
            &mut progress,
            &property("time-pos", Value::Null)
        ));
        assert_eq!(progress.position_seconds, 0.0);
        assert!(!apply_progress(
            &mut progress,
            &property("pause", json!(true))
        ));
        assert!(!apply_progress(&mut progress, &end(1, "eof")));
    }

    #[test]
    fn progress_ratio_is_clamped_and_unknown_duration_is_empty() {
        let progress = PlaybackProgress {
            position_seconds: 50.0,
            duration_seconds: 10.0,
        };
        assert_eq!(progress.ratio(), 1.0);
        assert_eq!(PlaybackProgress::default().ratio(), 0.0);
    }

    #[test]
    fn polling_applies_progress_without_hiding_completion_events() {
        let (sender, receiver) = mpsc::channel();
        let mut engine = MpvEngine::new();
        engine.events = Some(receiver);
        engine.playback_events = PlaybackEvents::new(2);
        for message in [
            entry_reply(2, 5),
            property("duration", json!(200.0)),
            property("time-pos", json!(199.5)),
            end(5, "eof"),
            property("time-pos", Value::Null),
        ] {
            sender.send(message).expect("send");
        }
        assert_eq!(engine.poll_event(), Some(PlaybackEvent::Finished));
        assert_eq!(engine.progress().whole_seconds(), (199, 200));
        assert_eq!(engine.poll_event(), None);
        assert_eq!(engine.progress().position_seconds, 0.0);
        engine.events = None;
        assert_eq!(engine.stop().ok(), Some(()));
        assert_eq!(engine.progress(), PlaybackProgress::default());
    }

    #[test]
    fn mpv_observes_only_position_and_duration() {
        assert_eq!(
            observe_commands(),
            [
                json!(["observe_property", 1, "time-pos"]),
                json!(["observe_property", 2, "duration"])
            ]
        );
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
