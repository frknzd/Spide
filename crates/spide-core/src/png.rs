//! PNG export: render the exact SVG the canvas shows with resvg.
//!
//! The file records its resolution (`pHYs` = 72 × scale ppi): viewers and documents then show a 2×
//! export at its 1× size with every pixel sharp (Retina), instead of treating it as a 72-dpi image of
//! twice the size and resampling it (blurry).

use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

/// Points per inch: the 1× export is one pixel per point.
pub const BASE_DPI: f32 = 72.0;

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        db.set_sans_serif_family("Helvetica Neue");
        db.set_monospace_family("Menlo");
        Arc::new(db)
    })
    .clone()
}

/// The resolution a PNG rendered at `scale` declares.
pub fn dpi(scale: f32) -> f32 {
    BASE_DPI * scale.clamp(0.5, 6.0)
}

/// Render an SVG string to PNG bytes at `scale` (e.g. 2.0 for retina quality), declaring 72 × scale dpi.
pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    let opt = usvg::Options { fontdb: fontdb(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("svg: {e}"))?;
    let size = tree.size();
    let scale = scale.clamp(0.5, 6.0);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > 400_000_000 {
        return Err(format!("unreasonable image size {w}×{h}"));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("could not allocate image")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    encode(pixmap, dpi(scale))
}

/// PNG bytes (straight RGBA) with a `pHYs` chunk for `dpi`.
fn encode(pixmap: tiny_skia::Pixmap, dpi: f32) -> Result<Vec<u8>, String> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let rgba = pixmap.take_demultiplied();
    let ppm = (dpi as f64 / 0.0254).round() as u32;
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Render an SVG string to premultiplied RGBA pixels (previews in the export dialog).
pub fn render_rgba(svg: &str, scale: f32) -> Result<(u32, u32, Vec<u8>), String> {
    let opt = usvg::Options { fontdb: fontdb(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("svg: {e}"))?;
    let size = tree.size();
    let scale = scale.clamp(0.05, 6.0);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > 64_000_000 {
        return Err(format!("unreasonable image size {w}×{h}"));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("could not allocate image")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok((w, h, pixmap.take()))
}
