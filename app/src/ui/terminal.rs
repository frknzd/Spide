//! The embedded pi terminal: one ordered PTY stream parsed by `alacritty_terminal`, drawn as a cell
//! grid with egui. No terminal bytes or keystrokes drive design state.

use super::state::terminal_keys::{modified_key, KeyEvent};
use super::theme::{css, hex, sans, MONO_BOLD, MONO_BOLD_ITALIC, MONO_ITALIC};
use crate::events::{Events, UiEvent};
use crate::pi_bridge::PiBridge;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as TPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color as AColor, NamedColor, Processor, Rgb as ARgb};
use alacritty_terminal::Term;
use egui::{Color32, FontFamily, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Listener {
    replies: Arc<Mutex<Vec<String>>>,
}

impl EventListener for Listener {
    fn send_event(&self, e: TermEvent) {
        if let TermEvent::PtyWrite(s) = e {
            self.replies.lock().unwrap().push(s);
        }
    }
}

pub struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct TermState {
    pub term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    pub generation: Option<u64>,
    after: u64,
    pub modify_other_keys: bool,
    pub error: Option<String>,
    last_size: String,
    last_context: String,
    pub cols: usize,
    pub rows: usize,
    /// scan state for `CSI > 4 ; n m` split across reads
    tail: Vec<u8>,
}

impl TermState {
    fn new(listener: Listener) -> Self {
        let cfg = Config { scrolling_history: 10_000, ..Default::default() };
        Self {
            term: Term::new(cfg, &Size { cols: 90, rows: 40 }, listener.clone()),
            parser: Processor::new(),
            listener,
            generation: None,
            after: 0,
            modify_other_keys: false,
            error: None,
            last_size: String::new(),
            last_context: String::new(),
            cols: 90,
            rows: 40,
            tail: vec![],
        }
    }
    fn reset(&mut self) {
        let cfg = Config { scrolling_history: 10_000, ..Default::default() };
        self.term = Term::new(cfg, &Size { cols: self.cols, rows: self.rows }, self.listener.clone());
        self.parser = Processor::new();
        self.modify_other_keys = false;
        self.last_size.clear();
        self.tail.clear();
    }
    /// xterm modifyOtherKeys: `CSI > 4 ; 2 m` enables, other values disable.
    fn scan_modify_other_keys(&mut self, bytes: &[u8]) {
        let mut buf = std::mem::take(&mut self.tail);
        buf.extend_from_slice(bytes);
        let pat = b"\x1b[>4";
        let mut i = 0;
        while let Some(p) = buf[i..].windows(pat.len()).position(|w| w == pat) {
            let start = i + p + pat.len();
            let rest = &buf[start..];
            match rest.iter().position(|b| *b == b'm') {
                Some(end) => {
                    let params = &rest[..end];
                    self.modify_other_keys = params == b";2";
                    i = start + end + 1;
                }
                None if rest.len() < 8 => {
                    self.tail = buf[i + p..].to_vec();
                    return;
                }
                None => i = start,
            }
        }
        let keep = buf.len().saturating_sub(pat.len());
        self.tail = buf[keep..].to_vec();
    }
}

/// Where a terminal's bytes come from: the pi process, or the embedded Neovim.
pub trait PtySource: Send + Sync {
    fn read(&self, generation: Option<u64>, after: u64) -> Result<crate::terminal::Frame, String>;
    fn terminal(&self, generation: Option<u64>) -> Result<Arc<crate::terminal::Terminal>, String>;
    /// Which canvas tab the user looks at (pi uses it as prompt context).
    fn context(&self, _tab: String) {}
    /// Shown when the process exits.
    fn exit_message(&self) -> String {
        "The process exited.".into()
    }
}

impl PtySource for PiBridge {
    fn read(&self, generation: Option<u64>, after: u64) -> Result<crate::terminal::Frame, String> {
        PiBridge::read(self, generation, after)
    }
    fn terminal(&self, generation: Option<u64>) -> Result<Arc<crate::terminal::Terminal>, String> {
        PiBridge::terminal(self, generation)
    }
    fn context(&self, tab: String) {
        PiBridge::context(self, tab)
    }
    fn exit_message(&self) -> String {
        "pi exited. Use Restart to start a new interactive session.".into()
    }
}

/// The terminal session: a reader thread feeding the parser, and the widget state.
pub struct TermSession {
    pub state: Arc<Mutex<TermState>>,
    pi: Arc<dyn PtySource>,
    stop: Arc<AtomicBool>,
    pub tab: String,
    selecting: bool,
    pub has_focus: bool,
    scroll_acc: f32,
    pub focus_requested: bool,
}

impl Drop for TermSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl TermSession {
    pub fn start(pi: Arc<dyn PtySource>, events: Events) -> Self {
        let state = Arc::new(Mutex::new(TermState::new(Listener::default())));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (state, pi, stop) = (state.clone(), pi.clone(), stop.clone());
            std::thread::Builder::new()
                .name("spide-terminal".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        let (gen, after) = {
                            let s = state.lock().unwrap();
                            (s.generation, s.after)
                        };
                        match pi.read(gen, after) {
                            Ok(frame) => {
                                let replies;
                                let changed;
                                let exited = frame.exited;
                                {
                                    let mut s = state.lock().unwrap();
                                    let fresh = s.generation != Some(frame.generation) || frame.reset;
                                    if fresh {
                                        s.reset();
                                    }
                                    s.generation = Some(frame.generation);
                                    if !frame.bytes.is_empty() {
                                        s.scan_modify_other_keys(&frame.bytes);
                                        let st = &mut *s;
                                        st.parser.advance(&mut st.term, &frame.bytes);
                                    }
                                    // acknowledge only after parsing: backpressure stays end-to-end
                                    s.after = frame.next;
                                    let err = exited.then(|| pi.exit_message());
                                    changed = fresh || !frame.bytes.is_empty() || s.error != err;
                                    s.error = err;
                                    replies = std::mem::take(&mut *s.listener.replies.lock().unwrap());
                                }
                                if !replies.is_empty() {
                                    if let Ok(t) = pi.terminal(Some(frame.generation)) {
                                        for r in replies {
                                            let _ = t.write(r.as_bytes());
                                        }
                                    }
                                }
                                if changed {
                                    events.send(UiEvent::TerminalOutput);
                                }
                                if exited {
                                    std::thread::sleep(Duration::from_millis(500));
                                }
                            }
                            Err(e) => {
                                {
                                    let mut s = state.lock().unwrap();
                                    let changed = s.error.as_deref() != Some(e.as_str());
                                    s.generation = None;
                                    s.after = 0;
                                    s.error = Some(e);
                                    if changed {
                                        events.send(UiEvent::TerminalOutput);
                                    }
                                }
                                std::thread::sleep(Duration::from_millis(500));
                            }
                        }
                    }
                })
                .expect("terminal thread");
        }
        Self { state, pi, stop, tab: "class".into(), selecting: false, has_focus: false, scroll_acc: 0.0, focus_requested: false }
    }

    pub fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }

    fn send(&self, bytes: &[u8]) {
        let generation = self.state.lock().unwrap().generation;
        let Some(g) = generation else { return };
        match self.pi.terminal(Some(g)) {
            Ok(t) => {
                if let Err(e) = t.write(bytes) {
                    self.state.lock().unwrap().error = Some(e);
                }
            }
            Err(e) => self.state.lock().unwrap().error = Some(e),
        }
    }

    /// Tell pi which tab the user looks at (context for its prompts).
    pub fn context(&self, tab: &str) {
        let mut s = self.state.lock().unwrap();
        let Some(g) = s.generation else { return };
        let key = format!("{g}:{tab}");
        if key != s.last_context {
            s.last_context = key;
            drop(s);
            self.pi.context(tab.to_string());
        }
    }

    fn fit(&self, cols: usize, rows: usize) {
        let mut s = self.state.lock().unwrap();
        let Some(g) = s.generation else { return };
        let key = format!("{g}:{cols}:{rows}");
        if key == s.last_size {
            return;
        }
        s.last_size = key;
        s.cols = cols;
        s.rows = rows;
        s.term.resize(Size { cols, rows });
        drop(s);
        if let Ok(t) = self.pi.terminal(Some(g)) {
            if let Err(e) = t.resize(cols as u16, rows as u16) {
                self.state.lock().unwrap().error = Some(e);
            }
        }
    }

    /// Draw and drive the terminal in `rect`.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let font = FontId::new(12.0, FontFamily::Name(super::theme::MONO.into()));
        let (cw, ch) = ui.ctx().fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let ch = ch.ceil().max(13.0);
        let pad = 4.0;
        let inner = rect.shrink(pad);
        if inner.width() >= 20.0 && inner.height() >= 20.0 {
            let cols = ((inner.width() / cw).floor() as usize).max(10);
            let rows = ((inner.height() / ch).floor() as usize).max(3);
            self.fit(cols, rows);
        }
        self.context(&self.tab.clone());
        let id = ui.scope_id().with(("terminal", Arc::as_ptr(&self.state) as usize));
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() || std::mem::take(&mut self.focus_requested) {
            resp.request_focus();
        }
        self.has_focus = resp.has_focus();
        if self.has_focus {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, css::SURFACE);

        let mut out: Vec<u8> = vec![];
        let mut copy: Option<String> = None;
        {
            let mut s = self.state.lock().unwrap();
            let mode = *s.term.mode();
            // mouse: selection and scrolling
            let to_point = |s: &TermState, p: Pos2| {
                let col = (((p.x - inner.left()) / cw).floor().max(0.0) as usize).min(s.cols.saturating_sub(1));
                let row = (((p.y - inner.top()) / ch).floor().max(0.0) as usize).min(s.rows.saturating_sub(1));
                let off = s.term.grid().display_offset();
                let side = if ((p.x - inner.left()) / cw).fract() > 0.5 { Side::Right } else { Side::Left };
                (TPoint::new(Line(row as i32) - off, Column(col)), side)
            };
            if resp.drag_started() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let (pt, side) = to_point(&s, p);
                    s.term.selection = Some(Selection::new(SelectionType::Simple, pt, side));
                    self.selecting = true;
                }
            } else if resp.dragged() && self.selecting {
                if let Some(p) = resp.interact_pointer_pos() {
                    let (pt, side) = to_point(&s, p);
                    if let Some(sel) = s.term.selection.as_mut() {
                        sel.update(pt, side);
                    }
                }
            } else if resp.drag_stopped() {
                self.selecting = false;
            } else if resp.clicked() {
                s.term.selection = None;
            }
            if resp.double_clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let (pt, side) = to_point(&s, p);
                    s.term.selection = Some(Selection::new(SelectionType::Semantic, pt, side));
                }
            }
            if resp.hovered() {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy != 0.0 {
                    self.scroll_acc += dy / ch;
                    let lines = self.scroll_acc.trunc() as i32;
                    if lines != 0 {
                        self.scroll_acc -= lines as f32;
                        if mode.intersects(TermMode::MOUSE_MODE) {
                            // The app captures the mouse (pi's fullscreen transcript): report wheel events.
                            let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(inner.left_top());
                            let col = (((p.x - inner.left()) / cw).floor().max(0.0) as usize).min(s.cols.saturating_sub(1));
                            let row = (((p.y - inner.top()) / ch).floor().max(0.0) as usize).min(s.rows.saturating_sub(1));
                            let mods = ui.input(|i| i.modifiers);
                            for _ in 0..lines.abs() {
                                out.extend_from_slice(&encode_wheel(lines > 0, col, row, mods, mode));
                            }
                        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
                            let key = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                            for _ in 0..lines.abs() {
                                out.extend_from_slice(key);
                            }
                        } else {
                            s.term.scroll_display(Scroll::Delta(lines));
                        }
                    }
                }
            }
            // keyboard
            if self.has_focus {
                let events = ui.input(|i| i.events.clone());
                for ev in events {
                    match ev {
                        egui::Event::Text(t) => {
                            s.term.scroll_display(Scroll::Bottom);
                            out.extend_from_slice(t.as_bytes());
                        }
                        egui::Event::Paste(t) => {
                            let t = t.replace("\r\n", "\r").replace('\n', "\r");
                            if mode.contains(TermMode::BRACKETED_PASTE) {
                                out.extend_from_slice(b"\x1b[200~");
                                out.extend_from_slice(t.as_bytes());
                                out.extend_from_slice(b"\x1b[201~");
                            } else {
                                out.extend_from_slice(t.as_bytes());
                            }
                        }
                        egui::Event::Copy | egui::Event::Cut => {
                            copy = s.term.selection_to_string();
                        }
                        egui::Event::Key { key, pressed: true, modifiers, .. } => {
                            if let Some(bytes) = encode_key(key, modifiers, mode, s.modify_other_keys) {
                                s.term.scroll_display(Scroll::Bottom);
                                s.term.selection = None;
                                out.extend_from_slice(&bytes);
                            }
                        }
                        _ => {}
                    }
                }
            }
            paint(&painter, inner, cw, ch, &s, self.has_focus);
        }
        if let Some(c) = copy.filter(|c| !c.is_empty()) {
            ui.ctx().copy_text(c);
        }
        if !out.is_empty() {
            self.send(&out);
        }
    }
}

