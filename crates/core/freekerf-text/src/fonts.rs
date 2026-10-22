//! The fonts a text can use: the built-in single-line fonts (compiled in) and,
//! when the host gives one, a [`FontSource`] of installed fonts. Faces are read
//! once and kept.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::TextError;
use crate::face::{Face, OutlineFont};
use crate::svgfont::StrokeFont;

/// A font family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyInfo {
    /// Family name (what a text's `font` says).
    pub name: String,
    /// Single-line (open strokes): one pass of the laser per stroke.
    pub single_line: bool,
    /// Compiled into FreeKerf (always available).
    pub builtin: bool,
    /// Has a bold face (else bold is drawn thicker).
    pub bold: bool,
    /// Has an italic face (else italic is slanted).
    pub italic: bool,
}

/// The data of one face.
#[derive(Debug, Clone)]
pub enum FaceData {
    /// An SVG font (single-line), as text.
    Svg(Arc<str>),
    /// A TrueType / OpenType file (or collection) and the face's index in it.
    OpenType {
        /// The file.
        bytes: Arc<[u8]>,
        /// Index of the face in a collection (0 otherwise).
        index: u32,
    },
}

/// A face a source found for a family and style.
#[derive(Debug, Clone)]
pub struct SourceFace {
    /// Its data.
    pub data: FaceData,
    /// The face is bold.
    pub bold: bool,
    /// The face is italic.
    pub italic: bool,
}

/// Fonts beyond the built-in ones (the installed fonts; a native host provides it).
pub trait FontSource: Send + Sync {
    /// The families.
    fn families(&self) -> Vec<FamilyInfo>;
    /// The face of `family` closest to the style (the same family, `None` when the
    /// family is unknown).
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace>;
}

/// A built-in family: its faces as (bold, italic, SVG font).
struct Builtin {
    name: &'static str,
    faces: &'static [(bool, bool, &'static str)],
}

macro_rules! font {
    ($file:literal) => {
        include_str!(concat!("../fonts/", $file))
    };
}

/// The built-in families, in the order the font list shows them.
const BUILTIN: &[Builtin] = &[
    Builtin {
        name: "Relief SingleLine",
        faces: &[(false, false, font!("ReliefSingleLine-Regular.svg"))],
    },
    Builtin {
        name: "Hershey Sans",
        faces: &[
            (false, false, font!("HersheySans1.svg")),
            (true, false, font!("HersheySansMed.svg")),
        ],
    },
    Builtin {
        name: "Hershey Serif",
        faces: &[
            (false, false, font!("HersheySerifMed.svg")),
            (false, true, font!("HersheySerifMedItalic.svg")),
            (true, false, font!("HersheySerifBold.svg")),
            (true, true, font!("HersheySerifBoldItalic.svg")),
        ],
    },
    Builtin {
        name: "Hershey Script",
        faces: &[(false, false, font!("HersheyScript1.svg"))],
    },
    Builtin {
        name: "Hershey Gothic English",
        faces: &[(false, false, font!("HersheyGothEnglish.svg"))],
    },
    Builtin {
        name: "EMS Readability",
        faces: &[
            (false, false, font!("EMSReadability.svg")),
            (false, true, font!("EMSReadabilityItalic.svg")),
        ],
    },
    Builtin {
        name: "EMS Nixish",
        faces: &[
            (false, false, font!("EMSNixish.svg")),
            (false, true, font!("EMSNixishItalic.svg")),
        ],
    },
    Builtin {
        name: "EMS Allure",
        faces: &[(false, false, font!("EMSAllure.svg"))],
    },
    Builtin {
        name: "EMS Tech",
        faces: &[(false, false, font!("EMSTech.svg"))],
    },
];

/// The family of new texts.
pub const DEFAULT_FONT: &str = "Relief SingleLine";

/// A face for a text's style: the family's face closest to it, and what has to be
/// drawn because the face does not have it.
#[derive(Debug, Clone)]
pub struct StyledFace {
    /// The face.
    pub face: Face,
    /// Bold is drawn (the face is not bold).
    pub synthetic_bold: bool,
    /// Italic is drawn (the face is not italic).
    pub synthetic_italic: bool,
}

