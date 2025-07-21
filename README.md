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
use serde::{self, Deserialize};
use hauchiwa::{Website, Page, Hook};
use hauchiwa::loader::{
    self, glob_content, glob_images, glob_assets, glob_scripts, glob_styles, glob_svelte,
    yaml, Content
};

const BASE: &str = "content";

type Props = ();
type Post = ();

struct Bibtex {
    path: camino::Utf8PathBuf,
    data: String,
};

#[derive(Default)]
struct MyData {};

// Here we start by calling the `setup` function.
let mut website = Website::config()
    .add_loaders([
        // We can configure the collections of files used to build the pages.
        loader::glob_content(BASE, "posts/**/*.md", yaml::<Post>),
        // We can configure the generator to process additional files like images or custom assets.
        loader::glob_images(BASE, "**/*.jpg"),
        loader::glob_images(BASE, "**/*.png"),
        loader::glob_images(BASE, "**/*.gif"),
        loader::glob_assets(BASE, "**/*.bib", |rt, data| {
            // save the raw data in cache and return path
            let path = rt.store(&data, "bib").unwrap();
            let text = String::from_utf8_lossy(&data);
            let data = todo!(); // TODO: load bibtex via `hayagriva`

            // return data (path to file + parsed bibtex)
            Ok(Bibtex { path, data })
        }),
        // We can add directories containing global stylesheets, either CSS or SCSS.
        loader::glob_styles("styles", "**/[!_]*.scss"),
        // We can add JavaScript scripts compiled via ESBuild
        loader::glob_scripts("scripts", "src/*/main.ts"),
        // We can add Svelte component compiled via ESbuild. We can use type
        // parameter to specify the shape of props passed to the component,
        // or we can use `()` if we don't need anything.
        loader::glob_svelte::<Props>("scripts", "src/*/App.svelte"),
    ])
    // We can add a simple task to generate the `index.html` page with arbitrary
    // content, here it's `<h1>hello world!</h1>`.
    .add_task("index page", |_| {
        let pages = vec![Page::text("index.html".into(), String::from("<h1>hello world!</h1>"))];

        Ok(pages)
    })
    // We can retrieve any loaded content from the `ctx` provided to the task.
    // Note that you have to bring your own markdown parser and HTML templating
    // engine here.
    .add_task("posts", |ctx| {
        let mut pages = vec![];

        for item in ctx.glob_with_file::<Content<Post>>("posts/**/*")? {
            // Retrieve any assets required to build the page.
            let pattern = format!("{}/*", item.file.area);
            let library = ctx.get::<Bibtex>(&pattern)?;
            // Parse the content of a Markdown file, bring your own library.
            let (parsed, outline, bibliography): (String, (), ()) =
                todo!("whatever you want to use, e.g pulldown_cmark");
            // Generate the HTML page, bring your own library.
            let rendered = todo!("whatever you want to use, e.g maud");
            // Return the path and content as a tuple.
            pages.push(Page::text(item.file.slug.join("index.html"), rendered))
        }

        Ok(pages)
    })
    // Do something after build
    .add_hook(Hook::post_build(|pages| {
        Ok(())
    }))
    // Complete the configuration process.
    .finish();


// Start the library in either the *build* or the *watch* mode.
website.build(MyData::default());
// website.watch(MyData::default());
```

The full documentation for this library is always available on
[docs.rs](https://docs.rs/hauchiwa/latest/hauchiwa/). Please feel free to take a
look! 😊

## Examples

- [kamoshi.org](https://git.kamoshi.org/kamov/kamoshi.org)

## License

This library is available under the GPL 2.0 (or later) license.
