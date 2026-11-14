//! Hover intent (port of `state/hover.ts`): target intent, not pointer stillness. Moving within a
//! target never postpones opening.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    pub key: String,
    pub at: Point,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Timer {
    Close,
    Open(String),
}

#[derive(Default)]
pub struct HoverStore {
    value: Option<Hover>,
    target: Option<String>,
    at: Point,
    timer: Option<(f64, Timer)>,
}

impl HoverStore {
    pub fn snapshot(&self) -> Option<&Hover> {
        self.value.as_ref()
    }
    /// The class the hovered element belongs to.
    pub fn class_key(&self) -> Option<String> {
        let k = &self.value.as_ref()?.key;
        if let Some(c) = k.strip_prefix("class:") {
            return Some(c.to_string());
        }
        let member = k.strip_prefix("method:").or_else(|| k.strip_prefix("attribute:"))?;
        member.split('(').next().unwrap_or(member).rfind('.').map(|i| member[..i].to_string())
    }
    /// A pinned card stays put and accepts the pointer until it is closed.
    pub fn pin(&mut self, key: &str) {
        self.timer = None;
        self.target = Some(key.to_string());
        let at = match &self.value {
            Some(v) if v.key == key => v.at,
            _ => self.at,
        };
        self.value = Some(Hover { key: key.to_string(), at, pinned: true });
    }
    pub fn move_to(&mut self, key: Option<&str>, at: Point, now: f64) {
        if self.value.as_ref().is_some_and(|v| v.pinned) {
            self.at = at;
            return;
        }
        self.at = at;
        if key.map(String::from) == self.target {
            // same target: follow the pointer (next frame in the old UI)
            if let (Some(k), Some(v)) = (key, self.value.as_mut()) {
                if v.key == k {
                    v.at = at;
                }
            }
            return;
        }
        self.target = key.map(String::from);
        self.timer = None;
        match key {
            None => self.timer = Some((now + 0.08, Timer::Close)),
            Some(k) if self.value.is_some() => self.value = Some(Hover { key: k.to_string(), at, pinned: false }),
            Some(k) => self.timer = Some((now + 0.06, Timer::Open(k.to_string()))),
        }
    }
    pub fn clear(&mut self) {
        self.timer = None;
        self.target = None;
        self.value = None;
    }
    /// Fire due timers. Returns true while a timer is pending (repaint later).
    pub fn tick(&mut self, now: f64) -> Option<f64> {
        if let Some((due, t)) = self.timer.clone() {
            if now >= due {
                self.timer = None;
                match t {
                    Timer::Close => self.value = None,
                    Timer::Open(k) => {
                        if self.target.as_deref() == Some(k.as_str()) {
                            self.value = Some(Hover { key: k, at: self.at, pinned: false });
                        }
                    }
                }
                return None;
            }
            return Some(due - now);
        }
        None
    }
}

pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

pub struct Placement {
    pub left: f32,
    pub top: f32,
    pub max_width: f32,
    pub max_height: f32,
}

/// Fit an actual measured card into the canvas (never the adjacent panes).
pub fn place_card(at: Point, size: (f32, f32), b: &Bounds) -> Placement {
    let pad = 8.0;
    let width = size.0.min(b.right - b.left - 2.0 * pad);
    let height = size.1.min(b.bottom - b.top - 2.0 * pad);
    let x = if at.x + 18.0 + width > b.right - pad { at.x - width - 18.0 } else { at.x + 18.0 };
    Placement {
        left: (b.left + pad).max(x.min(b.right - width - pad)),
        top: (b.top + pad).max((at.y + 12.0).min(b.bottom - height - pad)),
        max_width: (b.right - b.left - 2.0 * pad).max(0.0),
        max_height: (b.bottom - b.top - 2.0 * pad).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn motion_over_one_target_does_not_postpone_the_intent_timer() {
        let mut s = HoverStore::default();
        let mut t = 0.0;
        s.move_to(Some("class:A"), p(1.0, 2.0), t);
        t += 0.040;
        s.tick(t);
        s.move_to(Some("class:A"), p(9.0, 10.0), t);
        t += 0.020;
        s.tick(t);
        assert_eq!(s.snapshot().map(|h| h.key.as_str()), Some("class:A"));
        assert_eq!(s.snapshot().unwrap().at.x, 9.0);
        s.move_to(Some("method:A.f"), p(2.0, 3.0), t);
        assert_eq!(s.snapshot().map(|h| h.key.as_str()), Some("method:A.f"));
        assert_eq!(s.class_key().as_deref(), Some("A"));
        s.move_to(None, p(0.0, 0.0), t);
        t += 0.079;
        s.tick(t);
        assert!(s.snapshot().is_some());
        t += 0.0011;
        s.tick(t);
        assert!(s.snapshot().is_none());
    }

    #[test]
    fn tall_member_cards_fit_at_every_canvas_edge() {
        let b = Bounds { left: 300.0, top: 100.0, right: 900.0, bottom: 700.0 };
        for at in [p(300.0, 100.0), p(900.0, 100.0), p(300.0, 700.0), p(900.0, 700.0)] {
            let r = place_card(at, (360.0, 580.0), &b);
            assert!(r.top >= b.top + 8.0 && r.top + 580.0 <= b.bottom - 8.0);
            assert!(r.left >= b.left + 8.0 && r.left + 360.0 <= b.right - 8.0);
        }
        let b = Bounds { left: 0.0, top: 0.0, right: 250.0, bottom: 300.0 };
        let r = place_card(p(240.0, 290.0), (360.0, 580.0), &b);
        assert_eq!(r.max_width, 234.0);
        assert_eq!(r.max_height, 284.0);
    }

    #[test]
    fn clear_cancels_stale_opening() {
        let mut s = HoverStore::default();
        s.move_to(Some("class:A"), p(1.0, 1.0), 0.0);
        s.clear();
        s.tick(0.1);
        assert!(s.snapshot().is_none());
    }

    #[test]
    fn measured_card_fits_the_canvas_not_side_panes() {
        let pl = place_card(p(850.0, 700.0), (360.0, 1000.0), &Bounds { left: 420.0, top: 40.0, right: 900.0, bottom: 800.0 });
        assert!(pl.left >= 428.0);
        assert!(pl.left + 360.0 <= 892.0);
        assert_eq!(pl.max_height, 744.0);
        assert_eq!(pl.top, 48.0);
    }
}
