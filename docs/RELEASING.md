# Releasing tidalbar

Releases are tagged commits on `main` with a published GitHub release.
Publishing the release triggers `.github/workflows/release.yml`, which builds,
tests, and attaches binaries for Linux x86-64, macOS Apple Silicon and Intel,
and Windows x86-64 with SHA-256 checksums. Nothing is published to crates.io.

## Checklist

Use semantic versions: bump the patch version for fixes and small features, and
the minor version for larger features or behavior changes.

1. Start from a clean, up-to-date `main` with all feature work committed:

   ```console
   git status --short
   git fetch origin && git status -sb
   gh auth status
   ```

2. Bump `version` in `Cargo.toml`, then refresh `Cargo.lock` (the release
   build uses `--locked`):

   ```console
   cargo build
   git diff --stat   # expect Cargo.toml and Cargo.lock
   ```

   The release workflow fails if `tidalbar --version` does not equal the tag
   without its `v`, so the tag and `Cargo.toml` must agree.

3. Update the example tag in the README's `gh workflow run release.yml -f tag=...`
   command.

4. Run the full validation:

   ```console
   cargo fmt --all --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```

5. Commit as `Release vX.Y.Z`, push, and wait for CI on Linux, macOS, and
   Windows to pass before tagging:

   ```console
   git commit -am "Release vX.Y.Z"
   git push origin main
   sleep 10   # let GitHub register the run before looking it up
   gh run watch "$(gh run list --workflow CI --branch main --commit "$(git rev-parse HEAD)" --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
   ```

6. Create and push an annotated tag on that commit:

   ```console
   git tag -a vX.Y.Z -m "tidalbar vX.Y.Z"
   git push origin vX.Y.Z
   ```

7. Write release notes, then publish the release. Follow the earlier releases
   (`gh release view v0.1.3`):
   - A `##` heading summarizing the main feature, with bullets for user-visible
     changes and new keys.
   - A sentence on what was validated (local checks, CI platforms, any live
     mpv or `tidalbar doctor` checks).
   - Known limitations, then the standard paragraph about attached binaries:
     mpv and Python `tidalapi` are still separate requirements, and macOS
     binaries are not signed or notarized.
   - A `## Install or upgrade from source` section with
     `cargo install --git https://github.com/feoh/tidalbar.git --tag vX.Y.Z --locked --force`
     and a note on whether existing logins remain valid.

   ```console
   gh release create vX.Y.Z --verify-tag --title "tidalbar vX.Y.Z" --notes-file /tmp/tidalbar-notes.md
   rm /tmp/tidalbar-notes.md
   ```

8. Watch the binary build and confirm that all eight assets (four archives and
   four `.sha256` files) are attached:

   ```console
   sleep 10
   gh run watch "$(gh run list --workflow release.yml --event release --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
   gh release view vX.Y.Z --json assets --jq '.assets[].name'
   ```

## Recovery

- If the binary workflow fails after the release is published, fix the cause on
  `main` and rerun it for the existing tag:
  `gh workflow run release.yml -f tag=vX.Y.Z`. Uploads use `--clobber`, so
  rerunning replaces earlier assets.
- If the tagged source itself is wrong, do not move a published tag. Fix it on
  `main` and release the next patch version.
