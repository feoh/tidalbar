use std::collections::HashMap;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::models::{MediaItem, MediaKind};
use crate::playback::{AudioQuality, PlayableResource};

const API_BASE: &str = "https://openapi.tidal.com/v2";
const JSON_API: &str = "application/vnd.api+json";

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
                &["searchResults", query],
                &[(
                    "include",
                    "topHits,tracks,tracks.artists,albums,albums.artists,albums.coverArt,artists,artists.profileArt,playlists,playlists.coverArt",
                )],
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
            uri: uri.to_owned(),
            quality: AudioQuality::Preview,
        })
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
        let document = match self
            .get(
                &[resource, "me"],
                &[(
                    "include",
                    "items,items.artists,items.albums,items.coverArt,items.profileArt",
                )],
            )
            .await
        {
            Ok(document) => document,
            Err(TidalError::Api { status, .. }) if status == StatusCode::NOT_FOUND => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        Ok(items_from_document(&document, Some(&["items"])))
    }

    async fn get(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Value, TidalError> {
        let mut url =
            Url::parse(API_BASE).map_err(|error| TidalError::InvalidResponse(error.to_string()))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| TidalError::InvalidResponse("invalid API base URL".to_owned()))?;
            path.extend(segments);
        }
        url.query_pairs_mut().extend_pairs(query.iter().copied());

        let response = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::ACCEPT, JSON_API)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        decode_response(status, &bytes)
    }
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
        order
            .iter()
            .flat_map(|name| relationship_identifiers(primary.get("relationships"), name))
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
    let artist_names = relationship_identifiers(
        Some(&Value::Object(
            resource.relationships.clone().into_iter().collect(),
        )),
        "artists",
    )
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
    let artwork_url = ["coverArt", "profileArt"]
        .into_iter()
        .flat_map(|name| {
            relationship_identifiers(
                Some(&Value::Object(
                    resource.relationships.clone().into_iter().collect(),
                )),
                name,
            )
        })
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
        .map(ToOwned::to_owned);

    Some(MediaItem {
        id: resource.id.clone(),
        title: title.to_owned(),
        subtitle,
        kind,
        artwork_url,
        preview_url: None,
    })
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