type FaceKey = (String, bool, bool);

struct Inner {
    source: Option<Arc<dyn FontSource>>,
    faces: Mutex<HashMap<FaceKey, (Face, bool, bool)>>,
}

/// The fonts of a session: built-in plus an optional source. Cloning shares the
/// faces already read.
#[derive(Clone)]
pub struct Fonts {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts")
            .field("source", &self.inner.source.is_some())
            .finish_non_exhaustive()
    }
}

/// The same fonts (one is a clone of the other).
impl PartialEq for Fonts {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::builtin()
    }
}

/// The best of `faces` (bold, italic, data) for a style: the exact one, else the
/// one with the same slant, else the same weight, else the first.
fn closest<T>(faces: &[(bool, bool, T)], bold: bool, italic: bool) -> Option<&(bool, bool, T)> {
    faces
        .iter()
        .find(|f| f.0 == bold && f.1 == italic)
        .or_else(|| faces.iter().find(|f| !f.0 && f.1 == italic))
        .or_else(|| faces.iter().find(|f| f.0 == bold && !f.1))
        .or_else(|| faces.iter().find(|f| !f.0 && !f.1))
        .or_else(|| faces.first())
}

impl Fonts {
    /// Only the built-in fonts.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            inner: Arc::new(Inner {
                source: None,
                faces: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The built-in fonts and `source`'s (a built-in family wins a name clash).
    #[must_use]
    pub fn with_source(source: Arc<dyn FontSource>) -> Self {
        Self {
            inner: Arc::new(Inner {
                source: Some(source),
                faces: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Every family: the built-in ones first, then the source's by name.
    #[must_use]
    pub fn families(&self) -> Vec<FamilyInfo> {
        let mut out: Vec<FamilyInfo> = std::iter::once(FamilyInfo {
            name: freekerf_document::LEGACY_FONT.to_owned(),
            single_line: true,
            builtin: true,
            bold: false,
            italic: false,
        })
        .chain(BUILTIN.iter().map(|b| FamilyInfo {
            name: b.name.to_owned(),
            single_line: true,
            builtin: true,
            bold: b.faces.iter().any(|f| f.0),
            italic: b.faces.iter().any(|f| f.1),
        }))
        .collect();
        // LaserGRBL's font comes after the others: it is there for older texts
        out.rotate_left(1);
        if let Some(src) = &self.inner.source {
            let mut more: Vec<FamilyInfo> = src
                .families()
                .into_iter()
                .filter(|f| !out.iter().any(|b| b.name.eq_ignore_ascii_case(&f.name)))
                .map(|f| FamilyInfo { builtin: false, ..f })
                .collect();
            more.sort_by_key(|f| f.name.to_lowercase());
            more.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
            out.extend(more);
        }
        out
    }

    /// The face of `family` for a style.
    ///
    /// # Errors
    /// The family is not available, or its file cannot be read.
    pub fn face(&self, family: &str, bold: bool, italic: bool) -> Result<StyledFace, TextError> {
        let key = (family.to_lowercase(), bold, italic);
        let cached = self
            .inner
            .faces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned();
        let (face, is_bold, is_italic) = match cached {
            Some(c) => c,
            None => {
                let loaded = self.load(family, bold, italic)?;
                self.inner
                    .faces
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(key, loaded.clone());
                loaded
            }
        };
        Ok(StyledFace {
            face,
            synthetic_bold: bold && !is_bold,
            synthetic_italic: italic && !is_italic,
        })
    }

    fn load(&self, family: &str, bold: bool, italic: bool) -> Result<(Face, bool, bool), TextError> {
        if family.eq_ignore_ascii_case(freekerf_document::LEGACY_FONT) {
            return Ok((
                Face::Stroke(Arc::new(StrokeFont::lasergrbl_hershey())),
                false,
                false,
            ));
        }
        if let Some(b) = BUILTIN.iter().find(|b| b.name.eq_ignore_ascii_case(family)) {
            let (fb, fi, svg) =
                closest(b.faces, bold, italic).ok_or_else(|| TextError::UnknownFont(family.into()))?;
            return Ok((Face::Stroke(Arc::new(StrokeFont::parse_svg(svg)?)), *fb, *fi));
        }
        let found = self
            .inner
            .source
            .as_ref()
            .and_then(|s| s.face(family, bold, italic))
            .ok_or_else(|| TextError::UnknownFont(family.into()))?;
        let face = match found.data {
            FaceData::Svg(text) => Face::Stroke(Arc::new(StrokeFont::parse_svg(&text)?)),
            FaceData::OpenType { bytes, index } => Face::Outline(Arc::new(OutlineFont::new(bytes, index)?)),
        };
        Ok((face, found.bold, found.italic))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_font_reads_and_covers_portuguese() {
        let fonts = Fonts::builtin();
        for f in fonts.families() {
            let face = fonts.face(&f.name, false, false).unwrap();
            assert!(face.face.single_line(), "{}", f.name);
            assert!(face.face.covers('A'), "{}", f.name);
            if f.name != freekerf_document::LEGACY_FONT && !f.name.starts_with("Hershey Gothic") {
                for c in "ãçéêóõúÁÇ".chars() {
                    assert!(face.face.covers(c), "{} lacks {c}", f.name);
                }
            }
        }
        assert!(fonts.face("No Such Font", false, false).is_err());
    }

    #[test]
    fn styles_pick_a_face_or_are_drawn() {
        let fonts = Fonts::builtin();
        let serif = fonts.face("hershey serif", true, true).unwrap();
        assert!(!serif.synthetic_bold && !serif.synthetic_italic);
        let sans = fonts.face("Hershey Sans", true, true).unwrap();
        assert!(!sans.synthetic_bold && sans.synthetic_italic);
        let relief = fonts.face(DEFAULT_FONT, true, false).unwrap();
        assert!(relief.synthetic_bold && !relief.synthetic_italic);
        let list = fonts.families();
        assert_eq!(list[0].name, DEFAULT_FONT);
        assert_eq!(list.last().unwrap().name, freekerf_document::LEGACY_FONT);
        assert!(
            list.iter()
                .any(|f| f.name == "Hershey Serif" && f.bold && f.italic)
        );
        assert_eq!(closest::<()>(&[], false, false), None);
    }

    struct Fake;

    impl FontSource for Fake {
        fn families(&self) -> Vec<FamilyInfo> {
            vec![
                FamilyInfo {
                    name: "Zeta".into(),
                    single_line: true,
                    builtin: true,
                    bold: false,
                    italic: false,
                },
                FamilyInfo {
                    name: "hershey sans".into(),
                    single_line: true,
                    builtin: false,
                    bold: false,
                    italic: false,
                },
            ]
        }

        fn face(&self, family: &str, _bold: bool, _italic: bool) -> Option<SourceFace> {
            (family == "Zeta").then(|| SourceFace {
                data: FaceData::Svg(Arc::from(
                    r#"<svg><font><glyph unicode="Z" d="M0 0 L1 1"/></font></svg>"#,
                )),
                bold: false,
                italic: false,
            })
        }
    }

    #[test]
    fn a_source_adds_families() {
        let fonts = Fonts::with_source(Arc::new(Fake));
        let list = fonts.families();
        assert_eq!(list.last().unwrap().name, "Zeta");
        assert!(!list.last().unwrap().builtin);
        assert_eq!(
            list.iter()
                .filter(|f| f.name.eq_ignore_ascii_case("hershey sans"))
                .count(),
            1
        );
        let z = fonts.face("Zeta", true, false).unwrap();
        assert!(z.synthetic_bold && z.face.covers('Z'));
        // read once
        assert!(fonts.face("Zeta", true, false).is_ok());
        assert!(format!("{fonts:?}").contains("source: true"));
    }
}
