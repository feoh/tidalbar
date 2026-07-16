# tidalbar

A keyboard-driven terminal client for [TIDAL](https://tidal.com), built with
Rust and [Ratatui](https://ratatui.rs).

> [!IMPORTANT]
> tidalbar is an independent, early-stage project and is not affiliated with or
> endorsed by TIDAL. Full-track playback is deliberately disabled unless and
> until TIDAL grants written permission. The current playback boundary accepts
> official preview URLs only.

## Vision

tidalbar aims to combine the exploration depth of TIDAL's web application with
fast keyboard access to liked tracks, albums, artists, playlists, and mixes.
The interface starts at a **For You** screen, keeps playback controls visible,
and adapts from artwork-rich wide terminals to compact text-only sessions.

Planned first-class platforms are Linux and macOS. Windows is supported on a
best-effort basis and included in CI.

## Current status

The initial application shell is usable and includes:

- Responsive For You, Explore, Collection, and Playlists views
- Keyboard navigation and search input
- Automatic Kitty, iTerm2, Sixel, or Unicode half-block artwork selection
- A replaceable media-resolver and audio-engine boundary
- Persistent local playback through `mpv` for official previews
- Secure OAuth PKCE login, refresh, and OS credential-store persistence
- Official search, collection, playlist, recommendation-mix, artwork, and
  preview-manifest API integration
- Configuration in the platform-standard user configuration directory

Placeholder content is shown when tidalbar is not authenticated. Album and
artist drill-down, pagination, queue management, and collection mutations remain
under active development.

## Requirements

- Rust 1.88 or newer when building from source
- [`mpv`](https://mpv.io/) available on `PATH`
- A TIDAL subscription for subscriber-only API features
- A TIDAL developer application for API access

## Build and run

```console
git clone https://github.com/feoh/tidalbar.git
cd tidalbar
cargo run
```

Force portable text artwork instead of querying terminal image capabilities:

```console
cargo run -- --no-images
```

Inspect the configuration location or authentication state:

```console
cargo run -- config path
cargo run -- config set-client-id YOUR_PUBLIC_CLIENT_ID
cargo run -- config set-redirect-uri http://127.0.0.1:47831/oauth/callback
cargo run -- auth login
cargo run -- auth status
```

## Keybindings

| Key | Action |
| --- | --- |
| `g` | For You |
| `e` | Explore |
| `c` | Collection |
| `P` | Playlists |
| `/` | Search |
| `h`/`l` or arrows | Move within a shelf |
| `j`/`k` or arrows | Move between shelves |
| `Enter` or `p` | Play the selected official preview |
| `Space` | Pause or resume |
| `q` | Quit |

## Configuration and secrets

The public TIDAL client ID and exact registered redirect URI may be placed in
`config.toml`. OAuth access and refresh tokens are stored in the operating
system credential store rather than that file. tidalbar does not require,
store, or distribute a client secret.

Register the same loopback URI in the TIDAL developer dashboard before running
`tidalbar auth login`. The current callback listener accepts HTTP loopback URIs
using `localhost`, `127.0.0.1`, or another loopback IP; it deliberately rejects
remote redirects.

Never commit TIDAL credentials, OAuth tokens, stream URLs, or captured API
responses containing user data. Development credentials may be injected from a
local secret manager such as 1Password.

## Playback policy

Playback is split into two interfaces:

1. A **media resolver** converts catalog entries into authorized playable
   resources.
2. An **audio engine** sends those resources to a persistent `mpv` process.

Only the official-preview resolver is included today. This separation leaves a
clean integration point for a future TIDAL-approved playback SDK or service
without coupling access policy to the TUI. Code that bypasses DRM, subscription
checks, geographic restrictions, or other access controls is out of scope.

## Development

```console
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## License

MIT. See [LICENSE](LICENSE).
