use hauchiwa::{Blueprint, Output, output::OutputData};
use serde::Deserialize;

/// This example demonstrates the "Loaders" pattern in Hauchiwa.
///
/// It shows how to:
/// 1. Set up a Blueprint (the build plan).
/// 2. Load different types of assets (Markdown, CSS, JS, Images).
/// 3. Define a Task that consumes these assets.
/// 4. Generate an HTML output.
///
/// Prerequisite file structure for this example:
/// ├── examples/
/// │   └── assets/
/// │       ├── content/ (put .md files here)
/// │       ├── styles/ (put .scss files here)
/// │       ├── scripts/ (put .ts files here)
/// │       └── images/ (put .ppm files here)

// Define the structure of your Frontmatter (metadata at the top of Markdown files).
// This must derive `Deserialize` so Hauchiwa can parse the YAML header.
#[derive(Clone, Deserialize)]
struct Post {
    title: String,
}

fn main() -> anyhow::Result<()> {
    // -----------------------------------------------------------------------
    // 1. Initialize the Blueprint
    // -----------------------------------------------------------------------
    // The Blueprint is the central registry for your build configuration.
    // The generic parameter `<()>` indicates we are not using any custom
    // global configuration struct for this simple example.
    let mut config = Blueprint::<()>::new();

    // -----------------------------------------------------------------------
    // 2. Define loaders
    // -----------------------------------------------------------------------
    // Loaders scan the filesystem and prepare assets for processing.
    // They return "Handles" (lightweight references) that represent the future
    // result of loading these files.

    // A. Markdown Documents
    // We tell Hauchiwa to look for `.md` files in the content folder.
    // We explicitly pass `<Post>` so it knows how to parse the frontmatter.
    let posts = config
        .load_documents::<Post>()
        .source("examples/assets/content/*.md")
        .register()?;

    // B. Styles
    // We specify an "entry point" (main.scss) which imports other files,
    // and a "watch pattern" (**/*.scss) so the build triggers on any change.
    // Note: This uses the internal `grass` crate for compilation.
    let styles = config
        .load_css()
        .entry("examples/assets/styles/main.scss")
        .watch("examples/assets/styles/**/*.scss")
        .register()?;

    // C. Scripts
    // This uses `esbuild` (which must be installed in your environment) to
    // bundle modules starting from `main.js`.
    let scripts = config
        .load_js()
        .entry("examples/assets/scripts/main.ts")
        .watch("examples/assets/scripts/**/*.ts")
        .register()?;

    // D. Images
    // This loader finds images, optimizes them, and converts them to WebP.
    // The result is cached to speed up subsequent builds.
    let images = config
        .load_images()
        .format(hauchiwa::loader::image::ImageFormat::WebP)
        .source("examples/assets/images/*.ppm")
        .register()?;

    // -----------------------------------------------------------------------
    // 3. Define the build task
    // -----------------------------------------------------------------------
    // The `task!` macro constructs the dependency graph.
    // We pass our `config` and a closure. The arguments to the closure
    // (posts, styles, etc.) match the Handles we created above.
    //
    // Inside the closure, these variables are "resolved". They are no longer
    // handles, but the actual data (Maps of file paths to content).
    config
        .task()
        .depends_on((posts, styles, scripts, images))
        .run(|_, (posts, styles, scripts, images)| {
            // Start building the HTML string.
            let mut html = String::from("<html><head>");

            // Inject the link tag for our compiled CSS.
            // `styles.values()` gives us access to the processed CSS metadata.
            for css in styles {
                html.push_str(&format!(r#"<link rel="stylesheet" href="{}">"#, css.path));
            }

            html.push_str("</head><body>");
            html.push_str("<h1>My Blog</h1>");

            // Example: Find and display a specific logo image.
            // `images` is a Map, but we can search it using a glob pattern.
            if let Some(logo) = images.glob("**/logo.ppm")?.next() {
                // logo.1 contains the Image metadata (like its final path in `dist`).
                html.push_str(&format!(r#"<img src="{}" alt="Logo">"#, logo.1.default));
            }

            html.push_str("<ul>");

            // Iterate over our markdown posts and create a list.
            for post in &posts {
                html.push_str(&format!("<li>{}</li>", post.matter.title));
            }
            html.push_str("</ul>");

            // Inject the script tag for our compiled JS.
            for js in &scripts {
                html.push_str(&format!(
                    r#"<script type="module" src="{}"></script>"#,
                    js.path
                ));
            }

            html.push_str("</body></html>");

            // -------------------------------------------------------------------
            // 4. Return Output
            // -------------------------------------------------------------------
            // We return an `Output` struct which tells Hauchiwa to write a file
            // named "index.html" with the content we just generated.
            Ok(Output {
                path: "index.html".into(),
                data: OutputData::Utf8(html),
            })
        });

    // -----------------------------------------------------------------------
    // 5. Execute the build
    // -----------------------------------------------------------------------
    // Freeze the configuration and run the build engine.
    // The `()` argument is the global data available in `_ctx` in the task above.
    config.finish().build(())?;

    println!("Build complete! Check the 'dist' folder.");

    Ok(())
}
