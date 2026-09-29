# TIDAL API integration notes

These notes record the public catalog API and the unsupported private playback
request used by tidalbar. Re-check the live OpenAPI document before changing
public request or response handling.

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

- `/searchResults?filter[query]=...` with compound `include` paths
- `/userCollectionTracks/me/relationships/items`
- Equivalent collection endpoints for albums, artists, and playlists
- `/userDailyMixes/me`
- `/userDiscoveryMixes/me`
- `/userNewReleaseMixes/me`
- `/users/me` for the account country used in private playback requests

Search text is a query parameter; resource IDs are opaque path segments and
must be URL encoded. Album, artist, playlist, track, and artwork resources
arrive in top-level `included` data and are joined through JSON:API
relationship identifiers.

## Playback

Authenticated playback does not use preview manifests. It gets the country code
from the documented `/users/me` resource, then calls the *undocumented*
`api.tidal.com/v1/tracks/{id}/playbackinfopostpaywall` with `audioquality=HIGH`,
`assetpresentation=FULL`, and `playbackmode=STREAM`. This matches the full-track
request made by the `tidalapi` dependency in High Tide; it is not part of the
public API contract. Only unencrypted BTS manifests with HTTPS media URLs can
be handed to mpv. MPD and encrypted manifests produce explicit errors rather
than falling back to a preview. The legacy `official_preview` method remains
in the codebase, but it is not called for authenticated playback.

TIDAL does not support private-API clients and can reject the developer-app
OAuth token currently used for the official catalog. No client credentials are
copied from High Tide or `tidalapi`, and no DRM bypass is implemented. Doctor
checks authorization without printing signed media URLs.

## Known unknowns

Public documentation does not clearly specify dynamic loopback-port support,
HTTP localhost exceptions, default page sizes, numeric rate limits, manifest
lifetimes, or whether every track ID is also a manifest ID. The current login
flow therefore uses an exact, explicitly configured fixed loopback URI.
