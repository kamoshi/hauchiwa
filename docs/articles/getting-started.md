---
title: Getting started
order: 2
---

# Getting Started

To use Hauchiwa, you don't install a CLI tool. Instead, you create a new Rust
binary project that acts as your site generator.

## Installation

First, create a new Rust project:

```bash
cargo new generator
cd generator
```

Then, add `hauchiwa` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
hauchiwa = "*"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
```

## Quick start

This minimal example sets up a graph that reads Markdown files and "renders"
them (prints to log).

Open `src/main.rs` and paste the following:

```rust
use hauchiwa::Blueprint;
use serde::Deserialize;

// 1. Define your Frontmatter
#[derive(Clone, Deserialize, Debug)]
struct Frontmatter {
    title: String,
}

fn main() -> anyhow::Result<()> {
    // 2. Create the Blueprint
    let mut config = Blueprint::<()>::new();

    // 3. Register a Loader (Input)
    // This scans for .md files in the "content" directory
    let pages = config.load_documents::<Frontmatter>()
        .source("content/*.md")
        .register()?;

    // 4. Define a Task (Processing)
    config.task()
        .depends_on(pages)
        .run(|_ctx, pages| {
            // "pages" is a Tracker containing all your markdown files
            for (_path, doc) in pages {
                hauchiwa::tracing::info!("Found page: {}", doc.matter.title);
                // In a real site, you would render HTML here
            }
            Ok(())
        });

    // 5. Run the Website
    config.finish().build(())?;

    Ok(())
}
```

### Running it

Create a dummy content file to test it:

```bash
mkdir content
echo '---\ntitle: Hello Hauchiwa\n---\n# Content' > content/index.md
```

You also need a `public` directory for static assets (even if empty):

```bash
mkdir public
```

Now run your generator:

```bash
cargo run
```

You should see:
```text
Found page: Hello Hauchiwa
```

Congratulations! You have just built your first static site generator.
