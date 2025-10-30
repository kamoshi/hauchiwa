//! An example that demonstrates how to bundle SCSS files.

use hauchiwa::{
    executor,
    loader::styles::{build_style, Style},
    page::Page,
    {Site, SiteConfig},
};

fn main() {
    let mut config = SiteConfig::new();

    let style_handle = build_style(
        &mut config,
        "examples/styles_bundle_data/main.scss",
        "examples/styles_bundle_data/**/*.scss",
    );

    config.add_task((style_handle,), |_, (style,): (&Style,)| {
        Page {
            url: "/".to_string(),
            content: format!(
                "<html><head><link rel=\"stylesheet\" href=\"{}\"></head><body><h1>Hello, world!</h1></body></html>",
                style.path
            ),
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
