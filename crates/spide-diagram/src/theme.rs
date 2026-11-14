//! Light "print" theme for exports: rewrite the dark canvas palette on paper.
//! Known canvas tokens map exactly; anything else keeps its hue: neutrals invert, colours darken for contrast on white.

type Rgbf = [f64; 3];

fn exact(k: &str) -> Option<&'static str> {
    Some(match k {
        "#1d1e22" => "#ffffff", // bg (also label plates, chip fills)
        "#25272d" => "#ffffff", // node
        "#2b2e35" => "#f2f4f8", // node header
        "#26282e" => "#eceff4", // grid
        "#4b5263" => "#8b93a5", // border
        "#e6e8ee" => "#1b2030", // text
        "#9aa1b1" => "#4b5365", // dim
        "#5d6474" => "#7b8395", // faint
        "#8a93a6" => "#5a6378", // edge
        "#24314f" => "#e8eefc", // C4 system fill
        "#30343d" => "#eef0f4",
        "#ffffff" => "#1b2030", // "white" accents on dark become ink
        _ => return None,
    })
}

pub fn parse_color(c: &str) -> Option<Rgbf> {
    let s = c.trim().to_lowercase();
    if let Some(h) = s.strip_prefix('#') {
        if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64;
            return Some([v(0), v(2), v(4)]);
        }
        if h.len() == 3 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).unwrap() as f64;
            return Some([v(0), v(1), v(2)]);
        }
        return None;
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?;
    let parts: Vec<f64> = inner.trim_end_matches(')').split(',').take(3).map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
}

fn hex(c: Rgbf) -> String {
    format!("#{}", c.iter().map(|v| format!("{:02x}", v.clamp(0.0, 255.0).round() as u8)).collect::<String>())
}

fn to_hsl([r, g, b]: Rgbf) -> [f64; 3] {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return [0.0, 0.0, l];
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, s, l]
}

fn from_hsl([h, s, l]: [f64; 3]) -> Rgbf {
    if s == 0.0 {
        return [l * 255.0; 3];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |t: f64| {
        let t = (t + 1.0) % 1.0;
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0) * 255.0, f(h) * 255.0, f(h - 1.0 / 3.0) * 255.0]
}

/// The paper equivalent of a canvas colour.
pub fn light_color(c: &str) -> String {
    let k = c.trim().to_lowercase();
    if let Some(e) = exact(&k) {
        return e.to_string();
    }
    let Some(rgb) = parse_color(&k) else { return c.to_string() }; // none, transparent, url(#…), currentColor
    let [h, s, l] = to_hsl(rgb);
    if s < 0.18 {
        return hex(from_hsl([h, s, (1.08 - l).clamp(0.12, 0.97)])); // neutrals: invert lightness
    }
    // colours: deepen until they read on white (≥ 3.4:1), keeping the hue
    let mut ll = l.min(0.5);
    let mut out = hex(from_hsl([h, s, ll]));
    while ll > 0.15 && 1.05 / (luminance(&out) + 0.05) < 3.4 {
        ll -= 0.02;
        out = hex(from_hsl([h, s, ll]));
    }
    out
}

/// Relative luminance (WCAG).
pub fn luminance(c: &str) -> f64 {
    let Some(rgb) = parse_color(c) else { return 1.0 };
    let v: Vec<f64> = rgb
        .iter()
        .map(|v| {
            let v = v / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contrast(a: &str, b: &str) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn canvas_neutrals_become_paper_and_ink() {
        for dark in ["#1d1e22", "#25272d", "#2b2e35", "#26282e", "#191b20", "rgb(30,31,36)"] {
            assert!(luminance(&light_color(dark)) > 0.7, "{dark} → {}", light_color(dark));
        }
        for text in ["#e6e8ee", "#9aa1b1"] {
            assert!(contrast(&light_color(text), "#ffffff") >= 4.5, "{text} readable on paper");
        }
    }

    #[test]
    fn semantic_colours_keep_hue_but_read_on_white() {
        for c in ["#ff5c6c", "#ffb454", "#5eead4", "#7aa2ff", "#4ade80", "#c084fc", "#f472b6", "#60a5fa", "#34d399", "rgb(255,180,84)"] {
            let l = light_color(c);
            assert!(contrast(&l, "#ffffff") >= 3.0, "{c} → {l} too faint");
            assert_ne!(l, c);
        }
    }

    #[test]
    fn non_colours_pass_through() {
        for v in ["none", "transparent", "url(#halo3)", "currentColor"] {
            assert_eq!(light_color(v), v);
        }
    }
}
