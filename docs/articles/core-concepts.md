---
title: Core concepts
order: 4
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
* `load_esbuild`: For TypeScript/JavaScript bundling with an external binary.
* `load_rolldown`: For native Rust bundling (requires `rolldown`).
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

        // Iterate all items (coarse dependency - reruns if anything changes)
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

## Documents and routes

`load_documents::<Frontmatter>()` parses YAML frontmatter into `doc.matter` and
keeps the unrendered body in `doc.text`. Choose a Markdown renderer in your own
task, as shown in [Getting started](getting-started.html).

Set `.base("content")` on the loader to strip that directory from public routes.
The source path and Tracker key remain unchanged:

| Source | `doc.meta.href` with `.base("content")` | Output file |
| :--- | :--- | :--- |
| `content/index.md` | `/` | `index.html` |
| `content/about.md` | `/about/` | `about/index.html` |
| `content/posts/hello/index.md` | `/posts/hello/` | `posts/hello/index.html` |

`doc.meta.assets("*.png")` selects assets in the document's bundle directory.
`doc.meta.resolve("../other.md")` resolves a path relative to that bundle.

## Output

Return `Output` or `Vec<Output>` from a task to write files into the configured
output directory. Use the target builder to distinguish routes from exact files:

```rust
use hauchiwa::Output;

let about = Output::to("/about/").html("<h1>About</h1>")?;
let feed = Output::file("feed.xml").text("<feed></feed>")?;
let icon = Output::file("favicon.ico").bytes(bytes)?;
// Inside a document-rendering task:
let page = Output::to(doc).html(rendered_html)?;
```

`Output::to` infers a page route for an empty path, a path starting or ending
with `/`, or a path without an extension. Other paths are exact file targets.
Use `Output::page("/about/")` or `Output::file("about.html")` to be explicit.
For example, `Output::to("about.html")` writes `about.html`, while
`Output::to("/about.html")` writes `about.html/index.html`.

The `.html()`, `.text()`, and `.bytes()` methods return a `Result`, validating
that the final path stays inside the output directory. They select the stored
content representation; `Output` does not carry HTTP headers.

The older `Output::html(path, html)`, `Output::binary(path, bytes)`, and
`Output::mapper(source)` helpers remain available. `Output::html` transforms a
source-style path into a pretty HTML path; it does not strip a content base.
For loaded documents, prefer `Output::to(doc)` to use `doc.meta.href`.
