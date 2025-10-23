# Hauchiwa

An incredibly flexible static site generator library featuring incremental
rebuilds and cached image optimization. This library is designed to be the
robust backbone for your custom static site generator, handling all the common
tasks:

- **Loading content**: Efficiently collects content files from your file system.
- **Image optimization**: Optimizes images and intelligently caches the results
  to speed up subsequent builds.
- **Stylesheet compilation**: Compiles SCSS and CSS stylesheets.
- **JavaScript compilation**: Processes JavaScript applications using ESBuild.
- **Watch mode**: Monitors for changes and performs fast, incremental rebuilds.
- and much more...

## Why This Library?

I created this library out of dissatisfaction with existing static site
generators. Many felt either too **rigid** (like Jekyll, Hugo, and Zola),
**arcane** (like Hakyll), or simply **bloated** JavaScript frameworks (like
Gatsby and Astro).

In contrast, this library's API is purposefully **small**, **simple**,
**flexible**, and **powerful**. If you're looking to generate a static blog, you
likely won't need any other generator. Its true strength lies in its
extensibility, as you can leverage the entire Rust ecosystem to customize it in
countless ways. Also, the codebase is compact enough to be easily forked and
maintained by a single person, a feature that might be particularly appealing to
hackers like yourself!

## Feature flags

You can selectively enable features by specifying them in your Cargo.toml file.
This allows you to include only the functionalities your project needs, keeping
your dependencies lean.

```toml
[dependencies.hauchiwa]
features = [
    "asyncrt",  # Adds an asynchronous runtime (Tokio) and an async loader.
    "styles",   # Includes the Sass loader for CSS pre-processing.
    "images",   # Enables image loading and optimization capabilities.
    "reload",   # Activates live reloading in watch mode for a smoother development experience.
    "server",   # Provides an HTTP server for local development and writing in watch mode.
]
```

## Get started

To begin using Hauchiwa, add the following snippet to your Cargo.toml file.
Remember to replace "*" with the latest version available on Crates.io.

```toml
hauchiwa = "*" # change this version to the latest
```

## Declarative configuration

The configuration API is designed to be extremely minimal, yet powerful and
flexible, allowing you to define your website's structure and behavior with
clarity.

Here's a small sample demonstrating how you can use this library to create your
own static site generator. Let's start by defining the shape of the front matter
for a single post, typically stored as a Markdown file.

```rust ignore
/// Represents a simple post, this is the metadata for your Markdown content.
#[derive(Deserialize, Debug, Clone)]
pub struct Post {
    pub title: String,
    #[serde(with = "isodate")]
    pub date: DateTime<Utc>,
}
```

The `main.rs` of your application can use `clap` to accept any additional CLI
arguments, such as mode.

```rust ignore
use clap::{Parser, ValueEnum};

#[derive(Parser, Debug, Clone)]
struct Args {
    #[clap(value_enum, index = 1, default_value = "build")]
    mode: Mode,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Mode {
    Build,
    Watch,
}
```

Within your application's main function, you can configure precisely how the
website should be generated, defining content loaders, tasks, and hooks.

```rust
use hauchiwa::{
    core_structs::WebsiteBuilder,
    core_structs::{Website,Sack,DependencyGraph},
};
use std::collections::HashMap;
use std::path::PathBuf;

type PostFrontMatter = ();

fn main() {
    let mut builder = WebsiteBuilder::new();

    let posts = builder.add_collection("posts/**/*.md", |path: PathBuf, bytes: Vec<u8>| {
        (path, String::from_utf8_lossy(&bytes).to_string())
    });

    builder.add_task((posts,), move |(posts,): (Vec<(PathBuf, String)>,)| {
        let mut content = String::new();
        for (path, body) in posts {
            content.push_str(&format!("<h1>{:?}</h1><p>{}</p>", path, body));
        }
        (vec![(PathBuf::from("index.html"), content)], "index.html".to_string())
    });

    let website = builder.finish().unwrap();
    website.build().unwrap();
}
```

The full documentation for this library is always available on
[docs.rs](https://docs.rs/hauchiwa/latest/hauchiwa/). The loader submodule
contains all the available loader implementations along with documentation.
Please feel free to take a look! 😊

## Examples

- [kamoshi.org](https://git.kamoshi.org/kamov/kamoshi.org)

## License

This library is available under the GPL 2.0 (or later) license.
