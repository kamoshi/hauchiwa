use hauchiwa::{
    executor,
    page::Page,
    {Site, SiteConfig},
};
use std::fs;
use std::path::Path;

fn main() {
    let mut config = SiteConfig::new();

    config.add_task((), |_, _| {
        let content = "<h1>Hello from Live Reload!</h1>".to_string();
        if !Path::new("dist").exists() {
            fs::create_dir("dist").unwrap();
        }
        fs::write("dist/index.html", &content).unwrap();
        Page {
            url: "/".to_string(),
            content,
        }
    });

    let mut site = Site::new(config);
    executor::watch(&mut site, ());
}
