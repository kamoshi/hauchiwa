use hauchiwa::{Blueprint, Output, task};
use serde::Deserialize;

// -----------------------------------------------------------------------------
// Hugo-like workflow example
//
// This example demonstrates how to build a convention-based static site generator.
// It mimics the structure of tools like Hugo:
//
// - content/   -> Markdown posts
// - layouts/   -> HTML Templates
// - static/    -> Copied as-is
//
// -----------------------------------------------------------------------------

// HYPOTHETICAL ASKAMA SETUP:
//
// 1. Add `askama = "0.15.1"` to Cargo.toml.
// 2. Define your templates as structs:
//
//    #[derive(askama::Template)]
//    #[template(path = "single.html")] // Located in a `templates` folder
//    struct SinglePostTemplate<'a> {
//        site_title: &'a str,
//        title: &'a str,
//        content: &'a str,
//        css_url: &'a str,
//    }
//
// 3. Inside the task, render it:
//
//    let html = SinglePostTemplate {
//        site_title: "My Site",
//        title: &doc.metadata.title,
//        content: &doc.body,
//        css_url: "style.css",
//    }.render().unwrap();
// =============================================================================

#[derive(Clone, Deserialize)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    draft: bool,
}

fn main() -> anyhow::Result<()> {
    // In a real Hugo clone, you might read a config.toml here.
    const SITE_NAME: &str = "My Hugo-ish Site";

    let mut config = Blueprint::<()>::new();

    // -------------------------------------------------------------------------
    // 1. Content
    // -------------------------------------------------------------------------
    // We scan for all markdown files. In hauchiwa, this returns a Handle
    // to a `HashMap<PathBuf, Document<Frontmatter>>`.
    let content = config.load_documents::<Frontmatter>("examples/assets/content/*.md")?;

    // -------------------------------------------------------------------------
    // 2. Styles
    // -------------------------------------------------------------------------
    // We process SCSS files. This mimics `themes/mytheme/assets`.
    let css = config
        .load_css()
        .entry("examples/assets/styles/main.scss")
        .watch("examples/assets/styles/**/*.scss")
        .register()?;

    // -------------------------------------------------------------------------
    // 3. Build task
    // -------------------------------------------------------------------------
    // This task acts as the controller. It fetches the data, selects the
    // correct layout, and generates the final output.
    task!(config, |_ctx, content, css| {
        let mut outputs = Vec::new();

        // Resolve the hashed filename of the CSS (e.g., "main.a1b2c3.css")
        let css_link = css
            .values()
            .next()
            .map(|s| s.path.as_str())
            .unwrap_or("style.css");

        for doc in content.values() {
            // Skip drafts in production builds
            if doc.metadata.draft {
                continue;
            }

            // A. Determine Output Path
            //    Hugo Style: content/post.md -> public/post/index.html (Pretty URLs)
            let stem = doc.path.file_stem().unwrap_or_default();
            let out_path = format!("{}/index.html", stem);

            // B. Render the "Single" Layout
            //    If using Askama, you would initialize `SinglePostTemplate` here.
            let html = format!(
                r#"
                <!DOCTYPE html>
                <html lang="en">
                <head>
                    <meta charset="UTF-8">
                    <title>{title} | {site}</title>
                    <link rel="stylesheet" href="{css}">
                </head>
                <body>
                    <header>
                        <h1><a href="/">{site}</a></h1>
                    </header>
                    <main>
                        <article>
                            <header>
                                <h2>{title}</h2>
                                <time>{date}</time>
                            </header>
                            <div class="content">
                                {body}
                            </div>
                        </article>
                    </main>
                    <footer>
                        <p>&copy; 2024 {site}</p>
                    </footer>
                </body>
                </html>
                "#,
                title = doc.metadata.title,
                site = SITE_NAME,
                css = css_link,
                date = doc.metadata.date,
                // Note: `doc.body` is the raw Markdown content.
                // In a real app, you would pass this through `pulldown-cmark` here
                // to convert Markdown -> HTML before embedding it.
                body = doc.body
            );

            outputs.push(Output {
                url: out_path.into(),
                content: html,
            });
        }

        // ---------------------------------------------------------------------
        // 4. The homepage
        // ---------------------------------------------------------------------
        //    If using Askama, this would be a `ListTemplate` struct taking
        //    `posts: Vec<&Frontmatter>` as a field.

        let mut list_items = String::new();
        for doc in content.values() {
            if !doc.metadata.draft {
                let stem = doc.path.file_stem().unwrap_or_default();
                list_items.push_str(&format!(
                    r#"<li><span class="date">[{date}]</span> <a href="/{slug}/">{title}</a></li>"#,
                    date = doc.metadata.date,
                    slug = stem,
                    title = doc.metadata.title
                ));
            }
        }

        let home_html = format!(
            r#"
            <!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <title>{site}</title>
                <link rel="stylesheet" href="{css}">
            </head>
            <body>
                <header><h1>{site}</h1></header>
                <main>
                    <h2>Recent Posts</h2>
                    <ul>
                        {list}
                    </ul>
                </main>
            </body>
            </html>
            "#,
            site = SITE_NAME,
            css = css_link,
            list = list_items
        );

        outputs.push(Output {
            url: "index.html".into(),
            content: home_html,
        });

        Ok(outputs)
    });

    config.finish().build(())?;
    println!("Hugo-ish build complete. 'Hinagata' applied successfully.");

    Ok(())
}
