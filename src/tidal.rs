use std::collections::{HashMap, HashSet};

use base64::Engine;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::models::{MediaItem, MediaKind};
use crate::playback::{AudioQuality, PlayableResource, PlayableSource};

const API_BASE: &str = "https://openapi.tidal.com/v2";
const PRIVATE_API_BASE: &str = "https://api.tidal.com/v1";
const JSON_API: &str = "application/vnd.api+json";
const LIKED_TRACKS: [&str; 4] = ["userCollectionTracks", "me", "relationships", "items"];
// TIDAL pages collections 20 items at a time; this bounds a misbehaving cursor.
const MAX_COLLECTION_PAGES: usize = 1_000;

#[derive(Debug, Error)]
pub enum TidalError {
    #[error("TIDAL network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("TIDAL API returned {status}: {detail}")]
    Api { status: StatusCode, detail: String },
    #[error("TIDAL returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("full-track playback is disabled pending written permission from TIDAL")]
    FullTrackDisabled,
    #[error("the official preview requires DRM unsupported by tidalbar")]
    PreviewDrmUnsupported,
    #[error("TIDAL did not provide an official preview")]
    PreviewUnavailable,
    #[error("private TIDAL playback is unavailable: {0}")]
    PrivatePlayback(String),
}

#[derive(Clone, Debug)]
pub struct TidalClient {
    http: reqwest::Client,
    access_token: String,
}

