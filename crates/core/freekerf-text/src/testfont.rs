//! A tiny TrueType font made in memory for the tests (outline fonts without an
//! installed font): `H`, `I`, `O` (with a hole) and the space, 1000 units per em,
//! capitals 700 high.

use std::sync::Arc;

use crate::fonts::{FaceData, FamilyInfo, FontSource, Fonts, SourceFace};

/// Its family name.
pub(crate) const FAMILY: &str = "Test Outline";

/// Contours (TrueType: outer clockwise, holes counterclockwise), advance.
type GlyphDef = (&'static [&'static [(i16, i16)]], u16);

const GLYPHS: [GlyphDef; 5] = [
    (&[], 500),                                              // .notdef
    (&[], 300),                                              // space
    (&[&[(100, 0), (100, 700), (600, 700), (600, 0)]], 700), // H (a block)
    (&[&[(100, 0), (100, 700), (300, 700), (300, 0)]], 400), // I
    (
        &[
            &[(50, 0), (50, 700), (650, 700), (650, 0)],
            &[(200, 150), (500, 150), (500, 550), (200, 550)],
        ],
        700,
    ), // O
];

/// (character, glyph).
const CMAP: [(u32, u32); 4] = [(0x20, 1), (0x48, 2), (0x49, 3), (0x4F, 4)];

fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn bei16(v: &mut Vec<u8>, x: i16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn glyph(contours: &[&[(i16, i16)]]) -> Vec<u8> {
    let mut g = Vec::new();
    if contours.is_empty() {
        return g;
    }
    let pts: Vec<(i16, i16)> = contours.iter().flat_map(|c| c.iter().copied()).collect();
    let (x0, x1) = (
        pts.iter().map(|p| p.0).min().unwrap_or(0),
        pts.iter().map(|p| p.0).max().unwrap_or(0),
    );
    let (y0, y1) = (
        pts.iter().map(|p| p.1).min().unwrap_or(0),
        pts.iter().map(|p| p.1).max().unwrap_or(0),
    );
    bei16(&mut g, i16::try_from(contours.len()).unwrap_or(0));
    for v in [x0, y0, x1, y1] {
        bei16(&mut g, v);
    }
    let mut end = 0u16;
    for c in contours {
        end += u16::try_from(c.len()).unwrap_or(0);
        be16(&mut g, end - 1);
    }
    be16(&mut g, 0); // no instructions
    g.extend(std::iter::repeat_n(1u8, pts.len())); // on curve, long coordinates
    let (mut px, mut py) = (0i16, 0i16);
    for p in &pts {
        bei16(&mut g, p.0 - px);
        px = p.0;
    }
    for p in &pts {
        bei16(&mut g, p.1 - py);
        py = p.1;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    g
}

/// The font file.
pub(crate) fn ttf() -> Vec<u8> {
    let n = u16::try_from(GLYPHS.len()).unwrap_or(0);
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0);
    be16(&mut head, 1000);
    head.extend([0u8; 16]);
    for v in [0i16, 0, 700, 700] {
        bei16(&mut head, v);
    }
    be16(&mut head, 0);
    be16(&mut head, 8);
    bei16(&mut head, 2);
    bei16(&mut head, 1); // long offsets
    bei16(&mut head, 0);

    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    for v in [800i16, -200, 0] {
        bei16(&mut hhea, v);
    }
    be16(&mut hhea, 700);
    for v in [0i16, 0, 700, 1, 0, 0, 0, 0, 0, 0, 0] {
        bei16(&mut hhea, v);
    }
    be16(&mut hhea, n);

    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0000_5000);
    be16(&mut maxp, n);

    let mut hmtx = Vec::new();
    for (_, adv) in GLYPHS {
        be16(&mut hmtx, adv);
        bei16(&mut hmtx, 0);
    }

    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for (contours, _) in GLYPHS {
        be32(&mut loca, u32::try_from(glyf.len()).unwrap_or(0));
        glyf.extend(glyph(contours));
    }
    be32(&mut loca, u32::try_from(glyf.len()).unwrap_or(0));

    let mut cmap = Vec::new();
    be16(&mut cmap, 0);
    be16(&mut cmap, 1);
    be16(&mut cmap, 0); // Unicode
    be16(&mut cmap, 4); // full repertoire
    be32(&mut cmap, 12);
    be16(&mut cmap, 12); // format 12
    be16(&mut cmap, 0);
    be32(&mut cmap, 16 + 12 * u32::try_from(CMAP.len()).unwrap_or(0));
    be32(&mut cmap, 0);
    be32(&mut cmap, u32::try_from(CMAP.len()).unwrap_or(0));
    for (c, g) in CMAP {
        be32(&mut cmap, c);
        be32(&mut cmap, c);
        be32(&mut cmap, g);
    }

    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    be16(&mut out, 7);
    be16(&mut out, 64);
    be16(&mut out, 2);
    be16(&mut out, 48);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        be32(&mut out, 0);
        be32(&mut out, u32::try_from(offset).unwrap_or(0));
        be32(&mut out, u32::try_from(data.len()).unwrap_or(0));
        let mut d = data.clone();
        while d.len() % 4 != 0 {
            d.push(0);
        }
        offset += d.len();
        body.extend(d);
    }
    out.extend(body);
    out
}

struct Source(Arc<[u8]>);

impl FontSource for Source {
    fn families(&self) -> Vec<FamilyInfo> {
        vec![FamilyInfo {
            name: FAMILY.into(),
            single_line: false,
            builtin: false,
            bold: false,
            italic: false,
        }]
    }

    fn face(&self, family: &str, _bold: bool, _italic: bool) -> Option<SourceFace> {
        (family == FAMILY).then(|| SourceFace {
            data: FaceData::OpenType {
                bytes: self.0.clone(),
                index: 0,
            },
            bold: false,
            italic: false,
        })
    }
}

/// Built-in fonts plus the test font.
pub(crate) fn fonts() -> Fonts {
    Fonts::with_source(Arc::new(Source(Arc::from(ttf()))))
}
