//! SVG `text` elements read as editable text (FreeKerf's own import; LaserGRBL
//! writes a "not supported" comment instead): the lines (`tspan`s that start a
//! line, as Inkscape writes them), the position, and the style of the text or of
//! its first `tspan` (inherited, `style` before presentation attributes):
//! `font-family`, `font-size`, `font-weight`, `font-style`, `text-decoration`,
//! `text-anchor`, `letter-spacing`; and a `textPath` with its `startOffset` and
//! `side`.

use freekerf_document::TextAlign;
use roxmltree::Node;

use super::color::inherited;

/// Where a text on a path starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum StartOffset {
    /// User units along the path.
    Length(f64),
    /// Percent of the path's length.
    Percent(f64),
}

/// A `textPath`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OnPath {
    /// Id of the path element (without `#`).
    pub id: String,
    pub start: StartOffset,
    /// `side="right"`: the other side, backwards.
    pub flip: bool,
}

/// What a `text` element says.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SvgText {
    pub lines: Vec<String>,
    /// Start of the first baseline, user units.
    pub x: f64,
    pub y: f64,
    /// Font size (the em), user units.
    pub size: f64,
    pub family: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub align: TextAlign,
    /// User units.
    pub letter_spacing: f64,
    pub path: Option<OnPath>,
}

/// CSS's medium font size, px.
const DEFAULT_SIZE: f64 = 16.0;

/// A length (`12`, `12px`, `9pt`, `3mm`, `1.5em`...) in user units; `em` and `%`
/// relative to `em`.
fn length(v: &str, em: f64) -> Option<f64> {
    let v = v.trim();
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(v.len());
    // an exponent marker followed by a unit letter (`1em`) is the unit
    let (num, unit) = if v[..split].ends_with(['e', 'E']) && v[split..].starts_with(['m', 'x']) {
        (&v[..split - 1], &v[split - 1..])
    } else {
        (&v[..split], &v[split..])
    };
    let n: f64 = num.parse().ok().filter(|n: &f64| n.is_finite())?;
    let k = match unit.trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => em,
        "ex" => em / 2.0,
        "%" => em / 100.0,
        _ => return None,
    };
    Some(n * k)
}

/// The first number of a list attribute (`x="10 20 30"`).
fn first_number(node: Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)?
        .split(|c: char| c == ',' || c.is_whitespace())
        .find(|s| !s.is_empty())
        .and_then(|s| length(s, DEFAULT_SIZE))
}

fn font_size(node: Node<'_, '_>) -> f64 {
    // relative sizes go up the tree
    let parent = node
        .parent()
        .filter(Node::is_element)
        .map_or(DEFAULT_SIZE, font_size);
    super::color::property(node, "font-size")
        .and_then(|v| match v.as_str() {
            "inherit" => None,
            "larger" => Some(parent * 1.2),
            "smaller" => Some(parent / 1.2),
            _ => length(&v, parent),
        })
        .filter(|v| *v > 0.0)
        .unwrap_or(parent)
}

/// Collapsed white space (`xml:space` default).
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space && !out.is_empty() {
        out.push(' ');
    }
    out
}

/// A `tspan` that starts a line: Inkscape's `sodipodi:role="line"`, or one with
/// its own `y` or a `dy`.
fn starts_line(n: Node<'_, '_>) -> bool {
    n.attributes().any(|a| a.name() == "role" && a.value() == "line")
        || n.attribute("y").is_some()
        || n.attribute("dy")
            .is_some_and(|v| length(v, DEFAULT_SIZE).is_some_and(|d| d != 0.0))
}

fn gather(node: Node<'_, '_>, lines: &mut Vec<String>) {
    for c in node.children() {
        if c.is_text() {
            if let Some(last) = lines.last_mut() {
                last.push_str(c.text().unwrap_or_default());
            }
        } else if c.is_element() && matches!(c.tag_name().name(), "tspan" | "textPath" | "a") {
            if c.tag_name().name() == "tspan"
                && starts_line(c)
                && lines.last().is_some_and(|l| !l.trim().is_empty())
            {
                lines.push(String::new());
            }
            gather(c, lines);
        }
    }
}

/// The element whose style is the text's: the first `tspan` with text, or the
/// `text` itself.
fn styled<'a, 'i>(e: Node<'a, 'i>) -> Node<'a, 'i> {
    e.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "tspan")
        .find(|n| {
            n.children()
                .any(|c| c.is_text() && !c.text().unwrap_or_default().trim().is_empty())
        })
        .unwrap_or(e)
}

fn href<'a>(n: Node<'a, '_>) -> Option<&'a str> {
    n.attributes()
        .find(|a| a.name() == "href")
        .map(|a| a.value().trim().trim_start_matches('#'))
}

