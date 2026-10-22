//! SVG import: shapes grouped by colour (stroke, else fill), as LaserGRBL's colour
//! layer import reads them (`SvgColorLayer`, `GCodeFromSVG`).

mod color;
mod convert;
mod known_colors;
mod text;

use freekerf_document::{Color, ObjectKind, TextSpec};
use freekerf_geometry::Matrix;

use crate::{ColorCount, Import, ImportError, ImportedObject};

pub use color::normalize as normalize_color;
pub use convert::to_pixel;

/// Options of the SVG import.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgOptions {
    /// Scale the drawing so its larger side is this many mm (LaserGRBL's centerline
    /// conversion); `None` keeps the file's size.
    pub scale_to_mm: Option<f32>,
    /// Added to every coordinate, mm.
    pub user_offset: (f32, f32),
    /// Approximate Béziers with 12 equal steps instead of the adaptive flattening
    /// (`Vector.UseSmartBezier` off).
    pub legacy_bezier: bool,
    /// Import `text` elements as editable text laid out with these fonts (a font
    /// that is not available becomes the default single-line font). `None` keeps
    /// LaserGRBL's behaviour: a "not supported" item.
    pub fonts: Option<freekerf_text::Fonts>,
}

/// One element (or part of one) of the file, in drawing order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgItem {
    pub(crate) color: Color,
    pub(crate) kind: ObjectKind,
    pub(crate) text: Option<TextSpec>,
    pub(crate) transform: Matrix,
    pub(crate) link: Option<usize>,
}

impl SvgItem {
    pub(crate) const fn new(color: Color, kind: ObjectKind) -> Self {
        Self {
            color,
            kind,
            text: None,
            transform: Matrix::IDENTITY,
            link: None,
        }
    }
}

/// The text of a file as `File.ReadAllText` decodes it: byte order marks for UTF-8,
/// UTF-16 and UTF-32, UTF-8 otherwise (invalid sequences replaced).
#[must_use]
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], be: bool| {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    let utf32 = |b: &[u8], be: bool| {
        b.chunks_exact(4)
            .map(|c| {
                let v = if be {
                    u32::from_be_bytes([c[0], c[1], c[2], c[3]])
                } else {
                    u32::from_le_bytes([c[0], c[1], c[2], c[3]])
                };
                char::from_u32(v).unwrap_or('\u{fffd}')
            })
            .collect::<String>()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE, 0x00, 0x00]) {
        utf32(rest, false)
    } else if let Some(rest) = bytes.strip_prefix(&[0x00, 0x00, 0xFE, 0xFF]) {
        utf32(rest, true)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, false)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, true)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// `RemoveInvalidUnicode`: drops what XML forbids (control characters, U+FFFE/F) and,
/// as LaserGRBL's pattern does, every character outside the basic plane.
fn remove_invalid(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            matches!(c, '\t' | '\n' | '\r')
                || ('\u{20}'..='\u{d7ff}').contains(&c)
                || ('\u{e000}'..='\u{fffd}').contains(&c)
        })
        .collect()
}

/// Elements LaserGRBL converts (text and image are not).
const CONVERTED: [&str; 7] = ["path", "rect", "circle", "ellipse", "line", "polyline", "polygon"];

/// `SvgColorLayer.Scan`: one entry per colour of the converted elements (and of
/// texts when they are imported), in order of first appearance (document order,
/// groups included).
fn scan(node: roxmltree::Node<'_, '_>, ns: Option<&str>, texts: bool, out: &mut Vec<ColorCount>) {
    for e in node.children().filter(roxmltree::Node::is_element) {
        let name = e.tag_name();
        if name.namespace() != ns {
            continue;
        }
        if name.name() == "g" {
            scan(e, ns, texts, out);
        } else if CONVERTED.contains(&name.name()) || (texts && name.name() == "text") {
            let c = color::resolve(e);
            match out.iter_mut().find(|x| x.color == c) {
                Some(x) => x.count += 1,
                None => out.push(ColorCount { color: c, count: 1 }),
            }
        }
    }
}

