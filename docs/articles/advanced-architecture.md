---
title: Development
order: 7
---

# Development

## Watch mode

Call `website.watch(data)` instead of `website.build(data)` to start a
file-watching loop. Hauchiwa runs an initial build, then re-runs affected tasks
whenever a watched file changes.

```rust
match args.mode {
    Mode::Build => website.build(data)?,
    Mode::Watch => website.watch(data)?,
}
```

Each loader and task declares which file patterns it watches. When a file
changes, only the tasks that depend on that file - directly or transitively -
are re-run. Everything else is served from cache.

Watch mode also starts a WebSocket server on a free port. Inject the live-reload
script into your HTML to have the browser refresh automatically after each rebuild:

```rust
config.task().merge(|ctx, deps| {
    let refresh = ctx.env.get_refresh_script();  // Some(...) in watch mode, None otherwise
    let script_tag = refresh.unwrap_or_default();
    // include script_tag in your <head>
    Ok(output)
});
```

The `server` feature flag must be enabled for the HTTP development server.
The `live` feature flag must be enabled for the WebSocket live-reload.

## Logging

Enable the `logging` feature and call `hauchiwa::init_logging()` at the start of
`main` to get structured log output with ANSI colours, uptime timestamps, and
progress bars for parallel tasks:

```toml
[dependencies]
hauchiwa = { version = "*", features = ["logging"] }
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
