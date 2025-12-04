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
hauchiwa = "0.8.0" # change this version to the latest
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

Within your application's main function, you can configure precisely how the
website should be generated, defining content loaders, tasks, and hooks.

```rust
use serde::Deserialize;
use hauchiwa::{SiteConfig, Site, Globals};
use hauchiwa::page::Page;
use hauchiwa::loader::{self, Runtime};

struct Bibtex {
    path: camino::Utf8PathBuf,
    data: String,
}

#[derive(Default)]
struct MyData {};

fn main() -> anyhow::Result<()> {
    // Here we start by creating a new configuration.
    let mut config = SiteConfig::new();

    const BASE: &str = "content";

    // We can configure the collections of files used to build the pages.
    // glob_content expects YAML frontmatter matching the Post struct.
    let posts = loader::glob_content::<MyData, Post>(&mut config, "content/posts/**/*.md")?;

    // We can configure the generator to process additional files like images or custom assets.
    let images = loader::glob_images(&mut config, &["content/**/*.jpg", "content/**/*.png"])?;

    // We can add directories containing global stylesheets, either CSS or SCSS.
    let styles = loader::build_styles(&mut config, "styles/main.scss", "styles/**/*.scss")?;

    // We can add JavaScript scripts compiled via ESBuild
    let scripts = loader::build_scripts(&mut config, "scripts/main.ts", "scripts/**/*.ts")?;

    // We can add custom assets processing using glob_assets
    let bibtex = loader::glob_assets(&mut config, "content/**/*.bib", |globals, file| {
         let rt = Runtime;
         // save the raw data in cache and return path
         let path = rt.store(&file.metadata, "bib")?;
         let text = String::from_utf8_lossy(&file.metadata);
         let data = todo!(); // TODO: load bibtex via `hayagriva`

         // return data (path to file + parsed bibtex)
         Ok(Bibtex { path, data: data })
    })?;

    // We can add a simple task to generate the `index.html` page.
    // The task! macro makes it easy to declare dependencies.
    hauchiwa::task!(config, |ctx, posts, images, styles, scripts| {
        let mut pages = vec![];

        // posts is Registry<Content<Post>>
        for post in posts.values() {
            // Retrieve any assets required to build the page.
            // ...

            // Parse the content of a Markdown file, bring your own library.
            let (parsed, outline, bibliography): (String, (), ()) =
                todo!("whatever you want to use, e.g pulldown_cmark");

            // Generate the HTML page, bring your own library.
            let rendered = todo!("whatever you want to use, e.g maud");

            // Add the page to the list
            pages.push(Page::text(post.path.with_extension("html"), rendered));
        }

        Ok(pages)
    });

    // Create the site from the configuration
    let mut site = Site::new(config);

    // Start the library in either the *build* or the *watch* mode.
    site.build(MyData::default())?;
    // site.watch(MyData::default())?;

    Ok(())
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