fn named(n: NamedColor) -> Color32 {
    use NamedColor::*;
    match n {
        Black => hex(0x2e3436),
        Red => hex(0xcc0000),
        Green => hex(0x4e9a06),
        Yellow => hex(0xc4a000),
        Blue => hex(0x3465a4),
        Magenta => hex(0x75507b),
        Cyan => hex(0x06989a),
        White => hex(0xd3d7cf),
        BrightBlack => hex(0x555753),
        BrightRed => hex(0xef2929),
        BrightGreen => hex(0x8ae234),
        BrightYellow => hex(0xfce94f),
        BrightBlue => hex(0x729fcf),
        BrightMagenta => hex(0xad7fa8),
        BrightCyan => hex(0x34e2e2),
        BrightWhite => hex(0xeeeeec),
        Foreground | BrightForeground => hex(0xe5e7eb),
        Background => hex(0x1b1e24),
        Cursor => hex(0xa8c7ff),
        DimForeground => hex(0xe5e7eb).gamma_multiply(0.6),
        DimBlack => hex(0x2e3436).gamma_multiply(0.66),
        DimRed => hex(0xcc0000).gamma_multiply(0.66),
        DimGreen => hex(0x4e9a06).gamma_multiply(0.66),
        DimYellow => hex(0xc4a000).gamma_multiply(0.66),
        DimBlue => hex(0x3465a4).gamma_multiply(0.66),
        DimMagenta => hex(0x75507b).gamma_multiply(0.66),
        DimCyan => hex(0x06989a).gamma_multiply(0.66),
        DimWhite => hex(0xd3d7cf).gamma_multiply(0.66),
    }
}

