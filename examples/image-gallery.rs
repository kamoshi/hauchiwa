
use hauchiwa::{
    executor,
    loader::{self, Registry},
    page::Page,
    {Site, SiteConfig},
};

fn main() {
    let mut config = SiteConfig::new();

    let images_handle = loader::glob_images(&mut config, "examples/images/*.jpg");

    config.add_task((images_handle,), |_, (images,): (&Registry<loader::Image>,)| {
        let image_tags = images
            .values()
            .map(|image| {
                let path = image.path().unwrap();
                format!("<img src=\"{}\" />", path)
            })
            .collect::<String>();

        Page {
            url: "/index.html".into(),
            content: format!("<h1>Image Gallery</h1>{}", image_tags),
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
