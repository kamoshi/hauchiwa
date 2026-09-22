---
title: Asset pipeline
order: 6
---

# Asset pipeline

Hauchiwa comes batteries-included with a powerful asset pipeline. It treats
assets as first-class citizens in the graph.

## Images

With the `image` feature, Hauchiwa converts images to WebP, AVIF, and PNG and
caches the encoded results across builds. The current loader preserves source
dimensions; it does not expose a resize option.

```rust
use hauchiwa::loader::image::{ImageFormat, Quality};

// Returns Many<Image>
let images = config.load_images()
    .glob("assets/images/*.jpg")?
    .glob("assets/images/*.png")?
    .format(ImageFormat::WebP)
    .format(ImageFormat::Avif(Quality::Lossy(80)))
    .register();
```

The first requested format supplies `image.default`; `image.get(format)` looks
up another format. With no `.format()` calls, WebP is used. `image.width` and
`image.height` contain the source dimensions. Asset paths are root-relative URLs.

## Styling (CSS/Sass)

We use `grass`, a high-performance Sass compiler written in Rust.

```rust
// Returns Many<Stylesheet>
let css = config.load_css()
    .entry("assets/style.scss")?
    .watch("assets/**/*.scss")? // Watch imports for changes
    .minify(true)
    .register();
```

`Stylesheet.path` is a root-relative URL such as `/hash/<hash>.css`; use it
directly in `href`. Minification defaults to `true`. Explicit `.watch()` patterns
replace the default entry patterns, so include entry files as well as imports.
The same watch rule applies to esbuild, Rolldown, and Svelte loaders.

## Static files

Use `Blueprint::copy_static` to copy an entire directory tree into the output
directory as-is. Files whose content has not changed are skipped (mtime + BLAKE3
fallback), so repeated builds stay fast.

```rust
let config = Blueprint::<()>::new()
    .copy_static("assets/fonts", "fonts")   // dist/fonts/  ← assets/fonts/
    .copy_static("assets/icons", "icons");  // dist/icons/  ← assets/icons/
```

The destination path is validated to stay inside the output directory - relative
traversals like `../../etc` are rejected with an error at build time.

## Scripts (JS & Svelte)

### JavaScript / TypeScript

`load_esbuild()` bundles JavaScript and TypeScript using the external `esbuild`
binary, which must be on `PATH`. No Cargo feature is required for this loader.
Bundling and minification both default to `true`.

```rust
let js = config.load_esbuild()
    .entry("src/client.ts")?
    .watch("src/**/*.ts")?
    .bundle(true)
    .minify(true)
    .register();
```

Use `.external()` to mark an npm package as external. Hauchiwa will bundle it
separately and register it in the Import Map so the browser resolves it via a
bare specifier:

```rust
let js = config.load_esbuild()
    .entry("src/app.ts")?
    .external("react")
    .external("react-dom")
    .register();
```

### Native bundling with Rolldown

Enable the `rolldown` feature to use the Rust bundler without an external binary:

```toml
hauchiwa = { version = "0.22.1", features = ["rolldown"] }
```

```rust
let js = config.load_rolldown()
    .entry("src/client.ts")?
    .watch("src/**/*.ts")?
    .bundle(true)
    .minify(true)
    .register();
```

Both script loaders return `Many<Script>`, keyed by entry source path. Look up
`js.get("src/client.ts")?.path` in a dependent task and use the URL as the `src`
of a `<script type="module">` element. Rolldown also supports `.external()`.

### Svelte integration (SSR + hydration)

> **Requires:** `deno` binary on your system `PATH`.

This is one of Hauchiwa's superpower features. It orchestrates a hybrid
rendering pipeline using Deno.

1. **Server-side rendering (SSR)**: Components are compiled and executed by
   Deno subprocesses to generate static HTML.
2. **Hydration**: A lightweight client-side script is generated to "wake up" the
   component in the browser.

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct CounterProps {
    start: i32,
}

// 1. Load Component
let counters = config.load_svelte::<CounterProps>()
    .entry("components/Counter.svelte")?
    .register();

// 2. Render in Task
config.task().using(counters).merge(|ctx, counters| {
    let component = counters.get("components/Counter.svelte")?;
    
    // Render static HTML
    let html = (component.prerender)(&CounterProps { start: 10 })?;
    
    let imports = ctx.importmap.to_html()?;
    let page = format!(
        "{imports}{html}<script type=\"module\" src=\"{}\"></script>",
        component.hydration.path,
    );
    Ok(Output::to("/counter/").html(page)?)
});
```

### Import maps

Loaders register module mappings, such as the Svelte runtime or separately
bundled externals. Tasks receive mappings from their upstream dependencies through
`ctx.importmap`. Include `ctx.importmap.to_html()?` in the HTML `<head>` before
module scripts, and wire the relevant loader into the rendering task with
`.using()`.

## Templates (Minijinja)

Enable the `minijinja` feature to load Jinja2-style templates as a coarse-grained dependency in your graph.

```toml
hauchiwa = { version = "0.22.1", features = ["minijinja"] }
```

```rust
use hauchiwa::loader::TemplateEnv;

// Returns One<TemplateEnv>
let templates = config.load_minijinja()
    .glob("templates/**/*.html")?
    .root("templates")
    .register();

config.task().using(templates).merge(|ctx, env| {
    let tmpl = env.get_template("base.html")?;
    let html = tmpl.render(hauchiwa::minijinja::context! { title => "Hello" })?;
    Ok(Output::to("/").html(html)?)
});
```

`.root("templates")` makes `templates/base.html` available as `base.html`.
Without `.root()`, names include the matched path (`templates/base.html`).

Use `.filter()` to register custom Jinja filters before the environment is built:

```rust
let templates = config.load_minijinja()
    .glob("templates/**/*.html")?
    .root("templates")
    .filter("shout", |s: String| s.to_uppercase())
    .register();
```

Any change to a watched template file causes the loader to re-execute and all dependent tasks to re-run.

## Search

Enable `pagefind` to generate static search indexes from rendered outputs.

```rust
config.use_pagefind()
    .index(pages_a) // Many<Output>
    .index(pages_b) // One<Vec<Output>>
    .register();
```

Both Pagefind and sitemap builders accept `One<Output>`, `One<Vec<Output>>`, or
`Many<Output>`. Pass rendered output handles, not document-loader handles.

## Sitemap

Enable the `sitemap` feature, then register the page outputs to include:

```rust
use hauchiwa::loader::sitemap::ChangeFrequency;

config.use_sitemap("https://example.org")
    .add(pages, ChangeFrequency::Weekly, 0.8)
    .register();
```

URLs are derived from output paths. The builder returns `One<Vec<Output>>`,
writing `sitemap.xml` or splitting large collections into multiple sitemaps.
