# Hauchiwa

Incredibly flexible static site generator library with incremental rebuilds and
cached image optimization. This library can be used as the backbone of your own
static site generator, it can handle all the mundane work:

- gathering content files from the file system
- optimizing images and caching the work
- compiling SCSS and CSS stylesheets
- compiling JavaScript applications via ESBuild
- watching for changes and incremental rebuilds

## Feature flags

- `server` - add a HTTP server for the generated website's files in watch mode

## Get started

To get started add the following snippet to your `Cargo.toml` file.

```toml
[dependencies.hauchiwa]
version = "*" # change this version to the latest
features = ["server"]
```

## Declarative configuration

The configuration API is designed to be extremely minimal while providing the
maximum of value to the user by being flexible and unopinionated. It's supposed
to be really delightful and intuitive to use.

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
    let website = Website::setup()
        // We can configure the collections of files used to build the pages.
        .add_collections([
            Collection::glob_with("content", "posts/**/*", ["md"], process_matter_yaml::<Post>),
        ])
        // We can configure the generator to process additional files like images or custom assets.
        .add_processors([
            Processor::process_images(["jpg", "png", "gif"]),
            Processor::process_assets(["bib"], process_bibliography),
        ])
        // We can add directories containing global stylesheets, either CSS or SCSS.
        .add_global_styles(["styles"])
        // We can add entrypoints to scripts and their aliases.
        .add_scripts([
            ("search", "./js/search/dist/search.js"),
            ("photos", "./js/vanilla/photos.js"),
        ])
        // We can add a simple task to generate the `index.html` page with arbitrary
        // content, here it's `<h1>hello world!</h1>`.
        .add_task(|_| {
            vec![("index.html".into(), String::from("<h1>hello world!</h1>"))]
        })
        // We can retrieve any loaded content from the `sack` provided to the task.
        // Note that you have to bring your own markdown parser and HTML templating
        // engine here.
        .add_task(|sack| {
            sack.query_content::<Post>("posts/**/*")
                .into_iter()
                .map(|query| {
                    // Retrieve any assets required to build the page, they are automatically
                    // tracked when in watch mode, and cause a rebuild when modified.
                    let library = sack.get_library(query.area);
                    // Parse the content of a Markdown file, bring your own library.
                    let (parsed, outline, bib) = html::post::parse_content(query.content, &sack, query.area, library);
                    // Generate the HTML page, bring your own library.
                    let out_buff = html::post::as_html(query.meta, &parsed, &sack, outline, bib);
                    // Return the slug and content as a tuple.
                    (query.slug.join("index.html"), out_buff)
                })
                .collect()
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

This library is available under GPL 3.0.
