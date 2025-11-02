//! A simple static site generator that loads markdown files, converts them to HTML,
//! and generates an index page.

use hauchiwa::{
    camino::Utf8PathBuf,
    executor,
    loader::{self, Registry},
    page::Page,
    {Site, SiteConfig},
};

#[derive(Debug, Clone)]
struct Post {
    title: String,
    content: String,
    url: Utf8PathBuf,
}

fn main() {
    let mut config = SiteConfig::new();

    let posts_handle =
        loader::glob_assets(&mut config, "examples/posts/**/*.md", |_, file| {
            let content = String::from_utf8(file.metadata)?;
            let title = content
                .lines()
                .next()
                .unwrap_or("Untitled")
                .trim_start_matches('#')
                .trim()
                .to_string();

            let url = format!("/posts/{}.html", file.path.file_stem().unwrap());

            Ok(Post {
                title,
                content,
                url: url.into(),
            })
        });

    config.add_task((posts_handle,), |_, (posts,): (&Registry<Post>,)| {
        posts
            .values()
            .map(|post| Page {
                url: post.url.clone(),
                content: format!(
                    "<h1>{}</h1><pre><code>{}</code></pre>",
                    post.title, post.content
                ),
            })
            .collect::<Vec<_>>()
    });

    config.add_task((posts_handle,), |_, (posts,): (&Registry<Post>,)| {
        let post_links = posts
            .values()
            .map(|post| {
                format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    post.url, post.title
                )
            })
            .collect::<String>();

        Page {
            url: "/index.html".into(),
            content: format!("<h1>Posts</h1><ul>{}</ul>", post_links),
        }
    });

    let mut site = Site::new(config);
    let globals = hauchiwa::Globals {
        generator: "hauchiwa",
        mode: hauchiwa::Mode::Build,
        port: None,
        data: (),
    };
    let (_, pages) = executor::run_once(&mut site, &globals);

    for page in pages {
        println!("Page: {} ({} bytes)", page.url, page.content.len());
    }
}