/// Imports an SVG file.
///
/// # Errors
/// Not XML, or a structure LaserGRBL cannot read either (a viewBox without four
/// space-separated numbers, groups nested more than 9 levels).
pub fn import(bytes: &[u8], options: &SvgOptions) -> Result<Import, ImportError> {
    let text = remove_invalid(&decode_text(bytes));
    let doc = roxmltree::Document::parse_with_options(
        &text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .map_err(|e| ImportError::Xml(e.to_string()))?;
    let root = doc.root_element();
    let mut colors = Vec::new();
    scan(
        root,
        root.tag_name().namespace(),
        options.fonts.is_some(),
        &mut colors,
    );
    let items = convert::Converter::new(options).run(root)?;
    Ok(Import {
        colors,
        objects: items
            .into_iter()
            .map(|i| ImportedObject {
                color: i.color,
                kind: i.kind,
                text: i.text,
                transform: i.transform,
                link: i.link,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use freekerf_document::TextAlign;

    use super::*;

    fn fonts() -> SvgOptions {
        SvgOptions {
            fonts: Some(freekerf_text::Fonts::builtin()),
            ..SvgOptions::default()
        }
    }

    fn bounds(kind: &ObjectKind) -> freekerf_geometry::Rect {
        let mut r = freekerf_geometry::Rect::EMPTY;
        if let ObjectKind::Shape { paths } = kind {
            for p in paths {
                r.union(&p.bounds());
            }
        }
        r
    }

    #[test]
    fn texts_become_editable_text() {
        // 100 × 100 mm, user units are mm; Inkscape writes lines as tspans
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
            width="100mm" height="100mm" viewBox="0 0 100 100">
            <text x="10" y="30" style="font-size:10px;font-family:'Hershey Serif';font-weight:bold;fill:#ff0000">
              <tspan sodipodi:role="line" x="10" y="30">Olá</tspan><tspan sodipodi:role="line" x="10" y="45">ç</tspan></text>
            <text x="50" y="90" font-size="5" text-anchor="middle" font-family="Unknown Sans">fim</text></svg>"##
            .as_bytes();
        let imp = import(svg, &fonts()).unwrap();
        assert_eq!(imp.colors[0].color.to_string(), "#FF0000");
        let t = &imp.objects[0];
        let spec = t.text.as_ref().unwrap();
        assert_eq!(spec.text, "Olá\nç");
        assert_eq!(spec.font, "Hershey Serif");
        assert!(spec.bold);
        // a 10 mm font: Hershey Serif's capitals are 0.662 em, the baseline 70 mm up
        assert!((spec.height_mm - 6.62).abs() < 1e-4, "{}", spec.height_mm);
        let b = bounds(&t.kind);
        assert!(
            b.min_x > 9.0 && b.min_x < 12.0 && b.max_y > 74.0 && b.max_y < 78.0,
            "{b:?}"
        );
        assert!((t.transform.offset_x - 10.0).abs() < 1e-4 && (t.transform.offset_y - 70.0).abs() < 1e-4);
        // an unknown font becomes the default one, centred on its x
        let end = imp.objects[1].text.as_ref().unwrap();
        assert_eq!(end.font, freekerf_text::DEFAULT_FONT);
        assert_eq!(end.align, TextAlign::Center);
        let e = bounds(&imp.objects[1].kind);
        assert!(((e.min_x + e.max_x) / 2.0 - 50.0).abs() < 1.0, "{e:?}");
        // without fonts, as LaserGRBL: not supported
        let plain = import(svg, &SvgOptions::default()).unwrap();
        assert!(matches!(plain.objects[0].kind, ObjectKind::Unsupported { .. }));
        assert!(plain.objects[0].text.is_none());
    }

    #[test]
    fn text_paths_follow_their_path() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
            width="100mm" height="100mm" viewBox="0 0 100 100">
            <defs><path id="hidden" d="M0 50 L100 50"/></defs>
            <path id="arc" d="M10 80 L90 80" stroke="#00f"/>
            <text font-size="8"><textPath xlink:href="#arc" startOffset="50%">on</textPath></text>
            <text font-size="8"><textPath href="#hidden">off</textPath></text>
            <text font-size="8"><textPath href="#missing">none</textPath></text></svg>"##;
        let imp = import(svg, &fonts()).unwrap();
        // basic elements (texts) first, then paths
        let on = &imp.objects[0];
        assert_eq!(on.link, Some(3));
        let tp = on.text.as_ref().unwrap().path.as_ref().unwrap();
        // single precision, as LaserGRBL computes the geometry
        assert!((tp.offset_mm - 40.0).abs() < 1e-4, "half of 80 mm: {tp:?}");
        let b = bounds(&on.kind);
        assert!(b.min_x > 49.0 && b.min_y > 19.0 && b.max_y < 27.0, "{b:?}");
        // a path in defs: a copy, no link
        let off = &imp.objects[1];
        assert_eq!(off.link, None);
        assert!(off.text.as_ref().unwrap().path.is_some());
        assert!(bounds(&off.kind).min_y > 49.0);
        // a missing path: straight text
        assert!(imp.objects[2].text.as_ref().unwrap().path.is_none());
        // into a document: the link becomes the path object's id
        let doc = imp.into_document_with_defaults(1000, 1000, None);
        let text = doc
            .objects
            .iter()
            .find(|o| o.text.as_ref().is_some_and(|t| t.text == "on"))
            .unwrap();
        let path = doc.objects.iter().find(|o| o.text.is_none()).unwrap();
        assert_eq!(text.text.as_ref().unwrap().linked_path(), Some(path.id));
    }

    #[test]
    fn decoding_and_cleaning() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(decode_text(&[0xFF, 0xFE, b'a', 0]), "a");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0, b'a']), "a");
        assert_eq!(decode_text(&[0xFF, 0xFE, 0, 0, b'a', 0, 0, 0]), "a");
        assert_eq!(decode_text(&[0, 0, 0xFE, 0xFF, 0, 0, 0, b'a']), "a");
        assert_eq!(decode_text(b"a\xFFb"), "a\u{fffd}b");
        assert_eq!(remove_invalid("a\u{1}b\u{1F600}c\u{fffe}\td"), "abc\td");
    }

    #[test]
    fn imports_shapes_by_colour() {
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20">
            <rect x="1" y="1" width="5" height="5" stroke="red" fill="none"/>
            <g fill="#00f"><circle cx="10" cy="10" r="2"/><text>hi</text></g>
            <path d="M1 1 L5 5"/></svg>"##;
        let imp = import(svg, &SvgOptions::default()).unwrap();
        let colors: Vec<String> = imp
            .colors
            .iter()
            .map(|c| format!("{}x{}", c.color, c.count))
            .collect();
        assert_eq!(colors, ["#FF0000x1", "#0000FFx1", "#000000x1"]);
        // traversal order: root basic shapes, root paths, then groups
        assert_eq!(imp.objects.len(), 4);
        assert!(matches!(imp.objects[2].kind, ObjectKind::Shape { .. }));
        assert!(matches!(imp.objects[3].kind, ObjectKind::Unsupported { .. }));
        assert!(import(b"<svg", &SvgOptions::default()).is_err());
        // with fonts the text is editable text, on its own colour layer
        let with = SvgOptions {
            fonts: Some(freekerf_text::Fonts::builtin()),
            ..SvgOptions::default()
        };
        let imp = import(svg, &with).unwrap();
        assert_eq!(imp.objects[3].text.as_ref().unwrap().text, "hi");
        assert!(matches!(&imp.objects[3].kind, ObjectKind::Shape { paths } if !paths.is_empty()));
        assert!(import(br#"<svg viewBox="0,0,1,1"/>"#, &SvgOptions::default()).is_err());
    }
}
