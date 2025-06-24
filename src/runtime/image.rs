pub(crate) fn process_image(buffer: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(buffer).expect("Couldn't load image");
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder
        .encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)
        .expect("Encoding error");

    out
}