impl TidalClient {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.into(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["searchResults"],
                &[
                    ("filter[query]", query),
                    (
                        "include",
                        "topHits,tracks,tracks.artists,albums,albums.artists,albums.coverArt,artists,artists.profileArt,playlists,playlists.coverArt",
                    ),
                ],
            )
            .await?;
        Ok(items_from_document(
            &document,
            Some(&["topHits", "tracks", "albums", "artists", "playlists"]),
        ))
    }

    pub async fn album_items(&self, album_id: &str) -> Result<Vec<MediaItem>, TidalError> {
        self.relationship_items(
            &["albums", album_id, "relationships", "items"],
            "items,items.artists,items.albums,items.albums.coverArt",
        )
        .await
    }

    pub async fn artist_tracks(&self, artist_id: &str) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["artists", artist_id, "relationships", "tracks"],
                &[
                    ("collapseBy", "FINGERPRINT"),
                    (
                        "include",
                        "tracks,tracks.artists,tracks.albums,tracks.albums.coverArt",
                    ),
                ],
            )
            .await?;
        Ok(items_from_document(&document, None))
    }

    pub async fn playlist_items(&self, playlist_id: &str) -> Result<Vec<MediaItem>, TidalError> {
        self.relationship_items(
            &["playlists", playlist_id, "relationships", "items"],
            "items,items.tracks:artists,items.tracks:albums,items.tracks:albums.coverArt",
        )
        .await
    }

    pub async fn track_radio(&self, track_id: &str) -> Result<Vec<MediaItem>, TidalError> {
        self.relationship_items(
            &["tracks", track_id, "relationships", "radio"],
            "radio,radio.artists,radio.albums,radio.albums.coverArt",
        )
        .await
    }

    pub async fn similar_tracks(&self, track_id: &str) -> Result<Vec<MediaItem>, TidalError> {
        self.relationship_items(
            &["tracks", track_id, "relationships", "similarTracks"],
            "similarTracks,similarTracks.artists,similarTracks.albums,similarTracks.albums.coverArt",
        )
        .await
    }

    pub async fn collection_tracks(&self) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["userCollectionTracks", "me", "relationships", "items"],
                &[(
                    "include",
                    "items,items.artists,items.albums,items.albums.coverArt",
                )],
            )
            .await?;
        Ok(items_from_document(&document, None))
    }

    /// Every liked track ID, following the official collection cursor.
    pub async fn liked_track_ids(&self) -> Result<HashSet<String>, TidalError> {
        let mut ids = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_COLLECTION_PAGES {
            let query = cursor
                .as_deref()
                .map(|cursor| vec![("page[cursor]", cursor)])
                .unwrap_or_default();
            let document = self.get(&LIKED_TRACKS, &query).await?;
            ids.extend(
                identifiers(document.get("data"))
                    .into_iter()
                    .filter(|(kind, _)| kind == "tracks")
                    .map(|(_, id)| id),
            );
            match next_cursor(&document) {
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_owned()),
                _ => return Ok(ids),
            }
        }
        Err(TidalError::InvalidResponse(
            "liked-track pagination did not finish".to_owned(),
        ))
    }

    pub async fn set_track_liked(&self, track_id: &str, liked: bool) -> Result<(), TidalError> {
        let method = if liked {
            reqwest::Method::POST
        } else {
            reqwest::Method::DELETE
        };
        let body = serde_json::json!({"data": [{"type": "tracks", "id": track_id}]});
        let response = self
            .http
            .request(method, api_url(&LIKED_TRACKS, &[])?)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::ACCEPT, JSON_API)
            .header(reqwest::header::CONTENT_TYPE, JSON_API)
            .body(body.to_string())
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        write_result(status, &bytes, liked)
    }

    pub async fn collection_albums(&self) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["userCollectionAlbums", "me", "relationships", "items"],
                &[("include", "items,items.artists,items.coverArt")],
            )
            .await?;
        Ok(items_from_document(&document, None))
    }

    pub async fn collection_artists(&self) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["userCollectionArtists", "me", "relationships", "items"],
                &[("include", "items,items.profileArt")],
            )
            .await?;
        Ok(items_from_document(&document, None))
    }

    pub async fn collection_playlists(&self) -> Result<Vec<MediaItem>, TidalError> {
        let document = self
            .get(
                &["userCollectionPlaylists", "me", "relationships", "items"],
                &[("include", "items,items.coverArt")],
            )
            .await?;
        Ok(items_from_document(&document, None))
    }

    pub async fn daily_mixes(&self) -> Result<Vec<MediaItem>, TidalError> {
        self.mix_items("userDailyMixes").await
    }

    pub async fn discovery_mixes(&self) -> Result<Vec<MediaItem>, TidalError> {
        self.mix_items("userDiscoveryMixes").await
    }

    pub async fn new_release_mixes(&self) -> Result<Vec<MediaItem>, TidalError> {
        self.mix_items("userNewReleaseMixes").await
    }

    pub async fn official_preview(&self, track_id: &str) -> Result<PlayableResource, TidalError> {
        let document = self
            .get(
                &["trackManifests", track_id],
                &[
                    ("manifestType", "HLS"),
                    ("formats", "AACLC"),
                    ("uriScheme", "HTTPS"),
                    ("usage", "PLAYBACK"),
                    ("adaptive", "false"),
                ],
            )
            .await?;
        let attributes = document
            .get("data")
            .and_then(|data| data.get("attributes"))
            .ok_or_else(|| TidalError::InvalidResponse("manifest attributes missing".to_owned()))?;
        if attributes.get("trackPresentation").and_then(Value::as_str) != Some("PREVIEW") {
            return Err(TidalError::FullTrackDisabled);
        }
        if attributes.get("drmData").is_some_and(|drm| !drm.is_null()) {
            return Err(TidalError::PreviewDrmUnsupported);
        }
        let uri = attributes
            .get("uri")
            .and_then(Value::as_str)
            .filter(|uri| !uri.is_empty())
            .ok_or(TidalError::PreviewUnavailable)?;
        Ok(PlayableResource {
            source: PlayableSource::Url(uri.to_owned()),
            quality: AudioQuality::Preview,
        })
    }

    /// High Tide's tidalapi uses this undocumented endpoint for full tracks.
    /// Only unencrypted BTS manifests are usable by the current mpv engine.
    pub async fn unofficial_full_track(
        &self,
        track_id: &str,
    ) -> Result<PlayableResource, TidalError> {
        let session = self.private_get(private_url(&["sessions"], &[])?).await?;
        let country_code = session
            .get("countryCode")
            .and_then(Value::as_str)
            .filter(|code| code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .ok_or_else(|| {
                TidalError::InvalidResponse("private session countryCode missing".to_owned())
            })?;
        let session_id = session
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                TidalError::InvalidResponse("private session sessionId missing".to_owned())
            })?;
        let url = private_playback_url(track_id, session_id, country_code)?;
        let document = self.private_get(url).await?;
        full_track_from_document(&document)
    }

    async fn private_get(&self, url: Url) -> Result<Value, TidalError> {
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|error| TidalError::Network(error.without_url()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| TidalError::Network(error.without_url()))?;
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(TidalError::PrivatePlayback(format!(
                "private API returned {status}; this playback login is not authorized for the requested track"
            )));
        }
        if !status.is_success() {
            // Private requests carry a session ID in the URL. Never echo
            // response bodies that might reflect it back into UI or logs.
            return Err(TidalError::PrivatePlayback(format!(
                "private API returned {status}"
            )));
        }
        decode_response(status, &bytes)
    }

    pub async fn artwork(&self, url: &str) -> Result<Vec<u8>, TidalError> {
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    async fn relationship_items(
        &self,
        segments: &[&str],
        include: &str,
    ) -> Result<Vec<MediaItem>, TidalError> {
        let document = self.get(segments, &[("include", include)]).await?;
        Ok(items_from_document(&document, None))
    }

    async fn mix_items(&self, resource: &str) -> Result<Vec<MediaItem>, TidalError> {
        let document = match self.get(&[resource, "me"], &[("include", "items")]).await {
            Ok(document) => document,
            Err(TidalError::Api { status, .. }) if status == StatusCode::NOT_FOUND => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        Ok(items_from_document(&document, Some(&["items"])))
    }

    async fn get(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Value, TidalError> {
        let response = self
            .http
            .get(api_url(segments, query)?)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::ACCEPT, JSON_API)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        decode_response(status, &bytes)
    }
}

fn api_url(segments: &[&str], query: &[(&str, &str)]) -> Result<Url, TidalError> {
    let mut url =
        Url::parse(API_BASE).map_err(|error| TidalError::InvalidResponse(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| TidalError::InvalidResponse("invalid API base URL".to_owned()))?
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query.iter().copied());
    }
    Ok(url)
}

