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
use hauchiwa::{Blueprint, Output};
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
    
    let css = config.load_css()
        .entry("styles/**/*.scss")
        .register()?;

    // 4. Define a task to render pages
    // We declare that this task depends on `posts`.
    config
        .task()
        .each(posts)
        .using(css)
        // Iterate over loaded posts
        .map(|_, post, css| {
            // retrieve css bundle path
            let css = css.get("styles/main.scss")?;

            // format html
            let html_content = format!(
                "<link rel=\"stylesheet\" href=\"/{}\"><h1>{}</h1>",
                css.path, post.matter.title
            );

            // Output::html creates pretty URLs (e.g., /foo/index.html)
            Ok(Output::html(&post.meta.path, html_content))
        });

    // 5. Build the website
    let mut website = config.finish();
    website.build(())?;

    Ok(())
}
```

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
  compiler to enforce that the output type perfectly matches the input type.
* **Asset pipeline**: Built-in support for:
  * **Images**: Automatically generates multi-format sources (WebP, AVIF) via the `image` crate.
  * **CSS/Sass**: Integrates `grass` to compile and minify stylesheets, outputting CSS bundles.
  * **JavaScript**: Bundling and minification via `esbuild`.
  * **Svelte**: Orchestrates Deno to compile components into separate SSR and hydration scripts,
    automatically propagating import maps for seamless client-side interactivity.
  * **Search**: Static search indexing via `pagefind`.
  * **Sitemap**: Sitemap generation via `sitemap-rs`.
  
## Core Concepts

- **Blueprint**: The blueprint of your site. You use this to register tasks and loaders.
- **Task**: A single unit of work. Tasks can depend on other tasks.
  - **Coarse-grained**: Tasks that produce a single output.
  - **Fine-grained**: Tasks that produce multiple outputs.
- **Handle**: A reference to the future result of a task. You pass these to other tasks to define dependencies.
  - **One**: A handle to a single (coarse-grained) output.
  - **Many**: A handle to multiple (fine-grained) outputs.
- **Loader**: A kind of a task that reads data from the filesystem (e.g., markdown files, images).
- **Website**: The engine that converts the graph defined in `Blueprint` into a proper static website.

## Documentation

Introduction is available in the `docs/` directory (run `make watch`), or you
can visit the [online version](https://hauchiwa.kamoshi.org). The best place to
learn the API is the [full documentation](https://docs.rs/hauchiwa). It covers
the available features in depth.

## Examples

Some examples are available in the `examples/` directory.

In addition, there are also some real-world examples:
- [hauchiwa.kamoshi.org](https://github.com/kamoshi/hauchiwa/tree/main/docs)
- [kamoshi.org](https://github.com/kamoshi/kamoshi.org)

## Feature flags

Default features (opt out if not needed):

- `grass`: Enables SCSS/Sass compilation.
- `image`: Enables image optimization (WebP, resizing).
- `tokio`: Enables the Tokio runtime for async tasks.
- `live`: Enables live-reload during development.
- `server`: Enables the built-in development server.

Opt-in features:

- `pagefind`: Enables static search indexing.
- `sitemap`: Enables `sitemap.xml` generation.
- `minijinja`: Enables Jinja2-style template loading.

## License

GPL-2.0 or later.
