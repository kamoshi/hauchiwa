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

This minimal example sets up a pipeline that reads Markdown files and converts
them into HTML files.

Open `src/main.rs` and paste the following:

```rust
use hauchiwa::{Blueprint, Output};
use serde::Deserialize;

// 1. Define your Frontmatter
// This matches the YAML at the top of your markdown files.
#[derive(Clone, Deserialize, Debug)]
struct Frontmatter {
    title: String,
}

fn main() -> anyhow::Result<()> {
    // 2. Create the Blueprint
    let mut config = Blueprint::<()>::new();

    // 3. Register a Loader (Input)
    // This scans for .md files in the "content" directory.
    // 'pages' is a Handle representing all future markdown files.
    let pages = config.load_documents::<Frontmatter>()
        .source("content/*.md")
        .register()?;

    // 4. Define a Task (Processing)
    // We use .each().map() to process files one by one.
    // This ensures that if you edit one file, only that file is rebuilt.
    config.task()
        .each(pages)
        .map(|_ctx, doc, ()| {
            // 'doc' is the single document being processed.
            // We create a simple HTML string (in real apps, use a template engine).
            let html = format!(
                "<h1>{}</h1>\n{}", 
                doc.matter.title, 
                doc.content
            );

            // 5. Return Output
            // We tell Hauchiwa to write this string to a file.
            // .meta.path automatically handles clean URLs (e.g., /about/index.html).
            Ok(Output::html(&doc.meta.path, html))
        });

    // 6. Run the Website
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

See the result: Check the newly created `dist/` directory. You will see `dist/index.html`.

You should see:
```html
<h1>Hello World</h1>
This is my first **Hauchiwa** site.
```

Congratulations! You have just built your first static site generator.
