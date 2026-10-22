//! The installed fonts, for texts: the system's TrueType / OpenType fonts
//! (`fontdb`, through fontconfig's folders on Linux) and a folder of the user's
//! own fonts, where single-line SVG fonts can go too. The folders are read the
//! first time a font is asked for, not at start.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use freekerf_text::{FaceData, FamilyInfo, FontSource, SourceFace, StrokeFont};

/// Weights from this one up are bold.
const BOLD: u16 = 600;

struct Scanned {
    db: fontdb::Database,
    /// User SVG fonts: family, file.
    svg: Vec<(String, PathBuf)>,
}

/// Installed fonts.
pub struct SystemFonts {
    /// Folder of the user's fonts (TrueType, OpenType, SVG), if any.
    user_dir: Option<PathBuf>,
    /// Read the system's fonts (off for tests).
    system: bool,
    scanned: OnceLock<Scanned>,
}

impl std::fmt::Debug for SystemFonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemFonts")
            .field("user_dir", &self.user_dir)
            .field("system", &self.system)
            .finish_non_exhaustive()
    }
}

fn svg_fonts(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("svg")))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let font = StrokeFont::parse_svg(&text).ok()?;
            let family = if font.family().trim().is_empty() {
                p.file_stem()?.to_string_lossy().into_owned()
            } else {
                font.family().trim().to_owned()
            };
            Some((family, p))
        })
        .collect();
    out.sort();
    out
}

impl SystemFonts {
    /// The system's fonts and those in `user_dir`.
    #[must_use]
    pub fn new(user_dir: Option<PathBuf>) -> Self {
        Self {
            user_dir,
            system: true,
            scanned: OnceLock::new(),
        }
    }

    /// Only the fonts in `dir` (tests).
    #[must_use]
    pub fn only(dir: PathBuf) -> Self {
        Self {
            user_dir: Some(dir),
            system: false,
            scanned: OnceLock::new(),
        }
    }

    fn scanned(&self) -> &Scanned {
        self.scanned.get_or_init(|| {
            let mut db = fontdb::Database::new();
            if self.system {
                db.load_system_fonts();
            }
            let mut svg = Vec::new();
            if let Some(dir) = &self.user_dir {
                db.load_fonts_dir(dir);
                svg = svg_fonts(dir);
            }
            tracing::debug!(faces = db.len(), svg = svg.len(), "fonts scanned");
            Scanned { db, svg }
        })
    }
}

fn family_of(f: &fontdb::FaceInfo) -> Option<&str> {
    f.families
        .first()
        .map(|(name, _)| name.as_str())
        .filter(|n| !n.is_empty())
}

impl FontSource for SystemFonts {
    fn families(&self) -> Vec<FamilyInfo> {
        let s = self.scanned();
        let mut out: Vec<FamilyInfo> = Vec::new();
        for f in s.db.faces() {
            let Some(name) = family_of(f) else { continue };
            let bold = f.weight.0 >= BOLD;
            let italic = f.style != fontdb::Style::Normal;
            match out.iter_mut().find(|i| i.name == name) {
                Some(i) => {
                    i.bold |= bold;
                    i.italic |= italic;
                }
                None => out.push(FamilyInfo {
                    name: name.to_owned(),
                    single_line: false,
                    builtin: false,
                    bold,
                    italic,
                }),
            }
        }
        for (name, _) in &s.svg {
            if !out.iter().any(|i| i.name == *name) {
                out.push(FamilyInfo {
                    name: name.clone(),
                    single_line: true,
                    builtin: false,
                    bold: false,
                    italic: false,
                });
            }
        }
        out
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace> {
        let s = self.scanned();
        if let Some((_, path)) = s.svg.iter().find(|(n, _)| n.eq_ignore_ascii_case(family)) {
            let text = std::fs::read_to_string(path).ok()?;
            return Some(SourceFace {
                data: FaceData::Svg(Arc::from(text)),
                bold: false,
                italic: false,
            });
        }
        // the family's name as the fonts write it
        let name =
            s.db.faces()
                .filter_map(family_of)
                .find(|n| n.eq_ignore_ascii_case(family))?;
        let id = s.db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: if bold {
                fontdb::Weight::BOLD
            } else {
                fontdb::Weight::NORMAL
            },
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        })?;
        let info = s.db.face(id)?;
        let (is_bold, is_italic) = (info.weight.0 >= BOLD, info.style != fontdb::Style::Normal);
        let (bytes, index) =
            s.db.with_face_data(id, |data, index| (Arc::<[u8]>::from(data), index))?;
        Some(SourceFace {
            data: FaceData::OpenType { bytes, index },
            bold: is_bold,
            italic: is_italic,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_svg_fonts_are_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Mine.svg"),
            r#"<svg><font><font-face font-family="My Line"/><glyph unicode="a" d="M0 0 L1 1"/></font></svg>"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.svg"), "<svg>").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let fonts = SystemFonts::only(dir.path().to_path_buf());
        let list = fonts.families();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "My Line");
        assert!(list[0].single_line);
        let face = fonts.face("my line", true, false).unwrap();
        assert!(matches!(face.data, FaceData::Svg(_)) && !face.bold);
        assert!(fonts.face("Other", false, false).is_none());
        // through the text crate
        let all = freekerf_text::Fonts::with_source(Arc::new(fonts));
        assert!(all.face("My Line", false, false).unwrap().face.covers('a'));
        assert!(format!("{:?}", SystemFonts::new(None)).contains("system: true"));
    }
}
