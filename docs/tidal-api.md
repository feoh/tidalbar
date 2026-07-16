# TIDAL API integration notes

These notes record the public API contract used by tidalbar. Re-check the live
OpenAPI document before changing request or response handling.

## Official references

- [Authorization guide](https://developer.tidal.com/documentation/api-sdk/api-sdk-authorization)
- [Web API reference](https://tidal-music.github.io/tidal-api-reference/)
- [Downloadable OpenAPI document](https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json)
- [Shared Auth specification](https://github.com/tidal-music/tidal-sdk/blob/main/Auth.md)

The API base is `https://openapi.tidal.com/v2`. It uses JSON:API documents and
cursor pagination through `links.next`.

## Authorization

TIDAL implements OAuth 2.1. Native login uses Authorization Code with mandatory
S256 PKCE:

- Authorization endpoint: `https://login.tidal.com/authorize`
- Token endpoint: `https://auth.tidal.com/v1/oauth2/token`
- Requested read-only scopes: `collection.read`, `playback`, `playlists.read`,
  `recommendations.read`, `search.read`, and `user.read`

The distributed client does not use a client secret. Access and refresh tokens
are stored in the operating system credential store. The configured redirect
must exactly match a redirect registered in TIDAL's developer dashboard.

## Third-party endpoints used

- `/searchResults/{query}` with compound `include` paths
- `/userCollectionTracks/me/relationships/items`
- Equivalent collection endpoints for albums, artists, and playlists
- `/userDailyMixes/me`
- `/userDiscoveryMixes/me`
- `/userNewReleaseMixes/me`
- `/trackManifests/{track-id}` for official preview manifests

Search text and all IDs are opaque path segments and must be URL encoded. Album,
artist, playlist, track, and artwork resources arrive in top-level `included`
data and are joined through JSON:API relationship identifiers.

## Playback invariant

The track-manifest endpoint can return either `FULL` or `PREVIEW` in
`attributes.trackPresentation`. tidalbar accepts only `PREVIEW`. A `FULL`
response is rejected before its URI reaches the audio engine. DRM-protected
previews are also rejected because tidalbar has no approved DRM integration.

Full playback remains disabled until TIDAL provides written permission and an
approved integration path.

## Known unknowns

Public documentation does not clearly specify dynamic loopback-port support,
HTTP localhost exceptions, default page sizes, numeric rate limits, manifest
lifetimes, or whether every track ID is also a manifest ID. The current login
flow therefore uses an exact, explicitly configured fixed loopback URI.
