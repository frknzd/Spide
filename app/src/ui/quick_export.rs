//! Clipboard diagram images and quick access to the last exported file's folder.
use super::app::SpideApp;
impl SpideApp {
    pub fn clipboard_diagram_svg(&mut self) -> Option<String> {
        let view = self.current_export_view();
        let settings = self.settings_for_project();
        self.export_svg_for(&view, None, &settings.options).map(|(svg, _, _)| svg)
    }
    pub fn copy_diagram_image(&mut self) {
        if self.copying_image { return; }
        let Some(svg) = self.clipboard_diagram_svg() else { return self.toast("info", "This diagram is empty; nothing to copy.") };
        self.copying_image = true;
        self.backend.call(move |_, _| spide_core::png::render_clipboard_rgba(&svg, 2.0), |app, result| {
            app.copying_image = false;
            match result {
                Err(e) => app.toast("error", e),
                Ok((w, h, rgba)) => {
                    let result = arboard::Clipboard::new().and_then(|mut c| c.set_image(arboard::ImageData {
                        width: w as usize, height: h as usize, bytes: std::borrow::Cow::Owned(rgba),
                    }));
                    match result {
                        Ok(()) => app.toast("info", format!("Diagram copied as image ({w}×{h}, 2×)")),
                        Err(e) => app.toast("error", format!("Could not copy diagram image: {e}")),
                    }
                }
            }
        });
    }
    pub fn last_export_folder(&self) -> Option<std::path::PathBuf> {
        let path = self.settings_for_project().last_path.map(std::path::PathBuf::from)?;
        let dir = if path.is_dir() { path } else { path.parent()?.to_path_buf() };
        dir.is_dir().then_some(dir)
    }
    pub fn open_export_folder(&mut self) {
        let Some(dir) = self.last_export_folder() else { return self.toast("info", "No previous export folder is available for this project.") };
        if let Err(e) = std::process::Command::new("open").arg(dir).spawn() { self.toast("error", e.to_string()); }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn copied_image_uses_existing_svg_pipeline_and_straight_rgba() {
        let dir = crate::ui::tests::project("library");
        let mut h = crate::ui::tests::harness(dir.path(), (1440.0, 900.0));
        crate::ui::tests::steps(&mut h, 120);
        let svg = h.state_mut().clipboard_diagram_svg().unwrap();
        let (w, h, rgba) = spide_core::png::render_clipboard_rgba(&svg, 1.0).unwrap();
        assert!(w > 10 && h > 10);
        assert_eq!(rgba.len(), w as usize * h as usize * 4);
        let transparent = r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="red" opacity="0.5"/></svg>"#;
        let (_, _, pixels) = spide_core::png::render_clipboard_rgba(transparent, 1.0).unwrap();
        assert_eq!(pixels[0], 255, "clipboard data is straight, not premultiplied RGBA");
        assert!((127..=128).contains(&pixels[3]));
    }
}
