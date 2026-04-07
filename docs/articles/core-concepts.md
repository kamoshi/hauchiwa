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

You use `.using()` to connect tasks. This is where the magic happens. The
type system ensures that the data produced by the upstream task matches what the
downstream task expects.

```rust
// 'pages' is a Many<Document> handle
let pages = config.load_documents::<Frontmatter>().glob("*.md")?.register();

config.task()
    .using(pages) // We pass the handle here
    .merge(|ctx, pages| {
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
* `load_esbuild`: For TypeScript/JavaScript bundling.
* `load_images`: For optimizing images.
* `load_minijinja`: For Jinja2-style templates (requires `minijinja` feature).

## Accessing items from a Tracker

When a `Many<T>` dependency is resolved, you receive a `Tracker<T>`. It provides
several ways to access the items inside:

```rust
config.task()
    .using(pages)
    .merge(|ctx, pages| {
        // Look up a single item by path (fine-grained dependency)
        let post = pages.get("content/hello.md")?;

        // Iterate only items matching a glob (fine-grained dependency)
        for (path, post) in pages.glob("content/blog/**/*.md")? {
            println!("{path}: {}", post.matter.title);
        }

        // Iterate all items (coarse dependency — reruns if anything changes)
        for (path, post) in pages.iter() { /* ... */ }

        // Values-only shorthand (same coarse dependency as iter)
        let titles: Vec<_> = pages.values().map(|p| &p.matter.title).collect();

        Ok(())
    });
```

`Tracker` implements `IntoIterator` yielding `(&str, &T)` pairs, so you can use
it directly in a `for` loop:

```rust
for (path, post) in pages { /* ... */ }
```

## Output

To get data *out* of the graph and back onto the FileSystem (e.g., writing HTML
files), your tasks return `Output` values.

```rust
use hauchiwa::Output;

// Inside a task closure

// Convenience constructors
Ok(Output::html("about/index.html", "<h1>About</h1>"))
Ok(Output::binary("feed.xml", bytes))
```

`Output::html` sets the correct `Content-Type` and handles clean URL paths.
Tasks returning `Vec<Output>` can emit multiple files in a single run.
