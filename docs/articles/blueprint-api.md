---
title: Blueprint API
order: 4
---

# Blueprint API

The `Blueprint` is the heart of your site configuration. It provides a fluent API
to define tasks and the relationships between them.

## Task Creation

Every task starts with `config.task()`:

```rust
config.task()
    .name("My Task") // Optional: Name for debugging/visualization
    // ... chain methods ...
```

## Handles and Dependencies

Hauchiwa uses a strongly-typed handle system to manage dependencies.

* **`One<T>`**: A single value (e.g., a configuration object, a sitemap).
  * Resolved as: `&T`
* **`Many<T>`**: A collection of values (e.g., pages, images).
  * Resolved as: `Tracker<T>`

You inject dependencies using `.using()`:

```rust
let pages: Many<Document> = ...;
let config_obj: One<Config> = ...;

config.task()
    .using((pages, config_obj)) // Pass a tuple for multiple dependencies
    // ...
```

## Task Operations

The final method in the chain determines how the task executes and what kind of
output it produces.

### 1. `run()`

Use `.run()` for tasks with **zero dependencies** that produce a single output
(`One<T>`).

```rust
config.task().run(|ctx| {
    Ok("Hello World")
});
```

### 2. `merge()` (Gather)

Use `.merge()` to **gather** dependencies. This is the most common operation. It
takes any number of dependencies (One or Many) and runs once, providing access
to all of them.

* **Input**: Defined by `.using()`.
* **Output**: Returns `One<R>`.

```rust
// Example: Generate a sitemap from all pages
config.task()
    .using(pages)
    .merge(|ctx, pages| {
        // 'pages' is a Tracker<Document>
        let urls: Vec<String> = pages.values().map(|p| p.meta.href.clone()).collect();
        Ok(urls)
    });
```

### 3. `spread()` (Scatter)

Use `.spread()` to take a single input and **scatter** it into multiple outputs
(`Many<T>`).

* **Input**: Defined by `.using()`.
* **Output**: Returns `Many<R>`.

```rust
// Example: Create a task that generates multiple variants from one config
config.task()
    .using(global_config)
    .spread(|ctx, config| {
        Ok(vec![
            ("light".to_string(), Theme::Light),
            ("dark".to_string(), Theme::Dark),
        ])
    });
```

### 4. `each().map()` (Map)

Use `.each()` combined with `.map()` to process a `Many` handle item-by-item.
This is efficient because it enables **fine-grained invalidation**. If only one
item in the source changes, only that specific item is re-processed.

* **Input**: A primary `Many<T>` handle passed to `.each()`.
  * **Extras**: Optional extra dependencies via `.using()`.
* **Output**: Returns `Many<R>`.

```rust
// Example: Render HTML for each Markdown page
config.task()
    .each(pages) // Primary dependency (Many<Document>)
    .using(template) // Secondary dependency (One<Template>)
    .map(|ctx, doc, template| {
        // 'doc' is &Document (single item)
        // 'template' is &Template (resolved dependency)
        let html = template.render(doc);
        Ok(html)
    });
```

### 5. `glob().map()`

A shortcut to load files directly without a separate loader.

```rust
config.task()
    .glob("src/assets/*.png")
    .map(|ctx, store, input| {
        // Process file...
        Ok(processed_image)
    });
```
