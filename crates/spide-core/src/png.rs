//! PNG export: render the exact SVG the canvas shows with resvg.

use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

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

/// Render an SVG string to PNG bytes at `scale` (e.g. 2.0 for retina quality).
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
    pixmap.encode_png().map_err(|e| e.to_string())
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
