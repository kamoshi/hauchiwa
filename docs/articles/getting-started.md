---
title: Getting Started
order: 2
---

# Getting Started

To use Hauchiwa, you create a new Rust binary project that acts as your site generator.

## Installation

Add `hauchiwa` to your `Cargo.toml`:

```toml
[dependencies]
hauchiwa = "0.12.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
```

## Basic Example

Here is a minimal example of a Hauchiwa build script:

```rust
use hauchiwa::Blueprint;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
struct Frontmatter {
    title: String,
}

fn main() -> anyhow::Result<()> {
    let mut config = Blueprint::<()>::new();

    // 1. Load Markdown files
    let pages = config.load_documents::<Frontmatter>()
        .source("content/*.md")
        .register()?;

    // 2. Define a build task
    config.task()
        .depends_on(pages)
        .run(|_ctx, pages| {
            // Process pages and generate output
            for page in pages {
                println!("Processing page: {}", page.matter.title);
                // In a real app, you would render HTML and return outputs
            }
            Ok(())
        });

    // 3. Execute
    config.finish().build(())?;

    Ok(())
}
```

## Running the Build

Simply run your binary:

```bash
cargo run
```

This will execute the defined tasks. Hauchiwa automatically handles parallelism and caching.
