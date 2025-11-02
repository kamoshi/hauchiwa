//! An example that demonstrates how to bundle JavaScript files.

use hauchiwa::{
    Site, SiteConfig, executor,
    loader::{JS, Registry, build_scripts},
    page::Page,
};

fn main() {
    let mut config = SiteConfig::new();

    let scripts_handle = build_scripts(
        &mut config,
        "examples/script_bundle_data/**/[!_]*.js",
        "examples/script_bundle_data/**/*.js",
    );

    config.add_task((scripts_handle,), |_, (scripts,): (&Registry<JS>,)| {
        let script = scripts
            .get("examples/script_bundle_data/main.js")
            .unwrap();
        Page {
            url: "/".into(),
            content: format!(
                "<html><head><script src=\"{}\"></script></head><body><h1>Hello, world!</h1></body></html>",
                script.path
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