fn next_cursor(document: &Value) -> Option<&str> {
    document
        .get("links")
        .and_then(|links| links.get("meta"))
        .and_then(|meta| meta.get("nextCursor"))
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
}

/// Collection writes may answer with an empty body. Liking an already-liked
/// track is the state the user asked for, not a failure.
fn write_result(status: StatusCode, bytes: &[u8], liked: bool) -> Result<(), TidalError> {
    if status.is_success() {
        return Ok(());
    }
    let error = decode_response(status, bytes).expect_err("unsuccessful status");
    let duplicate = serde_json::from_slice::<Value>(bytes).is_ok_and(|document| {
        document
            .get("errors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|error| {
                error.get("code").and_then(Value::as_str) == Some("DUPLICATE_ITEMS_IN_COLLECTION")
            })
    });
    if liked && status == StatusCode::CONFLICT && duplicate {
        return Ok(());
    }
    Err(error)
}

fn private_url(segments: &[&str], query: &[(&str, &str)]) -> Result<Url, TidalError> {
    let mut url = Url::parse(PRIVATE_API_BASE)
        .map_err(|error| TidalError::InvalidResponse(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| TidalError::InvalidResponse("invalid private API base URL".to_owned()))?
        .extend(segments);
    url.query_pairs_mut().extend_pairs(query.iter().copied());
    Ok(url)
}

fn private_playback_url(
    track_id: &str,
    session_id: &str,
    country_code: &str,
) -> Result<Url, TidalError> {
    private_url(
        &["tracks", track_id, "playbackinfopostpaywall"],
        &[
            ("sessionId", session_id),
            ("countryCode", country_code),
            ("audioquality", "HIGH"),
            ("playbackmode", "STREAM"),
            ("assetpresentation", "FULL"),
        ],
    )
}

fn full_track_from_document(document: &Value) -> Result<PlayableResource, TidalError> {
    if document.get("assetPresentation").and_then(Value::as_str) != Some("FULL") {
        return Err(TidalError::PrivatePlayback(
            "TIDAL returned a short preview instead of a full track; this playback login or track is not entitled to full playback".to_owned(),
        ));
    }
    let encoded = document
        .get("manifest")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            TidalError::InvalidResponse("private playback manifest missing".to_owned())
        })?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| {
            TidalError::InvalidResponse("invalid private playback manifest encoding".to_owned())
        })?;
    let source = match document.get("manifestMimeType").and_then(Value::as_str) {
        Some("application/vnd.tidal.bts") => bts_source(&decoded)?,
        Some("application/dash+xml") => dash_source(&decoded)?,
        _ => {
            return Err(TidalError::PrivatePlayback(
                "unsupported playback manifest format".to_owned(),
            ));
        }
    };
    Ok(PlayableResource {
        source,
        quality: AudioQuality::Full,
    })
}