fn indexed(i: u8) -> Color32 {
    match i {
        0..=15 => named(match i {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            _ => NamedColor::BrightWhite,
        }),
        16..=231 => {
            let i = i - 16;
            let v = |c: u8| if c == 0 { 0 } else { 55 + c * 40 };
            Color32::from_rgb(v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

fn resolve(c: AColor, colors: &alacritty_terminal::term::color::Colors) -> Color32 {
    let over = |idx: usize| colors[idx].map(|ARgb { r, g, b }| Color32::from_rgb(r, g, b));
    match c {
        AColor::Spec(ARgb { r, g, b }) => Color32::from_rgb(r, g, b),
        AColor::Indexed(i) => over(i as usize).unwrap_or_else(|| indexed(i)),
        AColor::Named(n) => over(n as usize).unwrap_or_else(|| named(n)),
    }
}

fn cell_font(flags: Flags) -> FontId {
    let fam = match (flags.contains(Flags::BOLD), flags.contains(Flags::ITALIC)) {
        (true, true) => FontFamily::Name(MONO_BOLD_ITALIC.into()),
        (true, false) => FontFamily::Name(MONO_BOLD.into()),
        (false, true) => FontFamily::Name(MONO_ITALIC.into()),
        _ => FontFamily::Name(super::theme::MONO.into()),
    };
    FontId::new(12.0, fam)
}

fn paint(p: &egui::Painter, inner: Rect, cw: f32, ch: f32, s: &TermState, focused: bool) {
    let content = s.term.renderable_content();
    let colors = content.colors;
    let sel = content.selection;
    let off = content.display_offset as i32;
    let selection_bg = hex(0x435576);
    struct Run {
        x: f32,
        y: f32,
        text: String,
        color: Color32,
        flags: Flags,
    }
    let mut runs: Vec<Run> = vec![];
    for cell in content.display_iter {
        let row = cell.point.line.0 + off;
        let col = cell.point.column.0;
        let x = inner.left() + col as f32 * cw;
        let y = inner.top() + row as f32 * ch;
        if y > inner.bottom() {
            continue;
        }
        let flags = cell.flags;
        let mut fg = resolve(cell.fg, colors);
        let mut bg = resolve(cell.bg, colors);
        if flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.66);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        let selected = sel.is_some_and(|r| r.contains(cell.point));
        let wide = flags.contains(Flags::WIDE_CHAR);
        let w = if wide { cw * 2.0 } else { cw };
        if selected {
            p.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, ch)), 0.0, selection_bg);
        } else if bg != named(NamedColor::Background) {
            p.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(w + 0.5, ch)), 0.0, bg);
        }
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN) {
            continue;
        }
        let c = cell.c;
        if c == ' ' || c == '\t' || c == '\0' {
            continue;
        }
        let mut text = String::new();
        text.push(c);
        for z in cell.zerowidth().into_iter().flatten() {
            text.push(*z);
        }
        let style = flags & (Flags::BOLD | Flags::ITALIC | Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
        match runs.last_mut() {
            Some(r) if !wide && r.y == y && r.color == fg && r.flags == style && (r.x + r.text.chars().count() as f32 * cw - x).abs() < 0.5 && !text.chars().any(|c| c as u32 > 0x2fff) => r.text.push_str(&text),
            _ => runs.push(Run { x, y, text, color: fg, flags: style }),
        }
    }
    for r in runs {
        let font = cell_font(r.flags);
        let g = p.layout_no_wrap(r.text.clone(), font, r.color);
        let base = super::paint::galley_baseline(&g);
        let top = r.y + (ch * 0.78 - base).max(0.0);
        let width = r.text.chars().count() as f32 * cw;
        if r.flags.intersects(Flags::ALL_UNDERLINES) {
            p.hline(r.x..=r.x + width, r.y + ch - 1.5, Stroke::new(1.0, r.color));
        }
        if r.flags.contains(Flags::STRIKEOUT) {
            p.hline(r.x..=r.x + width, r.y + ch / 2.0, Stroke::new(1.0, r.color));
        }
        p.galley(Pos2::new(r.x, top), g, r.color);
    }
    // cursor
    let cur = content.cursor;
    if content.mode.contains(TermMode::SHOW_CURSOR) {
        let row = cur.point.line.0 + off;
        if row >= 0 && (row as usize) < s.rows {
            let r = Rect::from_min_size(Pos2::new(inner.left() + cur.point.column.0 as f32 * cw, inner.top() + row as f32 * ch), Vec2::new(cw, ch));
            let c = hex(0xa8c7ff);
            if focused {
                p.rect_filled(r, 0.0, c.gamma_multiply(0.85));
            } else {
                p.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
            }
        }
    }
}

