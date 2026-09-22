# Hauchiwa Docs

This site is built with the local Hauchiwa checkout (`path = ".."`). Articles
live in `articles/`, the generator in `src/main.rs`, and styles in `assets/`.
The examples describe Hauchiwa 0.22.1.

From this directory, run:

- `make build` — build once into `dist/` (equivalent to `cargo run --release`).
- `make watch` — rebuild on article/style changes and serve at
  <http://localhost:8080/> (equivalent to `cargo run --release -- watch`).

Run these commands from `docs/` because source paths are relative to the working
directory. The generator uses the `grass`, `live`, `server`, and `logging`
features; it does not require esbuild or Deno. Restart it after changing Rust source.

Logging defaults to `info`. Set `RUST_LOG=debug` for more detail, for example
`RUST_LOG=debug make build`.

Generated files in `dist/`, `.cache/`, and `target/` are not documentation sources.
