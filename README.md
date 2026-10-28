# Argentajo

Desktop app (Windows and Linux) that nests jewellery motifs onto silver sheets by their real contour, so the most pieces fit into the least silver, and prints an exact-scale template to glue on the silver and cut with a saw.

## Development

Requirements: Node 24, pnpm, Rust (stable) and the [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm tauri dev
```

Checks:

```sh
pnpm lint && pnpm fmt:check && pnpm build && pnpm test
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Release: bump `version` in the root `Cargo.toml`, commit, then push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`). The Release workflow builds the Windows (NSIS exe) and Linux (AppImage/deb) installers, unsigned, and attaches them to a GitHub Release with notes generated from the merged PRs, plus the `latest.json` the app checks for updates. Pre-releases don't reach the app, which only sees the latest release.

Updates: at startup the app offers to install a newer release; the AppImage and the Windows install update themselves, the .deb doesn't. Updates are signed with a minisign key whose public half is in `src-tauri/tauri.conf.json` and whose private half and password are the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets. Keep a backup of both: without them, installed copies reject every new release and have to be reinstalled by hand. A local `pnpm tauri build` needs the same two variables set, because it signs the updater artifacts.

Layout: `core/` is the pure-Rust domain crate (no Tauri), `src-tauri/` is the thin Tauri app exposing `core` as commands, and `src/` is the React frontend.