/// xterm-style key encoding (the special keys; printable text arrives as text events).
fn encode_key(key: Key, m: Modifiers, mode: TermMode, modify_other_keys: bool) -> Option<Vec<u8>> {
    if m.mac_cmd || m.command && !m.ctrl {
        return None;
    }
    let name = key.name();
    let dom = match key {
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        _ if name.chars().count() == 1 => name,
        _ => "",
    };
    let letter_key = if m.shift { dom.to_uppercase() } else { dom.to_lowercase() };
    if let Some(seq) = modified_key(&KeyEvent { key: if key == Key::Enter { "Enter" } else { &letter_key }, shift: m.shift, ctrl: m.ctrl, alt: m.alt, meta: m.mac_cmd, down: true }, modify_other_keys) {
        return Some(seq.into_bytes());
    }
    let modn = 1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
    let app = mode.contains(TermMode::APP_CURSOR);
    let csi = |final_: &str| -> Vec<u8> {
        if modn > 1 {
            format!("\x1b[1;{modn}{final_}").into_bytes()
        } else if app {
            format!("\x1bO{final_}").into_bytes()
        } else {
            format!("\x1b[{final_}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modn > 1 {
            format!("\x1b[{n};{modn}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    Some(match key {
        Key::Enter => {
            if m.alt {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        Key::Tab => {
            if m.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        Key::Backspace => {
            if m.ctrl {
                b"\x08".to_vec()
            } else if m.alt {
                b"\x1b\x7f".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => csi("A"),
        Key::ArrowDown => csi("B"),
        Key::ArrowRight => csi("C"),
        Key::ArrowLeft => csi("D"),
        Key::Home => csi("H"),
        Key::End => csi("F"),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => csi("P"),
        Key::F2 => csi("Q"),
        Key::F3 => csi("R"),
        Key::F4 => csi("S"),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ if m.ctrl => {
            // Ctrl+letter → C0 control; Ctrl+Space → NUL; Ctrl+[ \ ] → ESC FS GS
            let c = name.chars().next()?;
            let b = match c.to_ascii_lowercase() {
                'a'..='z' => (c.to_ascii_lowercase() as u8) & 0x1f,
                ' ' => 0,
                '[' => 0x1b,
                '\\' => 0x1c,
                ']' => 0x1d,
                _ if key == Key::Space => 0,
                _ => return None,
            };
            if m.alt {
                vec![0x1b, b]
            } else {
                vec![b]
            }
        }
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneAction {
    Commands,
    Restart,
    Collapse,
    /// Switch the pane between pi and the embedded Neovim.
    ToggleEditor,
}

/// The pi pane header: title, status, Commands, Restart (with confirmation), collapse.
pub fn header(ui: &mut egui::Ui, status: crate::state::PiStatus, restarting: bool, confirming: &mut bool, palette_open: bool, editor_on: bool) -> Option<PaneAction> {
    use crate::state::PiStatus;
    let mut out = None;
    let (text, dot) = match status {
        PiStatus::Starting => ("Starting…", css::WARN),
        PiStatus::Idle => ("Ready", css::GOOD),
        PiStatus::Busy => ("Working", css::ACCENT),
        PiStatus::Offline => ("Offline", css::BAD),
    };
    ui.horizontal_centered(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new("pi").font(super::theme::bold(13.0)).color(css::TEXT));
        ui.add_space(6.0);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
        let a = if status == PiStatus::Busy { 0.35 + 0.45 * ((ui.input(|i| i.time) * std::f64::consts::TAU / 1.2).sin() * 0.5 + 0.5) as f32 } else { 1.0 };
        if status == PiStatus::Busy {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        ui.painter().circle_filled(r.center(), 3.5, dot.gamma_multiply(a));
        let st = ui.label(egui::RichText::new(text).font(sans(11.5)).color(if status == PiStatus::Offline { hex(0xffb0b8) } else { css::DIM }));
        if status == PiStatus::Starting {
            st.on_hover_text("If pi asks “Trust project folder?”, answer it in the terminal below.");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            if super::widgets::icon_button(ui, "‹").on_hover_text("Collapse pi pane").clicked() {
                out = Some(PaneAction::Collapse);
            }
            let ed = egui::Button::new(egui::RichText::new(if editor_on { "◂ pi" } else { "Neovim ▸" }).font(sans(12.0)).color(if editor_on { css::ACCENT } else { css::DIM })).frame(false);
            if ui.add(ed).on_hover_text(if editor_on { "Back to the pi terminal" } else { "Show the embedded Neovim (your config; edits stay in Neovim)" }).clicked() {
                out = Some(PaneAction::ToggleEditor);
            }
            if ui.add_enabled(!restarting, egui::Button::new(egui::RichText::new(if restarting { "Restarting…" } else { "Restart" }).font(sans(12.0)).color(css::DIM)).frame(false)).clicked() {
                *confirming = !*confirming;
            }
            let cmd = egui::Button::new(egui::RichText::new("Commands").font(sans(12.0)).color(if palette_open { css::ACCENT } else { css::DIM })).frame(false);
            if ui.add(cmd).on_hover_text("Spide commands, including open, save and import").clicked() {
                out = Some(PaneAction::Commands);
            }
        });
    });
    if *confirming && out.is_none() {
        // the confirm row is drawn by the caller below the header
    }
    let _ = &mut out;
    if out == Some(PaneAction::Restart) {
        *confirming = false;
    }
    out
}

/// The restart confirmation strip.
pub fn confirm_restart(ui: &mut egui::Ui) -> Option<bool> {
    let mut out = None;
    egui::Frame::new().fill(Color32::from_rgba_unmultiplied(255, 191, 105, 20)).inner_margin(egui::Margin::symmetric(12, 9)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(egui::RichText::new("Restart pi? This ends the current terminal process. Spide reopens the latest saved session.").font(sans(12.0)).color(hex(0xffe2bb))).wrap());
            if super::widgets::warn_button(ui, "Restart").clicked() {
                out = Some(true);
            }
            if super::widgets::button(ui, "Cancel").clicked() {
                out = Some(false);
            }
        });
    });
    out
}

/// The "pi needs attention" banner under the terminal.
pub fn error_banner(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new().fill(Color32::from_rgba_unmultiplied(255, 116, 130, 23)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new("pi needs attention").font(super::theme::bold(12.0)).color(hex(0xffd9dd)));
        ui.add(egui::Label::new(egui::RichText::new(text).font(sans(12.0)).color(hex(0xffc5cb))).wrap());
    });
}

/// Encode a mouse-wheel event for an app that enabled mouse reporting (`col`/`row` are 0-based).
fn encode_wheel(up: bool, col: usize, row: usize, mods: Modifiers, mode: TermMode) -> Vec<u8> {
    let mut button = if up { 64 } else { 65 };
    if mods.shift { button += 4; }
    if mods.alt { button += 8; }
    if mods.ctrl { button += 16; }
    if mode.contains(TermMode::SGR_MOUSE) {
        format!("\x1b[<{button};{};{}M", col + 1, row + 1).into_bytes()
    } else {
        // Legacy X10 encoding: each value offset by 32 and limited to one byte.
        let enc = |v: usize| (32 + v).min(255) as u8;
        vec![0x1b, b'[', b'M', enc(button), enc(col + 1), enc(row + 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_reports_follow_mouse_mode() {
        let sgr = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(encode_wheel(true, 0, 0, Modifiers::NONE, sgr), b"\x1b[<64;1;1M".to_vec());
        assert_eq!(encode_wheel(false, 9, 4, Modifiers::NONE, sgr), b"\x1b[<65;10;5M".to_vec());
        assert_eq!(encode_wheel(true, 0, 0, Modifiers::ALT, sgr), b"\x1b[<72;1;1M".to_vec());
        assert_eq!(encode_wheel(false, 1, 2, Modifiers::NONE, TermMode::MOUSE_REPORT_CLICK), vec![0x1b, b'[', b'M', 97, 34, 35]);
    }

    #[test]
    fn modify_other_keys_negotiation_survives_split_reads() {
        let mut s = TermState::new(Listener::default());
        s.scan_modify_other_keys(b"hello \x1b[>4");
        assert!(!s.modify_other_keys);
        s.scan_modify_other_keys(b";2m world");
        assert!(s.modify_other_keys);
        s.scan_modify_other_keys(b"\x1b[>4;0m");
        assert!(!s.modify_other_keys);
    }

    #[test]
    fn fake_pty_stream_renders_in_workspace() {
        use portable_pty::CommandBuilder;
        use crate::ui::tests::{harness, project, steps, shot};
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "printf '\\033[38;2;122;162;255mFake pi\\033[0m\\r\\nready > '"]);
        let pty = crate::terminal::Terminal::spawn(cmd, 42).unwrap();
        let mut state = TermState::new(Listener::default());
        let mut cursor = 0;
        for _ in 0..100 {
            let f = pty.read(cursor).unwrap();
            cursor = f.next;
            state.scan_modify_other_keys(&f.bytes);
            state.parser.advance(&mut state.term, &f.bytes);
            if f.exited { break; }
        }
        pty.stop();
        assert_eq!(state.term.grid()[TPoint::new(Line(0), Column(0))].c, 'F');
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        h.state_mut().term = Some(TermSession {
            state: Arc::new(Mutex::new(state)), pi: Arc::new(PiBridge::new()),
            stop: Arc::new(AtomicBool::new(false)), tab: "class".into(),
            selecting: false, has_focus: false, scroll_acc: 0.0, focus_requested: false,
        });
        steps(&mut h, 150);
        shot(&mut h, "terminal-fake-pty");
        assert!(h.state().term.as_ref().unwrap().state.lock().unwrap().cols > 10);
    }

    #[test]
    fn xterm_key_encoding() {
        let none = Modifiers::NONE;
        assert_eq!(encode_key(Key::Enter, none, TermMode::empty(), false), Some(b"\r".to_vec()));
        assert_eq!(encode_key(Key::Tab, Modifiers::SHIFT, TermMode::empty(), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(encode_key(Key::ArrowUp, none, TermMode::empty(), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key(Key::ArrowUp, none, TermMode::APP_CURSOR, false), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode_key(Key::P, Modifiers::CTRL, TermMode::empty(), false), Some(vec![0x10]));
        assert_eq!(encode_key(Key::L, Modifiers::CTRL, TermMode::empty(), false), Some(vec![0x0c]));
        assert_eq!(encode_key(Key::Enter, Modifiers::SHIFT, TermMode::empty(), true), Some(b"\x1b[27;2;13~".to_vec()));
        assert_eq!(encode_key(Key::P, Modifiers::CTRL | Modifiers::SHIFT, TermMode::empty(), true), Some(b"\x1b[27;6;112~".to_vec()));
        assert_eq!(encode_key(Key::C, Modifiers::MAC_CMD | Modifiers::COMMAND, TermMode::empty(), false), None);
    }
}
