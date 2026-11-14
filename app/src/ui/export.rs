//! File › Export…: PNG, SVG, PDF (from the diagram scene), PlantUML, design JSON, ADRs, ArchUnit;
//! a live preview of exactly what will be written; settings persisted per project.

use super::theme::{bold, css, mono, sans};
use super::widgets::{self, button, icon_button, link, primary_enabled, segmented};
use egui::{Color32, Margin, RichText, Stroke, TextureHandle, Vec2};
use serde::{Deserialize, Serialize};
use spide_diagram::svg::ExportOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Svg,
    Pdf,
    Puml,
    Json,
    Adr,
    Archunit,
}

pub struct FormatInfo {
    pub id: Format,
    pub label: &'static str,
    pub help: &'static str,
    pub ext: &'static str,
    pub image: bool,
}

pub const FORMATS: [FormatInfo; 7] = [
    FormatInfo { id: Format::Png, label: "PNG", help: "Pixel image, sharp at any scale you choose", ext: "png", image: true },
    FormatInfo { id: Format::Svg, label: "SVG", help: "Vector image for the web and design tools", ext: "svg", image: true },
    FormatInfo { id: Format::Pdf, label: "PDF", help: "Vector document with selectable text, for print and reports", ext: "pdf", image: true },
    FormatInfo { id: Format::Puml, label: "PlantUML", help: "Text diagram of this view (PlantUML lays it out itself)", ext: "puml", image: false },
    FormatInfo { id: Format::Json, label: "Design JSON", help: "The whole design, to open in Spide again", ext: "json", image: false },
    FormatInfo { id: Format::Adr, label: "ADRs", help: "One Markdown decision record per decided decision", ext: "", image: false },
    FormatInfo { id: Format::Archunit, label: "ArchUnit", help: "A JUnit test that enforces your architecture rules in Java", ext: "java", image: false },
];

pub fn info(f: Format) -> &'static FormatInfo {
    FORMATS.iter().find(|x| x.id == f).unwrap()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: Format,
    pub options: ExportOptions,
    pub scale: u8,
    pub last_path: Option<String>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { format: Format::Png, options: ExportOptions::default(), scale: 2, last_path: None }
    }
}

#[derive(Default)]
pub enum ExportState {
    #[default]
    Idle,
    Busy,
    Done(String),
    Error(String),
}

pub struct ExportDialog {
    pub settings: ExportSettings,
    pub state: ExportState,
    pub preview: Option<(TextureHandle, u32, u32)>,
    pub preview_key: String,
    pub preview_at: f64,
    pub empty: bool,
}

impl ExportDialog {
    pub fn new(settings: ExportSettings) -> Self {
        Self { settings, state: ExportState::Idle, preview: None, preview_key: String::new(), preview_at: 0.0, empty: false }
    }
}

pub enum DialogOut {
    Close,
    Export,
    Reveal(String),
    /// settings changed (persist)
    Changed,
}

