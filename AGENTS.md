# tidalbar agent notes

## Product constraints

- Full-track playback must remain disabled until TIDAL grants written
  permission. Do not add undocumented stream extraction, DRM circumvention, or
  access-control bypasses.
- Official previews are the only currently supported TIDAL playback resources.
- Never commit client secrets, OAuth tokens, stream URLs, or private API
  responses. The distributed application must work as an OAuth public client.
- Linux and macOS are first-class. Keep Windows compiling and tested when
  platform-specific code changes.

## Architecture

- `src/app.rs`: keyboard handling and UI state, independent of network and mpv.
- `src/ui.rs`: Ratatui rendering and responsive layout.
- `src/models.rs`: service-independent media and shelf models.
- `src/artwork.rs`: terminal capability detection and ratatui-image state.
- `src/auth.rs`: OS credential-store boundary for OAuth tokens.
- `src/config.rs`: non-secret platform configuration.
- `src/tidal.rs`: official JSON:API client and response-to-model mapping.
- `src/playback.rs`: `MediaResolver` policy boundary and `AudioEngine`/mpv.

Keep TIDAL API response types inside the service client and map them into the
models in `models.rs`. Keep real network, keyring, and mpv processes out of unit
tests.

## Validation

Run all checks before committing:

```console
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
