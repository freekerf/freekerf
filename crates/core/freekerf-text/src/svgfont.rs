//! Single-line fonts: SVG fonts (`<font>` with `<glyph d=...>`, the format of
//! Inkscape's Hershey Text and of Relief SingleLine) and LaserGRBL's Hershey table.
//! Their glyphs are open strokes, drawn once by the laser.

use std::collections::HashMap;

use freekerf_geometry::{Path, Point};

use crate::face::{Metrics, ShapedGlyph};
use crate::{TextError, hershey, svgpath};

#[derive(Debug, Clone)]
struct Glyph {
    paths: Vec<Path>,
    advance: f64,
}

/// A single-line font.
#[derive(Debug, Clone)]
pub struct StrokeFont {
    family: String,
    metrics: Metrics,
    glyphs: Vec<Glyph>,
    /// Glyphs by their first character: (the whole sequence, glyph), longest first
    /// (ligatures are glyphs of several characters).
    by_char: HashMap<char, Vec<(String, usize)>>,
    /// Kerning: how much less to advance between two glyphs, font units.
    kern: HashMap<(usize, usize), f64>,
    missing_advance: f64,
}

fn number(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

/// `u1`/`u2` of `hkern`: characters and `U+XXXX` code points, comma separated.
fn unicode_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            s.strip_prefix("U+")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32)
                .map_or_else(
                    || (!s.starts_with("U+")).then(|| s.to_owned()),
                    |c| Some(c.to_string()),
                )
        })
        .collect()
}

