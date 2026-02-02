mod highlight;

use clap::{Parser, ValueEnum};
use comrak::{Options, markdown_to_html_with_plugins, options::Plugins};
use hauchiwa::{Blueprint, Output, output::OutputData};
use hypertext::{prelude::*, rsx};
use serde::Deserialize;

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Mode {
    Build,
    Watch,
}

#[derive(Parser, Debug, Clone)]
struct Args {
    #[clap(value_enum, index = 1, default_value = "build")]
    mode: Mode,
}

#[derive(Clone, Deserialize, Debug)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    order: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut config = Blueprint::<()>::new();

    let css = config
        .load_css()
        .entry("assets/style.scss")
        .watch("assets/*.scss")
        .register()?;

    let articles = config
        .load_documents::<Frontmatter>()
        .source("articles/*.md")
        .register()?;

    config
        .task()
        .depends_on((css, articles))
        .run(|_, (css, articles)| {
            let mut outputs = Vec::new();

            let css_href = css.get("assets/style.scss")?;
            let css_href = css_href.path.as_str();

            let mut sorted_articles: Vec<_> = articles.iter().map(|(_, doc)| doc).collect();
            sorted_articles.sort_by_key(|doc| doc.matter.order);

            // Clone for sidebar to avoid move issues
            let sidebar_articles = sorted_articles.clone();

            let sidebar_rendered = rsx! {
                <div class="sidebar">
                    <h3> "Hauchiwa Docs" </h3>
                    <ul>
                        @for doc in &sidebar_articles {
                            <li>
                                @let stem = doc.meta.path.file_stem().unwrap_or("index");
                                @let href = format!("{}.html", stem);

                                <a href={href}>
                                    (doc.matter.title.as_str())
                                </a>
                            </li>
                        }
                    </ul>
                </div>
            }
            .render()
            .into_inner();

            for doc in sorted_articles {
                let stem = doc.meta.path.file_stem().unwrap_or("index");
                let out_filename = format!("{}.html", stem);

                let mut options = Options::default();
                options.extension.table = true;
                options.extension.strikethrough = true;
                options.extension.tasklist = true;
                options.extension.header_ids = Some("".to_string());

                let mut plugins = Plugins::default();
                plugins.render.codefence_syntax_highlighter = Some(&highlight::TreeSitter);

                let content_html = markdown_to_html_with_plugins(&doc.text, &options, &plugins);
                let content_raw = hypertext::Raw::dangerously_create(content_html);

                let page_title = &doc.matter.title;
                let sidebar_raw = hypertext::Raw::dangerously_create(sidebar_rendered.clone());

                let page_html = rsx! {
                    <html lang="en">
                        <head>
                            <meta charset="UTF-8" />
                            <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                            <title> (page_title) " | Hauchiwa Docs" </title>
                            <link rel="stylesheet" href={css_href} />
                        </head>
                        <body>
                            (sidebar_raw)
                            <div class="main">
                                (content_raw)
                            </div>
                        </body>
                    </html>
                };

                let full_html = format!("<!DOCTYPE html>{}", page_html.render().into_inner());

                outputs.push(Output {
                    path: out_filename.into(),
                    data: OutputData::Utf8(full_html),
                });
            }

            Ok(outputs)
        });

    let mut website = config.finish();

    match args.mode {
        Mode::Build => {
            website.build(())?;
        }
        Mode::Watch => {
            website.watch(())?;
        }
    };

    Ok(())
}
