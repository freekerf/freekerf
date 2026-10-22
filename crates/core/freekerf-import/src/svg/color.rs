//! The colour that groups an SVG element in a layer (`SvgColorLayer.ResolveColor`):
//! the stroke if it has one, otherwise the fill, inherited from parent groups, black
//! by default. Colour values are read like LaserGRBL reads them on Mono
//! (`ColorTranslator.FromHtml`): named colours (case-insensitive, system colours
//! included), `#RGB`, `#RRGGBB` (and the odd hex lengths .NET accepts), `rgb()`/`rgba()`
//! with numbers or percentages, plain decimal ARGB numbers and `r,g,b` lists.

use freekerf_document::Color;
use roxmltree::Node;

use super::known_colors::KNOWN_COLORS;
use crate::netparse::parse_double;

/// `GetProperty`: the value of `name` in the `style` attribute (first declaration,
/// `!important` removed), else the presentation attribute.
pub(super) fn property(node: Node<'_, '_>, name: &str) -> Option<String> {
    if let Some(style) = node.attribute("style") {
        for decl in style.split(';') {
            if let Some(sep) = decl.find(':')
                && sep > 0
                && decl[..sep].trim() == name
            {
                return Some(decl[sep + 1..].replace("!important", "").trim().to_owned());
            }
        }
    }
    node.attribute(name).map(|v| v.trim().to_owned())
}

/// `GetInheritedProperty`: the property on the element or its nearest ancestor that
/// sets it to something other than `inherit`.
pub(super) fn inherited(node: Node<'_, '_>, name: &str) -> Option<String> {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.is_element()
            && let Some(v) = property(n, name)
            && v != "inherit"
        {
            return Some(v);
        }
        current = n.parent().filter(Node::is_element);
    }
    None
}

/// `ResolveColor`: stroke, else fill, else black.
#[must_use]
pub fn resolve(node: Node<'_, '_>) -> Color {
    inherited(node, "stroke")
        .and_then(|v| normalize(&v))
        .or_else(|| inherited(node, "fill").and_then(|v| normalize(&v)))
        .unwrap_or(Color::BLACK)
}

/// `Math.Round` (to even) of an `rgb()` component, clamped to 0..255.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn component(number: &str, percent: bool) -> Option<u8> {
    let mut v = parse_double(number)?;
    if percent {
        v = v * 255.0 / 100.0;
    }
    Some(v.round_ties_even().clamp(0.0, 255.0) as u8)
}

/// The `^rgba?\(\s*([\d.]+)(%?)\s*[,\s]\s*…` function of LaserGRBL (case-insensitive,
/// only the beginning has to match). `None` when it does not match; `Some(None)` when
/// it matches but a number cannot be read (an exception in LaserGRBL).
fn rgb_function(value: &str) -> Option<Option<Color>> {
    let lower = value.to_ascii_lowercase();
    let rest = lower.strip_prefix("rgba").or_else(|| lower.strip_prefix("rgb"))?;
    let mut s = rest.strip_prefix('(')?;
    let mut parts = Vec::new();
    for i in 0..3 {
        s = s.trim_start_matches(char::is_whitespace);
        let end = s
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(s.len());
        if end == 0 {
            return None;
        }
        let number = &s[..end];
        s = &s[end..];
        let percent = s.starts_with('%');
        if percent {
            s = &s[1..];
        }
        parts.push((number.to_owned(), percent));
        if i < 2 {
            // `\s*[,\s]\s*`: one comma between optional white space, or white space only
            let t = s.trim_start_matches(char::is_whitespace);
            if let Some(after) = t.strip_prefix(',') {
                s = after;
            } else if t.len() < s.len() {
                s = t;
            } else {
                return None;
            }
        }
    }
    let mut rgb = [0u8; 3];
    for (k, (n, p)) in parts.iter().enumerate() {
        match component(n, *p) {
            Some(v) => rgb[k] = v,
            None => return Some(None),
        }
    }
    Some(Some(Color::rgb(rgb[0], rgb[1], rgb[2])))
}

