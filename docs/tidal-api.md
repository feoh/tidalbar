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

The catalog client does not use a client secret. Access and refresh tokens
are stored in the operating system credential store. The configured redirect
must exactly match a redirect registered in TIDAL's developer dashboard.

The separate playback login follows the installed unofficial Python `tidalapi`
package's PKCE flow (as High Tide does), including its client identity, Android
redirect, and legacy scopes. The user's browser login is independent of the
catalog login. Playback tokens are stored under a separate keyring entry and
refreshed through the installed package; no High Tide credentials are copied or
committed. `TIDALBAR_PYTHON` can point to an interpreter with `tidalapi` installed.

## Endpoints used

- `/searchResults?filter[query]=...` with compound `include` paths
- `/userCollectionTracks/me/relationships/items`
- Equivalent collection endpoints for albums, artists, and playlists
- `/userDailyMixes/me`
- `/userDiscoveryMixes/me`
- `/userNewReleaseMixes/me`
- Private `/sessions` provides the session ID and account country for playback

Search text is a query parameter; resource IDs are opaque path segments and
must be URL encoded. Album, artist, playlist, track, and artwork resources
arrive in top-level `included` data and are joined through JSON:API
relationship identifiers.

## Playback

Authenticated playback does not use preview manifests. It gets the session ID
and country code from the private `/sessions` endpoint, then calls the
*undocumented* `api.tidal.com/v1/tracks/{id}/playbackinfopostpaywall` with the
**separate playback token**, session ID, `audioquality=HIGH`,
`assetpresentation=FULL`, and `playbackmode=STREAM`. This matches High Tide's `tidalapi` request, not a
public API contract. The response's `assetPresentation` must actually be `FULL`;
a downgraded `PREVIEW` response is rejected even if HTTP status is 200.
Unencrypted BTS streams use HTTPS media URLs directly. Unencrypted DASH/MPD
manifests with HTTPS segment URLs are staged in a private temporary `.mpd` file
for mpv and removed when playback stops or the process exits. Encrypted and
unsafe manifests are rejected. The legacy `official_preview` method is not
called for authenticated playback.

The developer-app token was observed to receive `PREVIEW`/`LOW` in response to
a `FULL`/`HIGH` request. High Tide instead uses its installed `tidalapi` PKCE
client identity, which may receive a different entitlement. TIDAL does not
support this private endpoint, and even the separate login is not a guarantee
of full playback. No client credentials are copied into tidalbar, and no DRM
bypass is implemented. Doctor checks the actual response without printing signed
media URLs.

## Known unknowns

Public documentation does not clearly specify dynamic loopback-port support,
HTTP localhost exceptions, default page sizes, numeric rate limits, manifest
lifetimes, or whether every track ID is also a manifest ID. The current login
flow therefore uses an exact, explicitly configured fixed loopback URI.
