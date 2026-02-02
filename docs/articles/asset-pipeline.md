---
title: The asset pipeline
order: 4
---

# The asset pipeline

Hauchiwa comes batteries-included with a powerful asset pipeline. It treats
assets as first-class citizens in the graph.

## Images

Hauchiwa can automatically resize and convert images to modern formats.

```rust
// Returns Many<Image>
let images = config.load_image()
    .entry("assets/images/*.jpg")
    .entry("assets/images/*.png")
    .format(ImageFormat::WebP)
    .register()?;
```

## Styling (CSS/Sass)

We use `grass`, a high-performance Sass compiler written in Rust.

```rust
// Returns Many<Stylesheet>
let css = config.load_css()
    .entry("assets/style.scss")
    .watch("assets/**/*.scss") // Watch imports for changes
    .minify(true)
    .register()?;
```

Hauchiwa hashes the output filename (e.g., `a1b2c3d4e5f6.css`) for perfect long-term caching.

## Scripts (JS & Svelte)

### JavaScript / TypeScript

Hauchiwa uses `esbuild` for blazingly fast bundling. It supports TypeScript out of the box.

```rust
let js = config.load_js()
    .entry("src/client.ts")
    .bundle(true)
    .minify(true)
    .register()?;
```

### Svelte integration (SSR + hydration)

This is one of Hauchiwa's superpower features. It orchestrates a hybrid
rendering pipeline using Deno.

1. **Server-side rendering (SSR)**: Components are compiled to runs on the
   server (in Rust via Deno) to generate static HTML.
2. **Hydration**: A lightweight client-side script is generated to "wake up" the
   component in the browser.

```rust
#[derive(Clone, Serialize, Deserialize)]
struct CounterProps {
    start: i32,
}

// 1. Load Component
let counters = config.load_svelte::<CounterProps>()
    .entry("components/Counter.svelte")
    .register()?;

// 2. Render in Task
config.task().depends_on(counters).run(|ctx, counters| {
    let component = counters.get("components/Counter.svelte").unwrap();
    
    // Render static HTML
    let html = (component.prerender)(&CounterProps { start: 10 })?;
    
    // 'component.hydration' points to the JS file needed for the browser
    println!("HTML: {}", html);
    Ok(())
});
```

### Import maps

Hauchiwa automatically generates an Import Map, resolving bare specifiers like
`"svelte"` or to their correct, hashed locations in the final build. It just
needs to be included in your the HTML `<head>`.

## Search

Hauchiwa integrates with `pagefind` to generate static search indexes.

```rust
config.load_pagefind()
    .index(pages_a) // One<Vec<Page>>
    .index(pages_b) // One<Vec<Page>>
    .register()?;
```
