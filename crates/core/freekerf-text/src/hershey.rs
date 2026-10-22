//! Hershey text: labels of the test generators (`Hershey.CreateString`), and the
//! strokes of [`crate::LEGACY_FONT`] texts. Each
//! character is a list of G-code fragments with coordinates relative to the glyph;
//! they are offset to their place and written with `double.ToString()` (15
//! significant digits, so floating point noise such as `Y0.00299999999999989` is
//! part of the output, FINDINGS F-40).

use freekerf_gcode::dotnet::double_to_string;

use crate::hershey_table::{HORIZONTAL, VERTICAL};

/// Space between characters, mm.
pub(crate) const SPACE_BETWEEN_LETTERS: f64 = 0.3;

/// The `[XY]-?\d+(\.\d+)?` tokens of a fragment: (start, end) byte offsets.
fn coordinates(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'X' || b[i] == b'Y' {
            let mut j = i + 1;
            if j < b.len() && b[j] == b'-' {
                j += 1;
            }
            let digits = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > digits {
                if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
                    j += 1;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                out.push((i, j));
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `ApplyOffset`: the fragments moved by the offsets, and the size of the glyph.
fn apply_offset(glyph: &[&str], dx: f64, dy: f64) -> (Vec<String>, f64, f64) {
    let (mut max_x, mut min_x, mut max_y, mut min_y) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut out = Vec::with_capacity(glyph.len());
    for part in glyph {
        let mut updated = (*part).to_owned();
        for (a, b) in coordinates(part) {
            let coord = &part[a..b];
            let axis = &coord[..1];
            let mut value: f64 = coord[1..].parse().unwrap_or(0.0);
            if axis == "X" {
                max_x = max_x.max(value);
                min_x = min_x.min(value);
                value += dx;
            } else {
                max_y = max_y.max(value);
                min_y = min_y.min(value);
                value += dy;
            }
            updated = updated.replace(coord, &format!("{axis}{}", double_to_string(value)));
        }
        out.push(updated);
    }
    (out, max_x - min_x, max_y - min_y)
}

/// The glyph of a UTF-16 code unit (C# iterates `string.ToCharArray()`).
fn glyph(c: u16, horizontal: bool) -> Option<&'static [&'static str]> {
    if !(32..=126).contains(&c) {
        return None;
    }
    let i = usize::from(c - 32);
    Some(if horizontal { HORIZONTAL[i] } else { VERTICAL[i] })
}

/// `MeasureString`: width and height of the text.
fn measure(text: &str, horizontal: bool) -> (f64, f64) {
    let (mut size_x, mut size_y) = (0.0f64, 0.0f64);
    let chars: Vec<u16> = text.encode_utf16().collect();
    for (i, &c) in chars.iter().enumerate() {
        if let Some(g) = glyph(c, horizontal) {
            let (_, w, h) = apply_offset(g, 0.0, 0.0);
            let not_last = i + 1 < chars.len();
            if horizontal {
                size_x += w;
                size_y = size_y.max(h);
                if not_last {
                    size_x += SPACE_BETWEEN_LETTERS;
                }
            } else {
                size_y += h;
                size_x = size_x.max(w);
                if not_last {
                    size_y += SPACE_BETWEEN_LETTERS;
                }
            }
        }
    }
    (size_x, size_y)
}

/// `Hershey.CreateString`: `text` centred on `(cx, cy)` (characters outside
/// printable ASCII are skipped), horizontal or vertical, engraved at `speed` and
/// `power` with `laser_on` (it replaces the glyphs' `M3`). One string per command.
#[must_use]
pub fn text(
    text: &str,
    cx: f64,
    cy: f64,
    speed: i64,
    power: i64,
    horizontal: bool,
    laser_on: &str,
) -> Vec<String> {
    let (mut size_x, mut size_y) = measure(text, horizontal);
    if horizontal {
        size_y = 2.0;
    } else {
        size_x = 2.0;
    }
    let mut rv = vec![format!("M5 S{power} F{speed}")]; // set power and speed (off)
    let mut ox = cx - size_x / 2.0;
    let mut oy = cy - size_y / 2.0;
    for c in text.encode_utf16() {
        let Some(g) = glyph(c, horizontal) else {
            continue;
        };
        let (parts, w, h) = apply_offset(g, ox, oy);
        rv.extend(parts.into_iter().map(|p| p.replace("M3", laser_on)));
        rv.push("M5".into()); // turn off after the char
        if horizontal {
            ox += w + SPACE_BETWEEN_LETTERS;
        } else {
            oy += h + SPACE_BETWEEN_LETTERS;
        }
    }
    rv
}

/// Height of the glyphs of the horizontal font, in font units (`text` uses 2).
pub const GLYPH_HEIGHT: f64 = 2.0;

/// The strokes of one character of the horizontal font, font units, and its width
/// (nothing for characters outside printable ASCII).
pub(crate) fn glyph_strokes(c: char) -> (Vec<Vec<(f64, f64)>>, f64) {
    let Some(g) = u16::try_from(u32::from(c)).ok().and_then(|c| glyph(c, true)) else {
        return (Vec::new(), 0.0);
    };
    let (_, w, _) = apply_offset(g, 0.0, 0.0);
    let (mut px, mut py) = (0.0f64, 0.0f64);
    let mut on = false;
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    for part in g {
        if part.starts_with("M3") {
            on = true;
            cur = vec![(px, py)];
            continue;
        }
        if part.starts_with("M5") {
            on = false;
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        for (a, b) in coordinates(part) {
            let v: f64 = part[a + 1..b].parse().unwrap_or(0.0);
            if &part[a..=a] == "X" {
                px = v;
            } else {
                py = v;
            }
        }
        if on {
            cur.push((px, py));
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    (out, w)
}

/// The strokes of `text` as polylines in mm: the bottom-left corner of the text at
/// `(x, y)`, glyphs scaled so the font's height is `height_mm`, Y up (characters
/// outside printable ASCII are skipped).
#[must_use]
pub fn text_polylines(text: &str, x: f64, y: f64, height_mm: f64) -> Vec<Vec<(f64, f64)>> {
    let k = height_mm / GLYPH_HEIGHT;
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut ox = 0.0;
    for c in text.chars() {
        if !(' '..='~').contains(&c) {
            continue;
        }
        let (strokes, w) = glyph_strokes(c);
        out.extend(strokes.into_iter().map(|s| {
            s.into_iter()
                .map(|(px, py)| ((ox + px) * k + x, py * k + y))
                .collect()
        }));
        ox += w + SPACE_BETWEEN_LETTERS;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_as_polylines() {
        let p = text_polylines("I", 10.0, 5.0, 10.0);
        assert!(!p.is_empty());
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in p.iter().flatten() {
            (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
        }
        assert!(x0 >= 10.0 - 1e-9 && y0 >= 5.0 - 1e-9, "{x0} {y0}");
        assert!(y1 - 5.0 <= 10.5 && y1 - y0 > 5.0, "{y0}..{y1}");
        assert!((x1 - x0).abs() < 1e-9, "\"I\" is one vertical stroke");
        let ab = text_polylines("AB", 0.0, 0.0, 10.0);
        assert!(ab.iter().flatten().map(|p| p.0).fold(0.0, f64::max) > 5.0);
        // wider text, more strokes; nothing for spaces or other characters
        assert!(text_polylines("II", 0.0, 0.0, 10.0).len() > p.len());
        assert!(text_polylines(" é", 0.0, 0.0, 10.0).is_empty());
    }

    #[test]
    fn tokens_and_offsets() {
        assert_eq!(coordinates("G0X0.056Y1.845"), vec![(2, 8), (8, 14)]);
        assert_eq!(coordinates("G1X-1Y2."), vec![(2, 5), (5, 7)]);
        assert!(coordinates("M3").is_empty());
        let (parts, w, h) = apply_offset(&["G0X0.4Y-0.5", "Y1"], 1.0, 0.25);
        assert_eq!(parts, vec!["G0X1.4Y-0.25", "Y1.25"]);
        assert!((w - 0.4).abs() < 1e-12 && (h - 1.5).abs() < 1e-12);
    }

    #[test]
    fn text_is_centred_and_skips_other_characters() {
        let t = text("A", 10.0, 10.0, 1000, 500, true, "M4");
        assert_eq!(t[0], "M5 S500 F1000");
        assert_eq!(t.last().unwrap(), "M5");
        assert!(t.iter().any(|l| l == "M4"));
        assert!(t.iter().all(|l| !l.contains("M3")));
        let v = text("é1", 0.0, 0.0, 1, 2, false, "M3");
        assert_eq!(v, text("1", 0.0, 0.0, 1, 2, false, "M3"));
        assert_eq!(text("", 0.0, 0.0, 1, 2, true, "M3"), vec!["M5 S2 F1"]);
        assert_eq!(glyph(0x7f, true), None);
    }
}
