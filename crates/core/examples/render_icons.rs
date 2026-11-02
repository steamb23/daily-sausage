//! Writes tray icon previews: `cargo run -p daily-sausage-core --example render_icons -- <out_dir>`
use daily_sausage_core::format::Level;
use daily_sausage_core::icon::render_tray;
use std::io::BufWriter;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&out).unwrap();
    let cases = [
        ("none", None, Level::Idle),
        ("ok-30", Some(0.3), Level::Ok),
        ("ok-60", Some(0.6), Level::Ok),
        ("warn-85", Some(0.85), Level::Warn),
        ("over-130", Some(1.3), Level::Over),
        ("stale-60", Some(0.6), Level::Idle),
    ];
    for size in [16u32, 32, 64] {
        for (name, ratio, level) in cases {
            let rgba = render_tray(size, ratio, level);
            let path = format!("{out}/tray-{name}-{size}.png");
            let f = std::fs::File::create(&path).unwrap();
            let mut enc = png::Encoder::new(BufWriter::new(f), size, size);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
    }
    println!("written to {out}");
}