fn bts_source(decoded: &[u8]) -> Result<PlayableSource, TidalError> {
    let manifest: Value = serde_json::from_slice(decoded).map_err(|_| {
        TidalError::InvalidResponse("invalid private playback manifest JSON".to_owned())
    })?;
    if manifest.get("encryptionType").and_then(Value::as_str) != Some("NONE") {
        return Err(TidalError::PrivatePlayback(
            "encrypted tracks are not supported".to_owned(),
        ));
    }
    let uri = manifest
        .get("urls")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .ok_or_else(|| TidalError::InvalidResponse("private playback URL missing".to_owned()))?;
    if !safe_media_url(uri) {
        return Err(TidalError::InvalidResponse(
            "private playback URL must be HTTPS".to_owned(),
        ));
    }
    Ok(PlayableSource::Url(uri.to_owned()))
}

fn safe_media_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn dash_source(decoded: &[u8]) -> Result<PlayableSource, TidalError> {
    let xml = std::str::from_utf8(decoded)
        .map_err(|_| TidalError::InvalidResponse("invalid DASH manifest encoding".to_owned()))?;
    let doc = roxmltree::Document::parse(xml)
        .map_err(|_| TidalError::InvalidResponse("invalid DASH manifest XML".to_owned()))?;
    if doc.root_element().tag_name().name() != "MPD" {
        return Err(TidalError::InvalidResponse(
            "DASH manifest root is not MPD".to_owned(),
        ));
    }
    let mut media_urls = 0;
    for node in doc.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() == "ContentProtection" {
            return Err(TidalError::PrivatePlayback(
                "encrypted DASH tracks are not supported".to_owned(),
            ));
        }
        if node.tag_name().name() == "BaseURL" && !node.text().is_some_and(safe_media_url) {
            return Err(TidalError::InvalidResponse(
                "DASH BaseURL must be HTTPS".to_owned(),
            ));
        }
        for attribute in node.attributes() {
            if matches!(
                attribute.name(),
                "media" | "initialization" | "sourceURL" | "href"
            ) {
                if !safe_media_url(attribute.value()) {
                    return Err(TidalError::InvalidResponse(
                        "DASH media URLs must be HTTPS".to_owned(),
                    ));
                }
                media_urls += 1;
            }
        }
    }
    if media_urls == 0 {
        return Err(TidalError::InvalidResponse(
            "DASH manifest has no media URLs".to_owned(),
        ));
    }
    Ok(PlayableSource::DashManifest(xml.to_owned()))
}

#[derive(Clone, Debug, Deserialize)]
struct Resource {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    attributes: Value,
    #[serde(default)]
    relationships: HashMap<String, Value>,
}

fn items_from_document(document: &Value, relationship_order: Option<&[&str]>) -> Vec<MediaItem> {
    let resources: Vec<Resource> = document
        .get("included")
        .cloned()
        .and_then(|included| serde_json::from_value(included).ok())
        .unwrap_or_default();
    let by_id: HashMap<(String, String), &Resource> = resources
        .iter()
        .map(|resource| ((resource.kind.clone(), resource.id.clone()), resource))
        .collect();

    let identifiers = if let Some(order) = relationship_order {
        let Some(primary) = document.get("data") else {
            return Vec::new();
        };
        let primaries = primary
            .as_array()
            .map_or_else(|| vec![primary], |items| items.iter().collect());
        primaries
            .into_iter()
            .flat_map(|item| {
                order
                    .iter()
                    .flat_map(|name| relationship_identifiers(item.get("relationships"), name))
                    .collect::<Vec<_>>()
            })
            .collect()
    } else {
        identifiers(document.get("data"))
    };

    identifiers
        .into_iter()
        .filter_map(|identifier| by_id.get(&identifier).copied())
        .filter_map(|resource| media_item(resource, &by_id))
        .collect()
}

fn relationship_identifiers(relationships: Option<&Value>, name: &str) -> Vec<(String, String)> {
    identifiers(
        relationships
            .and_then(|value| value.get(name))
            .and_then(|relationship| relationship.get("data")),
    )
}

