use std::collections::HashMap;

use hauchiwa::{Blueprint, Output, output::OutputData, task};
use serde::Deserialize;

/// This example demonstrates the "Diamond Dependency" pattern.
///
/// Use Case:
/// You have raw data (Markdown posts) and you need to calculate aggregate data
/// (Taxonomy/Tags) derived from them. Multiple output tasks (Pages, Sitemap)
/// need BOTH the raw posts and the aggregate statistics.
///
/// Topology:
///       [Load Documents]
///          /       \
///    [Taxonomy]     \
///     /      \       \
/// [Post Pages]   [Sitemap]
///
/// Prerequisite File Structure:
/// ├── examples/
/// │   └── assets/
/// │       └── content/ (put .md files with `tags: ["a", "b"]` in frontmatter)

#[derive(Clone, Deserialize)]
struct Post {
    title: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// A custom struct to hold our aggregated taxonomy data.
/// This acts as an "Intermediate Representation" in our build graph.
#[derive(Clone)]
struct Taxonomy {
    /// Maps a Tag Name -> List of Post Titles
    tags: HashMap<String, Vec<String>>,
}

fn main() -> anyhow::Result<()> {
    let mut config = Blueprint::<()>::new();

    // -----------------------------------------------------------------------
    // 1. Load sources
    // -----------------------------------------------------------------------
    // We start by loading the raw content. This handle `posts` will be
    // injected into multiple downstream tasks.
    let posts = config
        .load_documents::<Post>()
        .source("examples/assets/content/*.md")
        .register()?;

    // -----------------------------------------------------------------------
    // 2. Create taxonomy
    // -----------------------------------------------------------------------
    // This task acts as a bridge. It consumes `posts` to calculate statistics.
    // Crucially, it returns `Taxonomy`, NOT `Output`. This means it does not
    // write a file to disk; it only passes data in memory to the next tasks.
    let taxonomy = task!(config, |_, posts| {
        let mut tags = HashMap::new();

        for post in posts.values() {
            for tag in &post.matter.tags {
                tags.entry(tag.clone())
                    .or_insert_with(Vec::new)
                    .push(post.matter.title.clone());
            }
        }

        // Return the struct directly. Hauchiwa wraps this in a Handle.
        Ok(Taxonomy { tags })
    });

    // -----------------------------------------------------------------------
    // 3. Output Task A: individual post pages
    // -----------------------------------------------------------------------
    // This task depends on:
    // 1. `posts` (to get the content/title of the page being built)
    // 2. `taxonomy` (to look up how popular the tags on this page are)
    task!(config, |_, posts, taxonomy| {
        let mut pages = Vec::new();

        for post in posts.values() {
            // Logic: For every tag on this post, look up the total count in the taxonomy.
            let tag_counts = post
                .matter
                .tags
                .iter()
                .map(|t| {
                    // Safe lookup into the calculated taxonomy
                    let count = taxonomy.tags.get(t).map(|v| v.len()).unwrap_or(0);
                    format!("{} ({})", t, count)
                })
                .collect::<Vec<_>>()
                .join(", ");

            let content = format!("<h1>{}</h1><p>Tags: {}</p>", post.matter.title, tag_counts);

            // Create a "Pretty URL" (e.g., page1.md -> dist/page1/index.html)
            let stem = post.meta.path.file_stem().unwrap_or("unknown");

            pages.push(Output {
                path: format!("{}/index.html", stem).into(),
                data: OutputData::Utf8(content),
            });
        }

        Ok(pages)
    });

    // -----------------------------------------------------------------------
    // 4. Output Task B: sitemap
    // -----------------------------------------------------------------------
    // This task acts in parallel to the Post Pages task. It also consumes
    // the SAME `taxonomy` calculated in step 2. The build engine ensures
    // the taxonomy is only calculated once.
    task!(config, |_ctx, posts, taxonomy| {
        let mut xml =
            String::from(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);

        // List all posts
        for post in posts.values() {
            let stem = post.meta.path.file_stem().unwrap_or("unknown");
            xml.push_str(&format!("<url><loc>/{}</loc></url>", stem));
        }

        // List all virtual tag pages (derived from taxonomy)
        for tag in taxonomy.tags.keys() {
            xml.push_str(&format!("<url><loc>/tags/{}</loc></url>", tag));
        }

        xml.push_str("</urlset>");

        Ok(Output {
            path: "sitemap.xml".into(),
            data: OutputData::Binary(xml.into()),
        })
    });

    // -----------------------------------------------------------------------
    // 5. Execute
    // -----------------------------------------------------------------------
    // The engine resolves the DAG (Directed Acyclic Graph).
    // It sees that `Taxonomy` must run before `Post Pages` and `Sitemap`,
    // and `Load Documents` must run before `Taxonomy`.
    config.finish().build(())?;

    Ok(())
}
