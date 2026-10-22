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

Release: bump `version` in the root `Cargo.toml`, commit, then push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`). Use plain `x.y.z` versions, because the MSI installer rejects pre-release suffixes like `-beta`. The Release workflow builds the Windows (msi/exe) and Linux (AppImage/deb) installers, unsigned, and attaches them to a GitHub Release.

Layout: `core/` is the pure-Rust domain crate (no Tauri), `src-tauri/` is the thin Tauri app exposing `core` as commands, and `src/` is the React frontend.
