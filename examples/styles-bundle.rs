//! An example that demonstrates how to bundle SCSS files.

use hauchiwa::{
    executor,
    loader::styles::{build_styles, Styles},
    page::Page,
    {Site, SiteConfig},
};

fn main() {
    let mut config = SiteConfig::new();

    let styles_handle = build_styles(
        &mut config,
        "examples/styles_bundle_data/**/[!_]*.scss",
        "examples/styles_bundle_data/**/*.scss",
    );

    config.add_task((styles_handle,), |_, (styles,): (&Styles,)| {
        let style = styles
            .get("examples/styles_bundle_data/main.scss")
            .unwrap();
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