fn identifiers(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let values = value
        .as_array()
        .map_or_else(|| vec![value], |items| items.iter().collect());
    values
        .into_iter()
        .filter_map(|item| {
            Some((
                item.get("type")?.as_str()?.to_owned(),
                item.get("id")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn media_item(
    resource: &Resource,
    resources: &HashMap<(String, String), &Resource>,
) -> Option<MediaItem> {
    let (kind, title) = match resource.kind.as_str() {
        "tracks" => (MediaKind::Track, attribute(resource, "title")?),
        "albums" => (MediaKind::Album, attribute(resource, "title")?),
        "artists" => (MediaKind::Artist, attribute(resource, "name")?),
        "playlists" => (MediaKind::Playlist, attribute(resource, "name")?),
        kind if kind.to_ascii_lowercase().contains("mix") => (
            MediaKind::Mix,
            attribute(resource, "title").unwrap_or("TIDAL Mix"),
        ),
        _ => return None,
    };
    let artist_names = relationship_identifiers(Some(&relationships_value(resource)), "artists")
        .into_iter()
        .filter_map(|identifier| resources.get(&identifier))
        .filter_map(|artist| attribute(artist, "name"))
        .collect::<Vec<_>>();
    let subtitle = if !artist_names.is_empty() {
        artist_names.join(", ")
    } else {
        attribute(resource, "description")
            .unwrap_or_else(|| kind_label(&kind))
            .to_owned()
    };
    let artwork_url = artwork_url(resource, resources);

    Some(MediaItem {
        id: resource.id.clone(),
        title: title.to_owned(),
        subtitle,
        kind,
        artwork_url,
        preview_url: None,
    })
}

// Tracks reference their cover art indirectly through their album, so a
// track resource without its own coverArt/profileArt relationship falls back
// to the artwork of its album.
fn artwork_url(
    resource: &Resource,
    resources: &HashMap<(String, String), &Resource>,
) -> Option<String> {
    direct_artwork_url(resource, resources).or_else(|| {
        relationship_identifiers(Some(&relationships_value(resource)), "albums")
            .into_iter()
            .filter_map(|identifier| resources.get(&identifier))
            .find_map(|album| direct_artwork_url(album, resources))
    })
}

fn direct_artwork_url(
    resource: &Resource,
    resources: &HashMap<(String, String), &Resource>,
) -> Option<String> {
    ["coverArt", "profileArt"]
        .into_iter()
        .flat_map(|name| relationship_identifiers(Some(&relationships_value(resource)), name))
        .filter_map(|identifier| resources.get(&identifier))
        .flat_map(|artwork| {
            artwork
                .attributes
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .max_by_key(|file| {
            file.get("meta")
                .and_then(|meta| meta.get("width"))
                .and_then(Value::as_u64)
                .unwrap_or_default()
        })
        .and_then(|file| file.get("href"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn relationships_value(resource: &Resource) -> Value {
    Value::Object(resource.relationships.clone().into_iter().collect())
}

fn attribute<'a>(resource: &'a Resource, name: &str) -> Option<&'a str> {
    resource.attributes.get(name).and_then(Value::as_str)
}

fn kind_label(kind: &MediaKind) -> &'static str {
    match kind {
        MediaKind::Album => "Album",
        MediaKind::Artist => "Artist",
        MediaKind::Mix => "Mix",
        MediaKind::Playlist => "Playlist",
        MediaKind::Radio => "Radio",
        MediaKind::Track => "Track",
    }
}

fn decode_response(status: StatusCode, bytes: &[u8]) -> Result<Value, TidalError> {
    let parsed = serde_json::from_slice::<Value>(bytes);
    if !status.is_success() {
        let detail = parsed.as_ref().map_or_else(
            |_| {
                status
                    .canonical_reason()
                    .unwrap_or("empty API error response")
                    .to_owned()
            },
            api_error_detail,
        );
        return Err(TidalError::Api { status, detail });
    }
    parsed.map_err(|error| {
        TidalError::InvalidResponse(format!(
            "could not decode {status} response as JSON: {error}"
        ))
    })
}

fn api_error_detail(document: &Value) -> String {
    document
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("detail").or_else(|| error.get("code")))
        .and_then(Value::as_str)
        .unwrap_or("unknown API error")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn private_playback_url_encodes_track_id_and_session() {
        let url = private_playback_url("a/b ?", "session", "US").expect("valid URL");
        assert_eq!(url.path(), "/v1/tracks/a%2Fb%20%3F/playbackinfopostpaywall");
        let params: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            params.get("sessionId").map(|value| value.as_ref()),
            Some("session")
        );
        assert_eq!(
            params.get("countryCode").map(|value| value.as_ref()),
            Some("US")
        );
        assert_eq!(
            params.get("audioquality").map(|value| value.as_ref()),
            Some("HIGH")
        );
        assert_eq!(
            params.get("assetpresentation").map(|value| value.as_ref()),
            Some("FULL")
        );
    }

    #[test]
    fn unencrypted_bts_manifest_maps_to_full_track() {
        let manifest = json!({"encryptionType": "NONE", "urls": ["https://cdn.example.test/audio.flac?sig=abc"]});
        let encoded = base64::engine::general_purpose::STANDARD.encode(manifest.to_string());
        let resource = full_track_from_document(&json!({
            "assetPresentation": "FULL", "manifestMimeType": "application/vnd.tidal.bts", "manifest": encoded
        }))
        .expect("unencrypted stream");
        assert_eq!(resource.quality, AudioQuality::Full);
        assert_eq!(
            resource.source,
            PlayableSource::Url("https://cdn.example.test/audio.flac?sig=abc".to_owned())
        );
    }

    #[test]
    fn private_playback_rejects_encryption_and_unsafe_urls() {
        for manifest in [
            json!({"encryptionType": "AES", "urls": ["https://cdn.example.test/track"]}),
            json!({"encryptionType": "NONE", "urls": ["file:///etc/passwd"]}),
            json!({"encryptionType": "NONE", "urls": ["http://localhost/track"]}),
        ] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(manifest.to_string());
            assert!(
                full_track_from_document(&json!({
                    "assetPresentation": "FULL", "manifestMimeType": "application/vnd.tidal.bts", "manifest": encoded
                }))
                .is_err()
            );
        }
        assert!(
            full_track_from_document(
                &json!({"assetPresentation": "FULL", "manifestMimeType": "application/dash+xml"})
            )
            .is_err()
        );
    }

    #[test]
    fn dash_manifest_accepts_unencrypted_https_segments_only() {
        let xml = r#"<MPD mediaPresentationDuration="PT220S"><Period><AdaptationSet><Representation><SegmentTemplate initialization="https://cdn.example.test/init" media="https://cdn.example.test/segment-$Number$"/></Representation></AdaptationSet></Period></MPD>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(xml);
        let result = full_track_from_document(&json!({"assetPresentation":"FULL", "manifestMimeType":"application/dash+xml", "manifest":encoded})).expect("valid DASH");
        assert_eq!(result.source, PlayableSource::DashManifest(xml.to_owned()));
        for bad in [
            xml.replace("https://cdn.example.test/init", "file:///etc/passwd"),
            xml.replace("<Representation>", "<ContentProtection/><Representation>"),
        ] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bad);
            assert!(full_track_from_document(&json!({"assetPresentation":"FULL", "manifestMimeType":"application/dash+xml", "manifest":encoded})).is_err());
        }
    }

    #[test]
    fn downgraded_preview_is_not_reported_as_full_playback() {
        let response = json!({"assetPresentation": "PREVIEW", "audioQuality": "LOW", "manifestMimeType": "application/vnd.tidal.bts"});
        let error = full_track_from_document(&response).expect_err("preview must be rejected");
        assert!(error.to_string().contains("short preview"));
    }

    #[test]
    fn search_document_maps_relationship_order_and_artist_names() {
        let document = json!({
            "data": {
                "type": "searchResults",
                "id": "boards",
                "relationships": {
                    "topHits": {"data": [{"type": "tracks", "id": "track-1"}]},
                    "albums": {"data": [{"type": "albums", "id": "album-1"}]}
                }
            },
            "included": [
                {
                    "type": "albums",
                    "id": "album-1",
                    "attributes": {"title": "Tomorrow's Harvest"},
                    "relationships": {"artists": {"data": [{"type": "artists", "id": "artist-1"}]}}
                },
                {
                    "type": "tracks",
                    "id": "track-1",
                    "attributes": {"title": "Reach for the Dead"},
                    "relationships": {"artists": {"data": [{"type": "artists", "id": "artist-1"}]}}
                },
                {"type": "artists", "id": "artist-1", "attributes": {"name": "Boards of Canada"}}
            ],
            "links": {"self": "..."}
        });

        let items = items_from_document(&document, Some(&["topHits", "albums"]));

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Reach for the Dead");
        assert_eq!(items[0].subtitle, "Boards of Canada");
        assert_eq!(items[1].kind, MediaKind::Album);
    }

    #[test]
    fn search_results_array_maps_included_tracks() {
        let document = json!({
            "data": [{"type": "searchResults", "relationships": {
                "tracks": {"data": [{"type": "tracks", "id": "1"}]}
            }}],
            "included": [{"type": "tracks", "id": "1", "attributes": {"title": "Track"}}]
        });
        let items = items_from_document(&document, Some(&["tracks"]));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Track");
    }

    #[test]
    fn track_artwork_falls_back_to_its_albums_cover_art() {
        let document = json!({
            "data": [{"type": "tracks", "id": "track-1"}],
            "included": [
                {
                    "type": "tracks",
                    "id": "track-1",
                    "attributes": {"title": "Reach for the Dead"},
                    "relationships": {"albums": {"data": [{"type": "albums", "id": "album-1"}]}}
                },
                {
                    "type": "albums",
                    "id": "album-1",
                    "attributes": {"title": "Tomorrow's Harvest"},
                    "relationships": {"coverArt": {"data": [{"type": "artworks", "id": "artwork-1"}]}}
                },
                {
                    "type": "artworks",
                    "id": "artwork-1",
                    "attributes": {
                        "files": [{"href": "https://example.com/cover.jpg", "meta": {"width": 640}}]
                    }
                }
            ]
        });

        let items = items_from_document(&document, None);

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].artwork_url.as_deref(),
            Some("https://example.com/cover.jpg")
        );
    }

    #[test]
    fn liked_track_pages_follow_the_meta_cursor_until_it_is_absent() {
        let page = json!({
            "data": [{"type": "tracks", "id": "1"}, {"type": "videos", "id": "2"}],
            "links": {"next": "/userCollectionTracks/me/relationships/items?page%5Bcursor%5D=20", "meta": {"nextCursor": "20"}}
        });
        assert_eq!(next_cursor(&page), Some("20"));
        assert_eq!(
            next_cursor(&json!({"data": [], "links": {"self": "/"}})),
            None
        );
        assert_eq!(
            next_cursor(&json!({"links": {"meta": {"nextCursor": ""}}})),
            None
        );
    }

    #[test]
    fn collection_url_encodes_the_cursor_and_omits_an_empty_query() {
        let url = api_url(&LIKED_TRACKS, &[]).expect("url");
        assert_eq!(
            url.as_str(),
            "https://openapi.tidal.com/v2/userCollectionTracks/me/relationships/items"
        );
        let url = api_url(&LIKED_TRACKS, &[("page[cursor]", "a&b")]).expect("url");
        assert_eq!(url.query(), Some("page%5Bcursor%5D=a%26b"));
    }

    #[test]
    fn collection_writes_accept_empty_success_and_duplicate_likes_only() {
        assert!(write_result(StatusCode::NO_CONTENT, b"", true).is_ok());
        assert!(write_result(StatusCode::OK, b"{}", false).is_ok());
        let duplicate = br#"{"errors":[{"code":"DUPLICATE_ITEMS_IN_COLLECTION","status":"409"}]}"#;
        assert!(write_result(StatusCode::CONFLICT, duplicate, true).is_ok());
        assert!(write_result(StatusCode::CONFLICT, duplicate, false).is_err());
        let full = br#"{"errors":[{"code":"TOO_MANY_ITEMS_IN_COLLECTION","detail":"Collection item limit reached","status":"409"}]}"#;
        let error = write_result(StatusCode::CONFLICT, full, true).expect_err("limit");
        assert!(error.to_string().contains("Collection item limit reached"));
        assert!(write_result(StatusCode::FORBIDDEN, b"", true).is_err());
    }

    #[test]
    fn api_error_prefers_human_readable_detail() {
        let document =
            json!({"errors": [{"code": "GEO_RESTRICTED", "detail": "Not available here"}]});

        assert_eq!(api_error_detail(&document), "Not available here");
    }

    #[test]
    fn empty_error_response_uses_http_reason() {
        let error = decode_response(StatusCode::NOT_FOUND, &[]).expect_err("must fail");

        assert_eq!(
            error.to_string(),
            "TIDAL API returned 404 Not Found: Not Found"
        );
    }

    #[test]
    fn successful_non_json_response_is_rejected() {
        let error = decode_response(StatusCode::OK, b"not json").expect_err("must fail");

        assert!(
            error
                .to_string()
                .contains("could not decode 200 OK response")
        );
    }
}
