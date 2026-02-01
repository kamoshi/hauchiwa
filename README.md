# Hauchiwa

[![Crates.io](https://img.shields.io/crates/v/hauchiwa.svg)](https://crates.io/crates/hauchiwa)
[![Docs.rs](https://docs.rs/hauchiwa/badge.svg)](https://docs.rs/hauchiwa)

A flexible, incremental, graph-based static site generator library for Rust. It
provides the building blocks to create your own custom static site generator
tailored exactly to your needs.

Unlike traditional SSGs that force a specific directory structure or build pipeline,
Hauchiwa gives you a **task graph**. You define the inputs (files, data), the transformations
(markdown parsing, image optimization, SCSS compilation), and the dependencies between them.
Hauchiwa handles the parallel execution, caching, and incremental rebuilds.

If you are tired of:
- Rigid frameworks that force their file structure on you (Jekyll, Hugo).
- Complex config files that are hard to debug.
- Bloated JavaScript bundles for simple static content.

Then Hauchiwa is for you.

## Key Features

* **Graph-based**: Define your build as a graph where tasks are wired together
  using strictly typed handles rather than rigid file paths. This structure
  automatically resolves complex dependencies, ensuring shared ancestor tasks
  execute exactly once before efficiently distributing their results.
* **Incremental**: The engine identifies the specific task responsible for a
  changed file and marks only its dependent subgraph as "dirty". By re-executing
  only this precise chain of tasks, the system avoids wasteful full rebuilds and
  delivers near-instant updates.
* **Parallel**: A threaded execution engine schedules tasks to run on a thread
  pool the moment their dependencies are resolved. This saturates your CPU cores
  automatically, processing heavy assets and content concurrently without manual
  async orchestration.
* **Type-safe**: Dependencies are passed as generic tokens, allowing the Rust
  compiler to enforce that a producer's output type perfectly matches a
  consumer's input. Advanced static verification prevents broken builds by
  catching data flow errors at compile time rather than runtime.
* **Asset pipeline**: Built-in support for:
  * **[Images](crate::loader::image)**: Automatically generates multi-format
    sources (WebP, AVIF) with content-addressed hashing for immutable caching
    via the `image` crate.
  * **[CSS/Sass](crate::loader::css)**: Integrates `grass` to compile and
    minify stylesheets, outputting hashed CSS bundles that are ready for
    aggressive browser caching.
  * **[JavaScript](crate::loader::js)**: Bundling and minification via `esbuild`.
  * **[Svelte](crate::loader::svelte)**: Orchestrates Deno to compile components
    into separate SSR and hydration scripts, automatically propagating import
    maps for seamless client-side interactivity.
  * **[Search](crate::loader::pagefind)**: Static search indexing via `pagefind`.
  * **[Sitemap](crate::loader::sitemap)**: Sitemap generation via `sitemap-rs`.
  
## Core Concepts

- **[Blueprint](crate::Blueprint)**: The blueprint of your site. You use this to
  register tasks and loaders.
- **Task**: A single unit of work. Tasks can depend on other
  tasks.
  - **Coarse-grained**: Tasks that produce a single output.
  - **Fine-grained**: Tasks that produce multiple outputs.
- **Handle**: A reference to the future result of a task. You pass these to
  other tasks to define dependencies.
  - **[One](crate::One)**: A handle to a single (coarse-grained) output.
  - **[Many](crate::Many)**: A handle to multiple (fine-grained) outputs.
- **[Loader](crate::loader)**: A kind of a task that reads data from the
  filesystem (e.g., markdown files, images).
- **[Website](crate::Website)**: The engine that converts the graph defined in
  `Blueprint` into a proper static website.

## Quick Start

Add `hauchiwa` to your `Cargo.toml`:

```toml
[dependencies]
# Check crates.io for the latest version
hauchiwa = "*"
# Serde is needed to parse frontmatter
serde = { version = "1", features = ["derive"] }
```

Create your generator in `src/main.rs`:

```rust,no_run
use hauchiwa::{Blueprint, Website, Output};
use serde::Deserialize;


// 1. Define your content structure (Frontmatter)
#[derive(Deserialize, Clone)]
struct Post {
    title: String,
}

fn main() -> anyhow::Result<()> {
    // 2. Create the configuration
    // We explicitly specify the global data type as `()`
    // since we don't have any global state yet.
    let mut config = Blueprint::<()>::new();

    // 3. Add a loader to glob markdown files
    // `posts` is `Many<Document<Post>>`
    let posts = config.load_documents::<Post>()
        .source("content/**/*.md")
        .register()?;

    // 4. Define a task to render pages
    // We declare that this task depends on `posts`.
    config.task()
        .depends_on(posts)
        .run(|_, posts| {
        let mut pages = Vec::new();

        // Iterate over loaded posts
        for post in posts.values() {
            let html_content = format!("<h1>{}</h1>", post.matter.title);
            
            // Create a page structure
            // Output::html creates pretty URLs (e.g., /foo/index.html)
            pages.push(Output::html(&post.meta.path, html_content));
        }

        Ok(pages)
    });

    // 5. Build the website
    let mut website = config.finish();
    website.build(())?;

    Ok(())
}
```

## Feature flags

By default, Hauchiwa is built with the following features, but you can opt out
of them by disabling them in your `Cargo.toml` file, if you don't need them.

- `grass`: Enables SCSS/Sass compilation.
- `image`: Enables image optimization (WebP, resizing).
- `tokio`: Enables the Tokio runtime for async tasks.
- `live`: Enables live-reload during development.
- `server`: Enables the built-in development server.
- `pagefind`: Enables static search indexing.
- `sitemap`: Enables `sitemap.xml` generation.

## Documentation

The best place to learn is the [API Documentation](https://docs.rs/hauchiwa). It
covers the core concepts in depth.

## Examples

- [kamoshi.org](https://github.com/kamoshi/kamoshi.org)

## License

GPL-2.0 or later.
