//! A simple static site generator that loads markdown files, converts them to HTML,
//! and generates an index page.

use hauchiwa::{
    camino::Utf8PathBuf,
    executor,
    loader::File,
    page::Page,
    {Site, SiteConfig},
};

#[derive(Debug, Clone)]
struct RawPost {
    path: Utf8PathBuf,
    content: String,
}

#[derive(Debug, Clone)]
struct Post {
    title: String,
    content: String,
    url: String,
}

fn main() {
    let mut config = SiteConfig::new();

    let raw_posts_handle = config.add_task_opaque(FileLoaderTask::new(
        "examples/posts",
        "**/*.md",
        |_, file: File<Vec<u8>>| {
            let content = String::from_utf8(file.metadata)?;
            Ok(RawPost {
                path: file.path,
                content,
            })
        },
    ));

    let posts_handle = config.add_task((raw_posts_handle,), |_, (raw_posts,): (&Vec<RawPost>,)| {
        raw_posts
            .iter()
            .map(|raw_post| {
                let title = raw_post
                    .content
                    .lines()
                    .next()
                    .unwrap_or("Untitled")
                    .trim_start_matches('#')
                    .trim()
                    .to_string();

                let url = format!("/posts/{}.html", raw_post.path.file_stem().unwrap());

                Post {
                    title,
                    content: raw_post.content.clone(),
                    url,
                }
            })
            .collect::<Vec<_>>()
    });

    config.add_task((posts_handle,), |_, (posts,): (&Vec<Post>,)| {
        posts
            .iter()
            .map(|post| Page {
                url: post.url.clone(),
                content: format!(
                    "<h1>{}</h1><pre><code>{}</code></pre>",
                    post.title, post.content
                ),
            })
            .collect::<Vec<_>>()
    });

    config.add_task((posts_handle,), |_, (posts,): (&Vec<Post>,)| {
        let post_links = posts
            .iter()
            .map(|post| format!("<li><a href=\"{}\">{}</a></li>", post.url, post.title))
            .collect::<String>();

        Page {
            url: "/index.html".to_string(),
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
