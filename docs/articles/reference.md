---
title: Reference
order: 9
---

# Reference

## API Reference

The full API documentation is available on docs.rs:

[<https://docs.rs/hauchiwa>](https://docs.rs/hauchiwa)

## Feature flags

Hauchiwa uses feature flags to let you opt-in to expensive dependencies.

To keep your build times low, you can disable features you don't need in `Cargo.toml`:

```toml
[dependencies]
hauchiwa = { version = "0.22.1", default-features = false, features = ["grass"] }
```

### Available Features

| Feature     | Default | Description |
| :---        | :---:   | :---        |
| `grass`     | Yes     | Enables Sass/SCSS compilation via the `grass` crate. |
| `image`     | Yes     | Enables image conversion (WebP, AVIF, PNG) via the `image` crate. |
| `tokio`     | Yes     | Enables async runtime support (required for `server` and `pagefind`). |
| `live`      | Yes     | Enables live reload functionality (WebSocket + file watching). |
| `server`    | Yes     | Enables the development HTTP server (`axum`). |
| `rolldown`  | No      | Enables native Rust JS/TS bundling via Rolldown. |
| `pagefind`  | No      | Enables static search indexing via `pagefind`. |
| `sitemap`   | No      | Enables sitemap generation via `sitemap-rs`. |
| `minijinja` | No      | Enables Jinja2-style template loading via `minijinja`. |
| `logging`   | No      | Enables `init_logging()`: ANSI tracing subscriber with progress bar integration. |

`server` and `pagefind` enable `tokio` automatically. `server` alone does not
provide `watch()`; enable `live` as well for the development loop.

## External tools

`load_esbuild()` and `load_svelte()` are available without optional Cargo
features, but require `esbuild` and `deno`, respectively, on `PATH`. Hauchiwa
checks registered binary requirements before starting a build or watch session.
`load_rolldown()` needs the `rolldown` feature but no external bundler binary.
