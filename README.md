# Hauchiwa

[![Crates.io](https://img.shields.io/crates/v/hauchiwa.svg)](https://crates.io/crates/hauchiwa)
[![Docs.rs](https://docs.rs/hauchiwa/badge.svg)](https://docs.rs/hauchiwa)

A flexible, incremental, graph-based static site generator library for Rust. It
provides the building blocks to create your own custom static site generator
tailored exactly to your needs.

Unlike traditional SSGs that force a specific directory structure or build pipeline,
Hauchiwa gives you a **Task Graph**. You define the inputs (files, data), the transformations
(markdown parsing, image optimization, SCSS compilation), and the dependencies between them.
Hauchiwa handles the parallel execution, caching, and incremental rebuilds.

If you are tired of:
- Rigid frameworks that force their file structure on you (Jekyll, Hugo).
- Complex config files that are hard to debug.
- Bloated JavaScript bundles for simple static content.

Then Hauchiwa is for you.

## Key Features

* **Task Graph Architecture**: Define your build as a dependency graph. If Task
  B needs Task A, Hauchiwa ensures they run in order.
* **Incremental**: Because it knows the graph, Hauchiwa only rebuilds what has
  really changed.
* **Parallel**: Tasks run in parallel automatically.
* **Type-safe**: Leveraging Rust's type system to ensure dependencies are sound.
* **Asset pipeline**: Built-in support for:
  * **Images**: Automatic optimization via `image` and caching.
  * **Sass/SCSS**: Compilation via `grass`.
  * **JavaScript**: Bundling and minification via `esbuild`.
  * **Svelte**: SSR and hydration support via `deno` and `esbuild`.
  
## Core Concepts

- **[SiteConfig]**: The blueprint of your site. You use this to register tasks
  and loaders.
- **[Task](crate::task::Task)**: A single unit of work. Tasks can depend on
  other tasks.
- **[Handle](crate::task::Handle)**: A reference to the future result of a task.
  You pass these to other tasks to define dependencies.
- **[Loader](crate::loader)**: A kind of a task that reads data from the
  filesystem (e.g., markdown files, images).
- **[Site]**: The engine that converts the graph defined in `SiteConfig` into a
  proper static website.

## Quick Start

Add `hauchiwa` to your `Cargo.toml`:

```toml
[dependencies]
hauchiwa = "*" # Check crates.io for the latest version
serde = { version = "1", features = ["derive"] }
```

Create your generator in `src/main.rs`:

```rust,no_run
use hauchiwa::{SiteConfig, Site, Page};
use hauchiwa::loader::{self, Content};
use serde::Deserialize;


// 1. Define your content structure (Frontmatter)
#[derive(Deserialize, Clone)]
struct Post {
    title: String,
}

fn main() -> anyhow::Result<()> {
    // 2. Create the configuration
    // We explicitly specify the global data type as `()` since we don't have any shared state yet.
    let mut config: SiteConfig<()> = SiteConfig::new();

    // 3. Add a loader to glob markdown files
    // `posts` is a Handle<Registry<Content<Post>>>
    let posts = config.glob_content::<Post>("content/**/*.md")?;

    // 4. Define a task to render pages
    // We declare that this task depends on `posts`.
    hauchiwa::task!(config, |ctx, posts| {
        let mut pages = Vec::new();

        // Iterate over loaded posts
        for post in posts.values() {
            let html_content = format!("<h1>{}</h1>", post.metadata.title);
            
            // Create a page structure
            pages.push(Page::html(&post.path, html_content));
        }

        Ok(pages)
    });

    // 5. Build the site
    let mut site = Site::new(config);
    site.build(())?;

    Ok(())
}
```

## Feature Flags

- `asyncrt`: Enables the Tokio runtime for async tasks.
- `styles`: Enables SCSS/Sass compilation.
- `images`: Enables image optimization (WebP, resizing).
- `reload`: Enables live-reload during development.
- `server`: Enables the built-in development server.

## Documentation

The best place to learn is the [API Documentation](https://docs.rs/hauchiwa). It
covers the core concepts in depth:
- **[SiteConfig]**: How to wire up your graph.
- **[Loader]**: How to read files.
- **[Task]**: How to process data.

## License

GPL-2.0 or later.
