//! PDF export: the exact SVG the canvas shows, converted to a vector PDF (selectable, embedded text).

use std::sync::{Arc, OnceLock};
use svg2pdf::usvg;

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

/// Render an SVG string to PDF bytes. The page is the diagram's size (1 SVG px = 1/96 inch).
pub fn render(svg: &str) -> Result<Vec<u8>, String> {
    let opt = usvg::Options { fontdb: fontdb(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("svg: {e}"))?;
    let conv = svg2pdf::ConversionOptions { raster_scale: 2.0, ..Default::default() };
    svg2pdf::to_pdf(&tree, conv, svg2pdf::PageOptions { dpi: 96.0 }).map_err(|e| format!("pdf: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_a_vector_pdf_with_text() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
            <rect width="200" height="100" fill="#ffffff"/><path d="M10,50 L100,50 Q110,50 110,60" stroke="#333" fill="none"/>
            <text x="20" y="30" font-family="Helvetica Neue, sans-serif" font-size="14">LendingService</text></svg>"##;
        let pdf = super::render(svg).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.windows(5).any(|w| w == b"/Font"), "text is embedded as a font, not outlines");
        // 200×100 px at 96 dpi = 150×75 pt
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("150") && s.contains("75"), "page size in points");
    }

    #[test]
    fn rejects_garbage() {
        assert!(super::render("not svg").is_err());
    }
}
