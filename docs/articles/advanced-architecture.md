---
title: Development
order: 8
---

# Development

## Watch mode

Call `website.watch(data)` instead of `website.build(data)` to start a
file-watching loop. Hauchiwa runs an initial build, then re-runs affected tasks
whenever a watched file changes.

```rust
match args.mode {
    Mode::Build => { website.build(data)?; }
    Mode::Watch => { website.watch(data)?; }
}
```

Each loader and task declares which file patterns it watches. When a file
changes, only the tasks that depend on that file - directly or transitively -
are re-run. Everything else is served from cache.

Watch mode also starts a WebSocket server on a free port. Inject the live-reload
script into your HTML to have the browser refresh automatically after each rebuild:

```rust
config.task().run(|ctx| {
    let refresh = ctx.env.get_refresh_script()
        .map(|js| format!("<script>{js}</script>"))
        .unwrap_or_default();
    Ok(Output::to("/").html(format!("<h1>Hello</h1>{refresh}"))?)
});
```

`get_refresh_script()` returns JavaScript, so wrap it in a `<script>` element.
The `live` feature enables `watch()`, file watching, and WebSocket live reload.
With `server` also enabled, watch mode serves the output at
`http://localhost:8080/`. `env.port` is the separate WebSocket port.

Watch patterns come from loaders and `.glob()` tasks. For CSS, scripts, and
Svelte, use `.watch()` patterns that include both entry files and imports:
explicit watch patterns replace the default entry patterns. Static-copy sources
are watched too. Rust source changes require restarting your generator.

## Logging

Enable the `logging` feature and call `hauchiwa::init_logging()` at the start of
`main` to get structured log output with ANSI colours, uptime timestamps, and
progress bars for parallel tasks:

```toml
[dependencies]
hauchiwa = { version = "0.22.1", features = ["logging"] }
```

```rust
fn main() -> anyhow::Result<()> {
    hauchiwa::init_logging()?;

    let mut site = Blueprint::<()>::new()
        // ...
        .finish();

    site.build(())?;
    Ok(())
}
```

Without this feature, Hauchiwa emits `tracing` events but does not install a
subscriber - you can bring your own if you have an existing logging setup.

For Cargo-style messages, use
`hauchiwa::init_logging_with_format(hauchiwa::LogFormat::Humane)?` instead.
Both formats respect `RUST_LOG` (default: `info`). Progress bars can be customized
with `Blueprint::set_progress_styles(hauchiwa::ProgressStyles { ..Default::default() })`.

## Diagnostics

`website.build()` returns a `Diagnostics` value containing per-task execution
times. Two built-in renderers are available for visualising where time is spent.

### Mermaid diagram

`render_mermaid` returns a Mermaid graph string with nodes colour-coded by
duration (green = fast, yellow = moderate, red = slow, blue = cached):

```rust
let diagnostics = website.build(data)?;
println!("{}", diagnostics.render_mermaid(&website));
```

Paste the output into [mermaid.live](https://mermaid.live) to see the graph.

### Waterfall chart

`render_waterfall` returns an SVG timeline showing tasks laid out in parallel
lanes with duration labels - useful for spotting bottlenecks in large graphs:

```rust
let diagnostics = website.build(data)?;
diagnostics.render_waterfall_to_file(&website, "build-profile.svg")?;
```
