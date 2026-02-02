---
title: Reference
order: 6
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
hauchiwa = { version = "0.12.0", default-features = false, features = ["grass"] }
```

### Available Features

| Feature    | Default | Description |
| :---       | :---:   | :---        |
| `grass`    | Yes     | Enables Sass/SCSS compilation via the `grass` crate. |
| `image`    | Yes     | Enables image optimization (resize, convert) via the `image` crate. |
| `tokio`    | Yes     | Enables async runtime support (required for `server` and `pagefind`). |
| `live`     | Yes     | Enables live reload functionality (WebSocket + File Watching). |
| `server`   | Yes     | Enables the development http server (`axum`). |
| `pagefind` | Yes     | Enables static search indexing via `pagefind`. |
| `sitemap`  | Yes     | Enables sitemap generation via `sitemap-rs`. |
