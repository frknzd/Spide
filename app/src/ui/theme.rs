//! Look & feel: the dark palette of the old stylesheet mapped onto egui visuals, plus fonts.

pub fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../icons/128x128@2x.png")).unwrap_or_default()
}
