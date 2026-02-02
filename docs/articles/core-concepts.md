---
title: Core concepts
order: 3
---

# Core concepts

This section covers the mechanics of Hauchiwa's "graph" architecture.

## The Blueprint

The `Blueprint` is the architectural drawing board where you define your site.
It is the central registry where you add tasks and configure loaders.

```rust
let mut config = Blueprint::<()>::new();
```

The generic parameter (`<T>`) allows you to pass a shared context (like a global
configuration) to every task, though `()` is common for simple sites.

## Tasks and handles

In Hauchiwa, everything is a **Task**. Tasks take input, process it, and produce
output.

To wire tasks together, we use **Handles**. When you register a task (or a
loader), you get a Handle back. This Handle acts as a token that represents
the future output of that task.

### The handle system

Hauchiwa strictly types these handles to ensure your graph is valid:

* **`One<T>`**: Represents a single unit of data.
  * *Example*: A generated sitemap, or a listing of pages.
  * *Behavior*: If the source changes, tasks depending on it are re-run.
* **`Many<T>`**: Represents a collection of items (fine-grained).
  * *Example*: A collection of Markdown blog posts.
  * *Behavior*: Enables **surgical updates**. If you have 100 blog posts and
    edit *just one*, tasks depending on this `Many<T>` handle can be skipped if
    they don't depend on the modified item.

### Wiring dependencies

You use `.depends_on()` to connect tasks. This is where the magic happens. The
type system ensures that the data produced by the upstream task matches what the
downstream task expects.

```rust
// 'pages' is a Many<Document> handle
let pages = config.load_documents::<Frontmatter>().source("*.md").register()?;

config.task()
    .depends_on(pages) // We pass the handle here
    .run(|ctx, pages| {
        // 'pages' is now resolved to the actual data (Tracker<Document>)
        Ok(())
    });
```

## Loaders (input)

Loaders are special tasks that bridge the gap between the FileSystem and the
Graph. They ingest files and turn them into typed data structures.

Common loaders include:
* `load_documents`: For Markdown with Frontmatter.
* `load_css`: For SCSS/CSS.
* `load_js`: For TypeScript/JavaScript.
* `load_image`: For optimizing images.

## Output

To get data *out* of the graph and back onto the FileSystem (e.g., writing HTML
files), your tasks return `Output` structs.

```rust
use hauchiwa::{Output, output::OutputData};

// Inside a task closure
let html = "<html>...</html>";

Ok(vec![Output {
    path: "index.html".into(),
    data: OutputData::Utf8(html.to_string()),
}])
```