/// The export sheet. `svg` builds the current view's export SVG for the given options.
pub fn dialog(ctx: &egui::Context, d: &mut ExportDialog, view_title: &str, svg: &mut dyn FnMut(&ExportOptions) -> Option<(String, u32, u32)>) -> Option<DialogOut> {
    let mut out = None;
    let now = ctx.input(|i| i.time);
    let f = info(d.settings.format);
    // live preview of exactly what will be written (debounced)
    if f.image {
        let key = serde_json::to_string(&d.settings.options).unwrap_or_default();
        if key != d.preview_key {
            if d.preview_at == 0.0 {
                d.preview_at = now + 0.12;
            }
            if now >= d.preview_at {
                d.preview_key = key;
                d.preview_at = 0.0;
                match svg(&d.settings.options) {
                    Some((s, w, h)) => {
                        let scale = (1100.0 / w.max(h) as f32).min(2.0);
                        match spide_core::png::render_rgba(&s, scale) {
                            Ok((pw, ph, px)) => {
                                let img = egui::ColorImage::from_rgba_premultiplied([pw as usize, ph as usize], &px);
                                d.preview = Some((ctx.load_texture("export-preview", img, egui::TextureOptions::LINEAR), w, h));
                                d.empty = false;
                            }
                            Err(_) => d.preview = None,
                        }
                    }
                    None => {
                        d.preview = None;
                        d.empty = true;
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
        }
    }
    let screen = ctx.content_rect();
    // backdrop
    egui::Area::new(egui::Id::unique("export-backdrop")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(8, 9, 12, 140));
        if resp.clicked() {
            out = Some(DialogOut::Close);
        }
    });
    let w = (screen.width() - 48.0).min(980.0);
    let h = (screen.height() - 64.0).min(700.0);
    egui::Area::new(egui::Id::unique("export-sheet")).order(egui::Order::Tooltip).pivot(egui::Align2::CENTER_CENTER).fixed_pos(screen.center()).show(ctx, |ui| {
        egui::Frame::new()
            .fill(css::PANEL_SOLID)
            .stroke(Stroke::new(1.0, css::LINE_STRONG))
            .corner_radius(16)
            .shadow(egui::Shadow { offset: [0, 30], blur: 80, spread: 0, color: Color32::from_black_alpha(140) })
            .show(ui, |ui| {
                ui.set_width(w);
                ui.set_height(h);
                egui::Frame::new().inner_margin(Margin { left: 18, right: 14, top: 16, bottom: 12 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            widgets::kicker(ui, "Export");
                            ui.label(RichText::new(view_title).font(bold(16.0)).color(css::TEXT));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if icon_button(ui, "×").on_hover_text("Close export").clicked() {
                                out = Some(DialogOut::Close);
                            }
                        });
                    });
                });
                widgets::hline(ui);
                let body_h = h - 130.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let pw = (w * 0.575).max(300.0);
                    let (pr, _) = ui.allocate_exact_size(Vec2::new(pw, body_h), egui::Sense::hover());
                    // checkerboard
                    let p = ui.painter_at(pr);
                    let sq = 8.0;
                    let mut y = pr.top();
                    let mut row = 0;
                    while y < pr.bottom() {
                        let mut x = pr.left();
                        let mut col = row;
                        while x < pr.right() {
                            p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, y), Vec2::splat(sq)), 0.0, if col % 2 == 0 { super::theme::hex(0x1a1c21) } else { super::theme::hex(0x16181c) });
                            x += sq;
                            col += 1;
                        }
                        y += sq;
                        row += 1;
                    }
                    p.vline(pr.right(), pr.y_range(), Stroke::new(1.0, css::LINE));
                    if f.image {
                        match &d.preview {
                            Some((tex, iw, ih)) => {
                                let avail = pr.shrink(22.0);
                                let s = (avail.width() / *iw as f32).min(avail.height() / *ih as f32).min(1.0);
                                let size = Vec2::new(*iw as f32 * s, *ih as f32 * s);
                                let r = egui::Rect::from_center_size(avail.center(), size);
                                p.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            None => {
                                p.text(pr.center(), egui::Align2::CENTER_CENTER, if d.empty { "Nothing to export in this view yet." } else { "Preparing preview…" }, sans(12.5), css::FAINT);
                            }
                        }
                    } else {
                        p.text(pr.center() - Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, f.label, bold(18.0), css::TEXT);
                        p.text(pr.center() + Vec2::new(0.0, 12.0), egui::Align2::CENTER_CENTER, f.help, sans(12.5), css::DIM);
                    }
                    // the size label drawn properly (on top of its plate)
                    if let (true, Some((_, iw, ih))) = (f.image, &d.preview) {
                        let size_text = match d.settings.format {
                            Format::Png => format!("{} × {} px", (*iw as f32 * d.settings.scale as f32).round(), (*ih as f32 * d.settings.scale as f32).round()),
                            Format::Pdf => format!("{} × {} mm page", ((*iw as f32 / 96.0) * 25.4).round(), ((*ih as f32 / 96.0) * 25.4).round()),
                            _ => format!("{iw} × {ih} px canvas"),
                        };
                        let g = p.layout_no_wrap(size_text, mono(11.0), css::DIM);
                        let r = egui::Rect::from_min_size(pr.right_bottom() - Vec2::new(12.0, 10.0) - g.size(), g.size());
                        p.rect_filled(r.expand2(Vec2::new(8.0, 2.0)), 10.0, Color32::from_black_alpha(115));
                        p.galley(r.min, g, css::DIM);
                    }
                    ui.allocate_ui_with_layout(Vec2::new(w - pw, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            egui::Frame::new().inner_margin(Margin::symmetric(18, 16)).show(ui, |ui| {
                                ui.set_width(w - pw - 36.0);
                                ui.spacing_mut().item_spacing.y = 14.0;
                                egui::Grid::new("formats").spacing(Vec2::new(4.0, 4.0)).show(ui, |ui| {
                                    let cw = (w - pw - 36.0 - 12.0) / 4.0;
                                    for (i, x) in FORMATS.iter().enumerate() {
                                        let on = x.id == d.settings.format;
                                        let b = egui::Button::new(RichText::new(x.label).font(if on { bold(12.0) } else { sans(12.0) }).color(if on { css::INK } else { css::DIM }))
                                            .fill(if on { css::ACCENT } else { css::RAISED })
                                            .stroke(Stroke::new(1.0, if on { Color32::TRANSPARENT } else { css::LINE }))
                                            .corner_radius(6)
                                            .min_size(Vec2::new(cw, 30.0));
                                        if ui.add(b).on_hover_text(x.help).clicked() {
                                            d.settings.format = x.id;
                                            d.preview_key.clear();
                                            out = Some(DialogOut::Changed);
                                        }
                                        if i % 4 == 3 {
                                            ui.end_row();
                                        }
                                    }
                                });
                                widgets::para(ui, f.help, 12.0, css::DIM);
                                if f.image {
                                    let o = &mut d.settings.options;
                                    let mut changed = false;
                                    let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                                        ui.horizontal(|ui| {
                                            ui.add_sized(Vec2::new(92.0, 20.0), egui::Label::new(RichText::new(label.to_uppercase()).font(bold(11.0)).color(css::FAINT)));
                                            add(ui);
                                        });
                                    };
                                    row(ui, "Theme", &mut |ui| {
                                        if let Some(v) = segmented(ui, &o.theme, &[("light".to_string(), "Light (print)"), ("dark".to_string(), "Dark")]) {
                                            o.theme = v;
                                            changed = true;
                                        }
                                    });
                                    if d.settings.format != Format::Pdf {
                                        row(ui, "Background", &mut |ui| {
                                            if let Some(v) = segmented(ui, &o.background, &[("solid".to_string(), "Solid"), ("transparent".to_string(), "Transparent")]) {
                                                o.background = v;
                                                changed = true;
                                            }
                                        });
                                    }
                                    if d.settings.format == Format::Png {
                                        let mut sc = d.settings.scale;
                                        row(ui, "Scale", &mut |ui| {
                                            if let Some(v) = segmented(ui, &sc, &[(1u8, "1×"), (2, "2×"), (3, "3×"), (4, "4×")]) {
                                                sc = v;
                                            }
                                        });
                                        if sc != d.settings.scale {
                                            d.settings.scale = sc;
                                            out = Some(DialogOut::Changed);
                                        }
                                    }
                                    row(ui, "Margins", &mut |ui| {
                                        if let Some(v) = segmented(ui, &o.padding, &[("compact".to_string(), "Compact"), ("comfortable".to_string(), "Comfortable")]) {
                                            o.padding = v;
                                            changed = true;
                                        }
                                    });
                                    ui.horizontal_top(|ui| {
                                        ui.add_sized(Vec2::new(92.0, 20.0), egui::Label::new(RichText::new("INCLUDE").font(bold(11.0)).color(css::FAINT)));
                                        egui::Grid::new("includes").num_columns(2).spacing(Vec2::new(12.0, 6.0)).show(ui, |ui| {
                                            let items: [(&mut bool, &str, &str); 7] = [
                                                (&mut o.title, "Title", ""),
                                                (&mut o.date, "Date", ""),
                                                (&mut o.health, "Health score", ""),
                                                (&mut o.legend, "Legend", "Notation and colours used in this view"),
                                                (&mut o.packages, "Package frames", ""),
                                                (&mut o.ghosts, "Ghost proposals", "Proposed but not yet accepted changes"),
                                                (&mut o.diagnostics, "Diagnostics", "Heat halos, issue chips, member finding markers, cycle glow and rule warnings. Off: clean UML shapes"),
                                            ];
                                            for (i, (v, label, help)) in items.into_iter().enumerate() {
                                                let r = ui.checkbox(v, RichText::new(label).font(sans(12.5)).color(css::TEXT));
                                                let r = if help.is_empty() { r } else { r.on_hover_text(help) };
                                                if r.changed() {
                                                    changed = true;
                                                }
                                                if i % 2 == 1 {
                                                    ui.end_row();
                                                }
                                            }
                                        });
                                    });
                                    if changed {
                                        out = Some(DialogOut::Changed);
                                    }
                                }
                            });
                        });
                    });
                });
                widgets::hline(ui);
                egui::Frame::new().fill(css::SURFACE).inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
                    ui.set_width(w - 36.0);
                    ui.horizontal(|ui| {
                        match &d.state {
                            ExportState::Error(e) => {
                                ui.label(RichText::new(e).font(sans(11.5)).color(super::theme::hex(0xffb0b8)));
                            }
                            ExportState::Done(p) => {
                                let name = p.rsplit('/').next().unwrap_or(p).to_string();
                                ui.label(RichText::new(format!("Saved {name}")).font(sans(12.0)).color(css::GOOD));
                                if link(ui, "Reveal in Finder").clicked() {
                                    out = Some(DialogOut::Reveal(p.clone()));
                                }
                            }
                            _ => {}
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let busy = matches!(d.state, ExportState::Busy);
                            let label = if busy {
                                "Exporting…"
                            } else if d.settings.format == Format::Adr {
                                "Choose folder…"
                            } else {
                                "Export…"
                            };
                            if primary_enabled(ui, !busy && (!f.image || d.preview.is_some()), label).clicked() {
                                out = Some(DialogOut::Export);
                            }
                            if button(ui, if matches!(d.state, ExportState::Done(_)) { "Done" } else { "Cancel" }).clicked() {
                                out = Some(DialogOut::Close);
                            }
                        });
                    });
                });
            });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out = Some(DialogOut::Close);
    }
    out
}

/// Ask where to write (or which folder for ADRs). None: cancelled.
pub fn ask_path(format: Format, base_name: &str, project: &str) -> Option<String> {
    let f = info(format);
    if format == Format::Adr {
        return rfd::FileDialog::new().set_title("Folder for the ADRs (e.g. docs/adr)").set_directory(project).pick_folder().map(|p| p.display().to_string());
    }
    rfd::FileDialog::new().set_file_name(format!("{base_name}.{}", f.ext)).add_filter(f.label, &[f.ext]).save_file().map(|p| p.display().to_string())
}