/// `ColorTranslator.FromHtml` + `ColorConverter` on Mono: `None` for the empty
/// colour and for values that throw.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn from_html(html: &str) -> Option<Color> {
    let argb = |v: u32| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    if html.is_empty() {
        return None;
    }
    let special = match html.to_lowercase().as_str() {
        "buttonface" | "threedface" => Some("control"),
        "buttonhighlight" | "threedlightshadow" => Some("controllightlight"),
        "buttonshadow" => Some("controldark"),
        "captiontext" => Some("activecaptiontext"),
        "threeddarkshadow" => Some("controldarkdark"),
        "threedhighlight" => Some("controllight"),
        "background" => Some("desktop"),
        "buttontext" => Some("controltext"),
        "infobackground" => Some("info"),
        "lightgrey" => Some("lightgray"),
        _ => None,
    };
    let mut s = html.to_owned();
    if let Some(name) = special {
        s = name.to_owned();
    } else if s.starts_with('#') && s.chars().count() == 4 {
        let c: Vec<char> = s.chars().collect();
        s = format!("#{0}{0}{1}{1}{2}{2}", c[1], c[2], c[3]);
    }
    // ColorConverter.ConvertFromString
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_lowercase();
    if let Ok(i) = KNOWN_COLORS.binary_search_by(|(n, _)| (*n).cmp(lower.as_str())) {
        return Some(argb(KNOWN_COLORS[i].1));
    }
    if !s.contains(',') {
        let sharp = s.starts_with('#');
        let mut start = usize::from(sharp);
        let b = s.as_bytes();
        let mut hex = false;
        if b.len() > start + 1 && b[start] == b'0' {
            hex = b[start + 1] == b'x' || b[start + 1] == b'X';
            if hex {
                start += 2;
            }
        }
        if sharp || hex {
            let digits = s[start..].trim();
            if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None; // FormatException / OverflowException
            }
            let v = u32::from_str_radix(digits, 16).ok()?;
            return Some(argb(v));
        }
    }
    // Int32Converter on each comma-separated component
    let parts: Vec<&str> = s.split(',').collect();
    let mut n = Vec::with_capacity(parts.len());
    for p in &parts {
        let t = p.trim();
        let v: i64 = t.strip_prefix('+').unwrap_or(t).parse().ok()?;
        n.push(i32::try_from(v).ok()?);
    }
    let byte = |v: i32| u8::try_from(v).ok();
    match n.as_slice() {
        [v] => Some(argb(*v as u32)),
        [r, g, b] => Some(Color::rgb(byte(*r)?, byte(*g)?, byte(*b)?)),
        [a, r, g, b] => {
            byte(*a)?;
            Some(Color::rgb(byte(*r)?, byte(*g)?, byte(*b)?))
        }
        _ => None,
    }
}

/// `NormalizeColor`: a colour value as `#RRGGBB`, `None` for `none`, gradients
/// (`url(...)`), `currentColor` and values that cannot be read.
#[must_use]
pub fn normalize(value: &str) -> Option<Color> {
    if value.is_empty() || value == "none" || value.starts_with("url(") || value == "currentColor" {
        return None;
    }
    if let Some(rgb) = rgb_function(value) {
        return rgb;
    }
    from_html(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Option<String> {
        normalize(s).map(|c| c.to_string())
    }

    #[test]
    fn html_colours_as_mono_reads_them() {
        assert_eq!(n("red").as_deref(), Some("#FF0000"));
        assert_eq!(n(" RED ").as_deref(), Some("#FF0000"));
        assert_eq!(n("LightGrey").as_deref(), Some("#D3D3D3"));
        assert_eq!(n("darkseagreen").as_deref(), Some("#8FBC8B"));
        assert_eq!(n("threedface").as_deref(), Some("#ECE9D8"));
        assert_eq!(n("transparent").as_deref(), Some("#FFFFFF"));
        assert_eq!(n("#00f").as_deref(), Some("#0000FF"));
        assert_eq!(n("#12").as_deref(), Some("#000012"));
        assert_eq!(n("#12345678").as_deref(), Some("#345678"));
        assert_eq!(n("0x112233").as_deref(), Some("#112233"));
        assert_eq!(n("#0x112233").as_deref(), Some("#112233"));
        assert_eq!(n("255").as_deref(), Some("#0000FF"));
        assert_eq!(n("255,0,0").as_deref(), Some("#FF0000"));
        assert_eq!(n("1,255,0,0").as_deref(), Some("#FF0000"));
        for bad in [
            "notacolor",
            "#GGHHII",
            "#123456789",
            "1,2",
            "300,0,0",
            "",
            "none",
            "url(#g)",
            "currentColor",
        ] {
            assert_eq!(n(bad), None, "{bad}");
        }
    }

    #[test]
    fn rgb_functions() {
        assert_eq!(n("rgb(0, 0, 255)").as_deref(), Some("#0000FF"));
        assert_eq!(n("RGBA(0%,0%,100%,0.5)").as_deref(), Some("#0000FF"));
        assert_eq!(n("rgb(10 20 30)").as_deref(), Some("#0A141E"));
        assert_eq!(n("rgb(10 , 20,30)").as_deref(), Some("#0A141E"));
        assert_eq!(n("rgb(10,,20,30)"), None);
        assert_eq!(n("rgb(300,-1,0)"), None); // "-1" does not match, then FromHtml throws
        assert_eq!(n("rgb(1.2.3,0,0)"), None);
        assert_eq!(n("rgb(127.5,0,0)").as_deref(), Some("#800000"));
        assert_eq!(n("rgb(126.5,0,0)").as_deref(), Some("#7E0000"));
    }

    #[test]
    fn inheritance_and_fallbacks() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <g stroke="#123456" fill="red"><g stroke="inherit"><circle id="a"/></g>
            <rect id="b" style="stroke : none ; fill:#00ff00 !important"/><path id="c" stroke="url(#x)" fill="none"/></g>
            <line id="d"/></svg>"##;
        let doc = roxmltree::Document::parse(svg).unwrap();
        let get = |id: &str| doc.descendants().find(|n| n.attribute("id") == Some(id)).unwrap();
        assert_eq!(resolve(get("a")).to_string(), "#123456");
        assert_eq!(resolve(get("b")).to_string(), "#00FF00");
        assert_eq!(resolve(get("c")).to_string(), "#000000");
        assert_eq!(resolve(get("d")).to_string(), "#000000");
    }
}
