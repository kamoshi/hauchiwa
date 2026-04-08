# Hauchiwa

[![Crates.io](https://img.shields.io/crates/v/hauchiwa.svg)](https://crates.io/crates/hauchiwa)
[![Docs.rs](https://docs.rs/hauchiwa/badge.svg)](https://docs.rs/hauchiwa)

A flexible, incremental, graph-based static site generator library for Rust. It
provides the building blocks to create your own custom static site generator
tailored exactly to your needs.

Create tasks and orchestrate them using simple primitives, which are
type-checked by the Rust compiler. Each task can do a different thing, such as
load images, bundle JS, parse markdown, render Jinja templates. This library
already has some built-in tasks, but you can always define custom tasks when
needed.

> The overarching goal is to create an easy-to-use tool with near instantaneous
> rebuilds, which will work forever and be immune to churn by embracing web
> standards. The moment you start using this library you won't ever need to move
> to anything else.


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
        .glob("content/**/*.md")?
        .register();

    let css = config.load_css()
        .entry("styles/**/*.scss")?
        .register();

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
    // Optionally override the output and cache directories (defaults: "dist", ".cache"):
    // let config = config.set_dir_dist("output").set_dir_cache(".build-cache");
    let mut website = config.finish();
    website.build(())?;

    Ok(())
}
```


## Key features

* **Parallel**: Define your build as a graph where tasks are wired together
  using strictly typed handles, ensuring ideal parallelism.
* **Incremental**: By re-executing only chains of dirty tasks, we can avoid
  unneeded rebuilds and deliver near-instant updates.
* **Type-safe**: Dependencies are passed as generic tokens, allowing the Rust
  compiler to enforce that the output type perfectly matches the input type.
* **Content-addressed**: Assets are stored by their hash, guaranteeing
  cache-busting, deduplication, and reliable incremental builds out-of-the-box.


## Built-in support for
* **Static files**: Copy arbitrary file trees into the output directory.
* **Content**: Parse Markdown and Frontmatter safely into strongly-typed Rust structs.
* **Templating**: Render pages using `minijinja` (Jinja2 syntax) templates.
* **CSS/Sass**: Integrate `grass` to compile and minify stylesheets.
* **Images**: Generate optimized multi-format images via the `image` crate.
* **JavaScript**: Bundle and minify JS/TS via `esbuild`.
* **Svelte**: Compile components into separate SSR and hydration scripts.
* **Search**: Static search indexing via `pagefind`.
* **Sitemap**: Sitemap generation via `sitemap-rs`.

> **Need something else?**
> Hauchiwa is designed to be extensible. You can write custom tasks in standard
> Rust to handle anything not included out-of-the-box. The engine automatically
> applies the exact same caching, parallelism, and compile-time type safety to
> custom tasks.

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
- `logging`: Enables `init_logging()`, which sets up a `tracing` subscriber with ANSI colours, uptime timestamps, and progress bar integration.


## License

GPL-2.0 or later.
