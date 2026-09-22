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

Then, add Hauchiwa, Serde, and a Markdown renderer to your `Cargo.toml`:

```toml
[dependencies]
hauchiwa = "0.22.1"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
comrak = "0.50"
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
        .glob("content/**/*.md")?
        .base("content")
        .register();

    // 4. Define a Task (Processing)
    // We use .each().map() to process files one by one.
    // During watch rebuilds, unchanged documents can reuse their rendered output.
    config.task()
        .each(pages)
        .map(|_ctx, doc, ()| {
            // Hauchiwa parses frontmatter; Comrak renders the Markdown body.
            let html = comrak::markdown_to_html(&doc.text, &comrak::Options::default());

            // 5. Write the page at its public route, derived from .base("content").
            Ok(Output::to(doc).html(html)?)
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
printf -- '---\ntitle: Hello Hauchiwa\n---\n# Content\n' > content/index.md
```

Now run your generator:

```bash
cargo run
```

Check the newly created `dist/` directory (the default output location). You will see `dist/index.html`.

You should see:
```html
<h1>Content</h1>
```

The YAML title is available as `doc.matter.title` for your page template. The
body is available as `doc.text`; the document loader does not render Markdown.

With `.base("content")`, `content/index.md` maps to `/`, and
`content/about.md` maps to `/about/` (`dist/about/index.html`). Without `.base()`,
the source directory remains part of the route. `doc.meta.path` always retains
the original source path, while `doc.meta.href` holds the public route.

Continue with [Building out your site](building-out-your-site.html) to add a
shared template, navigation, CSS, and a live development preview.