/// Reads a `text` element (`None` when it has no text).
pub(super) fn read(e: Node<'_, '_>) -> Option<SvgText> {
    let mut lines = vec![String::new()];
    gather(e, &mut lines);
    let lines: Vec<String> = lines.iter().map(|l| collapse(l).trim().to_owned()).collect();
    if lines.iter().all(String::is_empty) {
        return None;
    }
    let style = styled(e);
    let prop = |name: &str| inherited(style, name).map(|v| v.to_ascii_lowercase());
    let size = font_size(style);
    let first_tspan = e
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "tspan");
    let pos = |name: &str| {
        first_number(e, name)
            .or_else(|| first_tspan.and_then(|t| first_number(t, name)))
            .unwrap_or(0.0)
    };
    let family = inherited(style, "font-family").and_then(|v| {
        v.split(',')
            .map(|f| f.trim().trim_matches(['\'', '"']).trim().to_owned())
            .find(|f| !f.is_empty())
    });
    let decoration = prop("text-decoration").unwrap_or_default();
    let path = e
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "textPath")
        .and_then(|tp| {
            let id = href(tp)?.to_owned();
            let start = tp.attribute("startOffset").map_or(StartOffset::Length(0.0), |v| {
                v.trim().strip_suffix('%').map_or_else(
                    || StartOffset::Length(length(v, size).unwrap_or(0.0)),
                    |p| StartOffset::Percent(p.trim().parse().unwrap_or(0.0)),
                )
            });
            Some(OnPath {
                id,
                start,
                flip: tp.attribute("side") == Some("right"),
            })
        });
    Some(SvgText {
        lines,
        x: pos("x"),
        y: pos("y"),
        size,
        family,
        bold: prop("font-weight")
            .is_some_and(|w| w == "bold" || w == "bolder" || w.parse::<u32>().is_ok_and(|n| n >= 600)),
        italic: prop("font-style").is_some_and(|s| s == "italic" || s == "oblique"),
        underline: decoration.contains("underline"),
        strikethrough: decoration.contains("line-through"),
        align: match prop("text-anchor").as_deref() {
            Some("middle") => TextAlign::Center,
            Some("end") => TextAlign::Right,
            _ => TextAlign::Left,
        },
        letter_spacing: prop("letter-spacing")
            .and_then(|v| length(&v, size))
            .unwrap_or(0.0),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(svg: &str) -> Option<SvgText> {
        let doc = roxmltree::Document::parse(svg).unwrap();
        let e = doc.descendants().find(|n| n.has_tag_name("text")).unwrap();
        read(e)
    }

    #[test]
    fn inkscape_lines_and_style() {
        let t = text(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
            <g style="font-family:'DejaVu Sans', sans-serif;font-size:4.2333px">
            <text x="10" y="20" style="font-weight:bold;text-anchor:middle">
              <tspan sodipodi:role="line" x="10" y="20" style="font-style:italic">Olá   mundo</tspan>
              <tspan sodipodi:role="line" x="10" y="25">linha 2</tspan>
            </text></g></svg>"#,
        )
        .unwrap();
        assert_eq!(t.lines, ["Olá mundo", "linha 2"]);
        assert_eq!((t.x, t.y), (10.0, 20.0));
        assert!((t.size - 4.2333).abs() < 1e-9);
        assert_eq!(t.family.as_deref(), Some("DejaVu Sans"));
        assert!(t.bold && t.italic && !t.underline);
        assert_eq!(t.align, TextAlign::Center);
        assert_eq!(t.path, None);
    }

    #[test]
    fn attributes_units_and_paths() {
        let t = text(
            r##"<svg><text x="1 2 3" y="4mm" font-size="12pt" font-weight="700" text-decoration="underline line-through"
                letter-spacing="0.5em" text-anchor="end"><textPath href="#c" startOffset="25%" side="right">round</textPath></text></svg>"##,
        )
        .unwrap();
        assert_eq!(t.lines, ["round"]);
        assert!((t.x - 1.0).abs() < 1e-9 && (t.y - 4.0 * 96.0 / 25.4).abs() < 1e-9);
        assert!((t.size - 16.0).abs() < 1e-9);
        assert!(t.bold && t.underline && t.strikethrough);
        assert!((t.letter_spacing - 8.0).abs() < 1e-9);
        assert_eq!(t.align, TextAlign::Right);
        assert_eq!(
            t.path,
            Some(OnPath {
                id: "c".into(),
                start: StartOffset::Percent(25.0),
                flip: true
            })
        );
        let x = text(
            r##"<svg xmlns:xlink="http://www.w3.org/1999/xlink"><text font-size="2em"><textPath xlink:href="#p" startOffset="3">a</textPath></text></svg>"##,
        )
        .unwrap();
        assert!((x.size - 32.0).abs() < 1e-9);
        assert_eq!(x.path.unwrap().start, StartOffset::Length(3.0));
        assert!(text("<svg><text>  </text></svg>").is_none());
        assert_eq!(length("1e1", 16.0), Some(10.0));
        assert_eq!(length("2ex", 16.0), Some(16.0));
        assert_eq!(length("3furlongs", 16.0), None);
        assert_eq!(
            text(r#"<svg><text font-size="smaller">a</text></svg>"#)
                .unwrap()
                .size,
            16.0 / 1.2
        );
    }
}