impl StrokeFont {
    /// Reads an SVG font (the first `<font>` element of the document).
    ///
    /// # Errors
    /// Not XML, or no `<font>` with glyphs.
    pub fn parse_svg(text: &str) -> Result<Self, TextError> {
        let doc = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..roxmltree::ParsingOptions::default()
            },
        )
        .map_err(|e| TextError::Font(format!("not an SVG font: {e}")))?;
        let font = doc
            .descendants()
            .find(|n| n.has_tag_name("font"))
            .ok_or_else(|| TextError::Font("no <font> element".into()))?;
        let face = font.children().find(|n| n.has_tag_name("font-face"));
        let attr = |name: &str| face.and_then(|f| number(f, name));
        let units_per_em = attr("units-per-em").filter(|v| *v > 0.0).unwrap_or(1000.0);
        let ascent = attr("ascent").unwrap_or(units_per_em * 0.8);
        let descent = attr("descent").unwrap_or(-units_per_em * 0.2);
        let default_advance = number(font, "horiz-adv-x").unwrap_or(units_per_em / 2.0);
        let mut f = Self {
            family: face
                .and_then(|f| f.attribute("font-family"))
                .or_else(|| font.attribute("id"))
                .unwrap_or_default()
                .to_owned(),
            metrics: Metrics::default(),
            glyphs: Vec::new(),
            by_char: HashMap::new(),
            kern: HashMap::new(),
            missing_advance: font
                .children()
                .find(|n| n.has_tag_name("missing-glyph"))
                .and_then(|n| number(n, "horiz-adv-x"))
                .unwrap_or(default_advance),
        };
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        for g in font.children().filter(|n| n.has_tag_name("glyph")) {
            let Some(unicode) = g.attribute("unicode").filter(|u| !u.is_empty()) else {
                continue;
            };
            let index = f.glyphs.len();
            f.glyphs.push(Glyph {
                paths: g.attribute("d").map(svgpath::parse).unwrap_or_default(),
                advance: number(g, "horiz-adv-x").unwrap_or(default_advance),
            });
            if let Some(name) = g.attribute("glyph-name") {
                by_name.entry(name).or_insert(index);
            }
            if let Some(first) = unicode.chars().next() {
                f.by_char
                    .entry(first)
                    .or_default()
                    .push((unicode.to_owned(), index));
            }
        }
        if f.glyphs.is_empty() {
            return Err(TextError::Font("the SVG font has no glyphs".into()));
        }
        for list in f.by_char.values_mut() {
            // longest sequence first; the first glyph of a sequence wins
            list.sort_by_key(|g| std::cmp::Reverse(g.0.chars().count()));
            list.dedup_by(|a, b| a.0 == b.0);
        }
        let glyphs_of = |f: &Self, names: Option<&str>, unicodes: Option<&str>| -> Vec<usize> {
            let mut out: Vec<usize> = names
                .map(|n| {
                    n.split(',')
                        .filter_map(|n| by_name.get(n.trim()).copied())
                        .collect()
                })
                .unwrap_or_default();
            for u in unicodes.map(unicode_list).unwrap_or_default() {
                let first = u.chars().next();
                if let Some(list) = first.and_then(|c| f.by_char.get(&c))
                    && let Some((_, i)) = list.iter().find(|(s, _)| *s == u)
                {
                    out.push(*i);
                }
            }
            out
        };
        for k in font.children().filter(|n| n.has_tag_name("hkern")) {
            let Some(amount) = number(k, "k") else { continue };
            let left = glyphs_of(&f, k.attribute("g1"), k.attribute("u1"));
            let right = glyphs_of(&f, k.attribute("g2"), k.attribute("u2"));
            for &a in &left {
                for &b in &right {
                    f.kern.insert((a, b), amount);
                }
            }
        }
        // the height of the capitals: measured on `H` (the Hershey and EMS files
        // say 500 units, their capitals are taller), else the font's
        let measured = |f: &Self, c: char| {
            f.by_char
                .get(&c)
                .and_then(|l| l.iter().find(|(s, _)| s.chars().count() == 1))
                .map(|(_, i)| {
                    let mut r = freekerf_geometry::Rect::EMPTY;
                    for p in &f.glyphs[*i].paths {
                        r.union(&p.bounds());
                    }
                    r.max_y
                })
                .filter(|h| h.is_finite() && *h > 0.0)
        };
        let cap_height = measured(&f, 'H')
            .or_else(|| attr("cap-height").filter(|v| *v > 0.0))
            .unwrap_or(units_per_em * 0.7);
        let x_height = attr("x-height")
            .filter(|v| *v > 0.0)
            .or_else(|| measured(&f, 'x'))
            .unwrap_or(cap_height * 0.65);
        f.metrics = Metrics {
            units_per_em,
            ascent,
            descent,
            line_gap: 0.0,
            cap_height,
            drawn_cap_height: cap_height,
            x_height,
            underline_position: attr("underline-position").unwrap_or(-units_per_em * 0.12),
            underline_thickness: attr("underline-thickness").unwrap_or(units_per_em * 0.05),
            strikeout_position: x_height / 2.0,
            strikeout_thickness: attr("underline-thickness").unwrap_or(units_per_em * 0.05),
        };
        Ok(f)
    }

    /// LaserGRBL's Hershey font (the one of the test generators), so texts written
    /// before fonts could be chosen keep their exact strokes: 2 units high, glyphs
    /// as wide as their strokes, 0.3 units between them.
    #[must_use]
    pub fn lasergrbl_hershey() -> Self {
        let mut f = Self {
            family: freekerf_document::LEGACY_FONT.to_owned(),
            metrics: Metrics {
                units_per_em: 3.0,
                ascent: 2.2,
                descent: -0.8,
                line_gap: 0.0,
                cap_height: hershey::GLYPH_HEIGHT,
                drawn_cap_height: hershey::GLYPH_HEIGHT,
                x_height: 1.4,
                underline_position: -0.4,
                underline_thickness: 0.1,
                strikeout_position: 0.8,
                strikeout_thickness: 0.1,
            },
            glyphs: Vec::new(),
            by_char: HashMap::new(),
            kern: HashMap::new(),
            missing_advance: 1.0,
        };
        for code in 32u8..=126 {
            let c = char::from(code);
            let (strokes, width) = hershey::glyph_strokes(c);
            let paths = strokes
                .into_iter()
                .map(|pts| {
                    let pts: Vec<Point> = pts.into_iter().map(|(x, y)| Point::new(x, y)).collect();
                    Path::polyline(&pts, false)
                })
                .collect();
            f.by_char.insert(c, vec![(c.to_string(), f.glyphs.len())]);
            f.glyphs.push(Glyph {
                paths,
                advance: width + hershey::SPACE_BETWEEN_LETTERS,
            });
        }
        // the strokes are kept at 2 units for the height (older texts), but its
        // capitals are lower: letters from another font are sized on these
        let mut r = freekerf_geometry::Rect::EMPTY;
        let h = f
            .by_char
            .get(&'H')
            .and_then(|l| l.first())
            .and_then(|(_, i)| f.glyphs.get(*i));
        for p in h.map(|g| g.paths.as_slice()).unwrap_or_default() {
            r.union(&p.bounds());
        }
        if r.max_y > 0.0 {
            f.metrics.drawn_cap_height = r.max_y;
        }
        f
    }

    /// The family name the file gives.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    pub(crate) const fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Whether the font has a glyph for `c`.
    #[must_use]
    pub fn covers(&self, c: char) -> bool {
        self.by_char.contains_key(&c)
    }

    /// The glyphs of one line of text, font units, kerned.
    pub(crate) fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        let mut out: Vec<ShapedGlyph> = Vec::new();
        let mut prev: Option<usize> = None;
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            let matched = self
                .by_char
                .get(&c)
                .and_then(|list| list.iter().find(|(s, _)| rest.starts_with(s.as_str())));
            let (len, glyph) = matched.map_or((c.len_utf8(), None), |(s, i)| (s.len(), Some(*i)));
            if let (Some(p), Some(g), Some(last)) = (prev, glyph, out.last_mut())
                && let Some(k) = self.kern.get(&(p, g))
            {
                last.x_advance -= k;
            }
            out.push(ShapedGlyph {
                paths: glyph.map(|g| self.glyphs[g].paths.clone()).unwrap_or_default(),
                x_advance: glyph.map_or(self.missing_advance, |g| self.glyphs[g].advance),
                x_offset: 0.0,
                y_offset: 0.0,
                space: c.is_whitespace(),
            });
            prev = glyph;
            rest = &rest[len..];
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs>
        <font id="T" horiz-adv-x="500">
          <font-face font-family="Test Font" units-per-em="1000" ascent="800" descent="-200" cap-height="700"/>
          <missing-glyph horiz-adv-x="300"/>
          <glyph unicode="A" glyph-name="A" horiz-adv-x="600" d="M0 0 L300 700 L600 0"/>
          <glyph unicode="V" glyph-name="V" d="M0 700 L250 0 L500 700"/>
          <glyph unicode="fi" glyph-name="f_i" horiz-adv-x="550" d="M0 0 L0 700"/>
          <glyph unicode="f" glyph-name="f" horiz-adv-x="300" d="M0 0 L0 700"/>
          <glyph unicode=" " glyph-name="space" horiz-adv-x="250"/>
          <hkern g1="A" g2="V" k="80"/>
          <hkern u1="V" u2="U+0041" k="40"/>
        </font></defs></svg>"#;

    #[test]
    fn reads_glyphs_metrics_kerning_and_ligatures() {
        let f = StrokeFont::parse_svg(FONT).unwrap();
        assert_eq!(f.family(), "Test Font");
        assert!((f.metrics().cap_height - 700.0).abs() < 1e-9);
        let g = f.shape("AVA");
        assert_eq!(g.len(), 3);
        assert!((g[0].x_advance - 520.0).abs() < 1e-9, "{}", g[0].x_advance);
        assert!((g[1].x_advance - 460.0).abs() < 1e-9);
        assert_eq!(g[0].paths[0].segments.len(), 2);
        // the ligature is one glyph; a missing character takes the missing glyph's advance
        let g = f.shape("fi f é");
        assert_eq!(g.len(), 5);
        assert!((g[0].x_advance - 550.0).abs() < 1e-9);
        assert!(g[1].space && (g[1].x_advance - 250.0).abs() < 1e-9);
        assert!(g[4].paths.is_empty() && (g[4].x_advance - 300.0).abs() < 1e-9);
        assert!(f.covers('A') && !f.covers('é'));
        assert!(StrokeFont::parse_svg("<svg/>").is_err());
        assert!(StrokeFont::parse_svg("<svg><font/></svg>").is_err());
        assert!(StrokeFont::parse_svg("<").is_err());
        assert_eq!(unicode_list("a, U+0042,U+zz,"), ["a", "B"]);
    }

    #[test]
    fn the_lasergrbl_font_matches_its_table() {
        let f = StrokeFont::lasergrbl_hershey();
        let g = f.shape("I");
        let expected = hershey::text_polylines("I", 0.0, 0.0, hershey::GLYPH_HEIGHT);
        let got: Vec<Vec<(f64, f64)>> = g[0]
            .paths
            .iter()
            .map(|p| p.to_polyline(0.01).into_iter().map(|q| (q.x, q.y)).collect())
            .collect();
        assert_eq!(got, expected);
        assert!(!f.covers('é'));
    }
}
