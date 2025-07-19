# Hauchiwa

Incredibly flexible static site generator library with incremental rebuilds and
cached image optimization. This library can be used as the backbone of your own
static site generator, it can handle all the mundane work for you:

- gathering content files from the file system
- optimizing images and caching the work
- compiling SCSS and CSS stylesheets
- compiling JavaScript applications via ESBuild
- watching for changes and incremental rebuilds

This library's API is purposefully designed to be small, simple, flexible and powerful.


## Feature flags

```toml
[dependencies.hauchiwa]
features = [
    "asyncrt",  # add async runtime (tokio) and async loader
    "styles",   # add sass loader
    "images",   # add image loader + optimizer
    "reload",   # add live reload in watch mode
    "server",   # add http server for local development and writing in watch mode
]
```

## Get started

To get started add the following snippet to your `Cargo.toml` file.

```toml
hauchiwa = "*" # change this version to the latest
```

## Declarative configuration

The configuration API is designed to be extremely minimal, but powerful and flexible.

Here's a small sample of how you can use this library to create your own
generator. Let's start by defining the shape of front matter for a single post
stored as a Markdown file.

```rust
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

```rust
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

In the `main` function of your application you can configure how the website should be generated.

```rust
fn main() {
    let args = Args::parse();

    // Here we start by calling the `setup` function.
    let website = Website::config()
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
                let data = hayagriva::io::from_biblatex_str(&text).unwrap();

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
            let pages = ctx.glob_with_file::<Content<Post>>("posts/**/*")
                .into_iter()
                .map(|item| {
                    // Retrieve any assets required to build the page.
                    let pattern = format!("{}/*", item.file.area);
                    let library = ctx.get::<Bibtex>(&pattern)?;
                    // Parse the content of a Markdown file, bring your own library.
                    let (parsed, outline, bibliography) = crate::md::parse(&ctx, item.data.text, library);
                    // Generate the HTML page, bring your own library.
                    let rendered = crate::html::render(&ctx, parsed, outline, bibliography);
                    // Return the path and content as a tuple.
                    (item.file.slug.join("index.html"), rendered)
                })
                .collect()?;

            Ok(pages)
        })
        // Complete the configuration process.
        .finish();

    // Start the library in either the *build* or the *watch* mode.
    match args.mode {
        Mode::Build => website.build(MyData::new()),
        Mode::Watch => website.watch(MyData::new()),
    }
}
```

The full documentation for this library is always available on
[docs.rs](https://docs.rs/hauchiwa/latest/hauchiwa/), please feel free to take a
look at it 😊

## License

This library is available under GPL 2.0 (or later).
