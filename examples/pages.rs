use hauchiwa::{
    executor,
    page::Page,
    {Site, SiteConfig},
};

fn main() {
    let mut config = SiteConfig::new();

    config.add_task((), |_, _| Page {
        url: "/".into(),
        content: "<h1>Hello, world!</h1>".to_string(),
    });

    config.add_task((), |_, _| {
        vec![
            Page {
                url: "/about".into(),
                content: "<h1>About us</h1>".to_string(),
            },
            Page {
                url: "/contact".into(),
                content: "<h1>Contact us</h1>".to_string(),
            },
        ]
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
