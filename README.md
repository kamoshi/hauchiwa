# Hauchiwa

A small, fast SSG library.

The main goal of this library is to serve as the backbone of your custom
generator by doing the mundane things:
- gathering content files
- optimizing images
- compiling SCSS styles
- compiling JavaScript scripts

The entire library is centered around a single trait [Content], which is then
internally used by the library for processing content files and outputting
HTML files.

Here's a small sample of how you can use this library to create your own generator:

```rust
use clap::{Parser, ValueEnum};
use hauchiwa::Website;
use hypertext::Renderable;

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

fn main() {
	let args = Args::parse();

	let website = Website::design()
		.content::<crate::html::Post>("content/posts/**/*", ["md", "mdx"].into())
		.content::<crate::html::Wiki>("content/wiki/**/*", ["md"].into())
		.js("search", "./js/search/dist/search.js")
		.add_virtual(
			|sack| crate::html::search(sack).render().to_owned().into(),
			"search/index.html".into(),
		)
		.add_virtual(
			|sack| crate::html::to_list(sack, sack.get_links("posts/**/*.html"), "Posts".into()),
			"posts/index.html".into(),
		)
		.finish();

	match args.mode {
		Mode::Build => website.build(),
		Mode::Watch => website.watch(),
	}
}
```

Note that you have to implement your own logic for parsing Markdown and for
rendering HTML pages by implementing the [Content] trait for your own front
matter struct type.

In this example the trait is implemented for `crate::html::Post` and
`crate::html::Wiki`, each being a different style of a page, with different
front matter.
