//! Text for FreeKerf: fonts, shaping and layout of editable text objects.
//!
//! - [`Fonts`]: the built-in single-line fonts (SVG fonts compiled in: Relief
//!   SingleLine, the Hershey families, the EMS fonts, LaserGRBL's Hershey) and the
//!   installed TrueType / OpenType fonts a host gives as a [`FontSource`];
//! - [`layout`]: a [`TextSpec`] to paths: kerning, ligatures and marks
//!   (`harfrust`), lines and alignment, letter and line spacing, width, bold and
//!   italic (the family's faces, else drawn), underline and strikethrough, text
//!   bent on a circle or along a path, and for outline fonts weld, stencil bridges
//!   and an outline;
//! - [`apply`] and [`follow_paths`]: a text object's geometry from its spec, and
//!   texts that follow a path laid out again when that path changes.
//!
//! Core layer: no I/O (fonts come as bytes), builds for `wasm32-unknown-unknown`.
//! The Hershey text of the test generators ([`hershey`]) also lives here.
//!
//! ```
//! use freekerf_document::TextSpec;
//! use freekerf_text::{DEFAULT_FONT, Fonts, layout};
//!
//! let paths = layout(&TextSpec::new("Olá", 10.0, DEFAULT_FONT), &Fonts::builtin()).unwrap();
//! assert!(paths.len() > 3);
//! ```

mod along;
mod face;
mod fonts;
pub mod hershey;
mod hershey_table;
mod layout;
mod svgfont;
pub mod svgpath;
#[cfg(test)]
mod testfont;

use freekerf_document::{Document, Object, ObjectId, ObjectKind};
use freekerf_geometry::{Matrix, Path};

pub use face::{Face, OutlineFont};
pub use fonts::{DEFAULT_FONT, FaceData, FamilyInfo, FontSource, Fonts, SourceFace, StyledFace};
pub use freekerf_document::{LEGACY_FONT, TextAlign, TextPath, TextSpec};
pub use layout::{MAX_CHARS, check, layout};
pub use svgfont::StrokeFont;

/// Errors of text layout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// The font family is not available.
    #[error("the font `{0}` is not available")]
    UnknownFont(String),
    /// A font file that cannot be read.
    #[error("cannot read the font: {0}")]
    Font(String),
    /// A value out of range.
    #[error("{0}")]
    InvalidSpec(String),
    /// The effects failed (coordinates out of range).
    #[error("geometry: {0}")]
    Geometry(String),
}

/// The baseline an object gives a text: its first path (a shape, not a text).
#[must_use]
pub fn baseline_of(o: &Object) -> Option<&Path> {
    match &o.kind {
        ObjectKind::Shape { paths } if o.text.is_none() => paths.first(),
        _ => None,
    }
}

/// Lays `spec` out and makes it `o`'s text and geometry. A text that follows an
/// object's path is placed by the path alone (its transform is reset).
///
/// # Errors
/// See [`layout`].
pub fn apply(o: &mut Object, spec: TextSpec, fonts: &Fonts) -> Result<(), TextError> {
    let paths = layout(&spec, fonts)?;
    if spec.linked_path().is_some() {
        o.transform = Matrix::IDENTITY;
    }
    o.set_text_paths(spec, &paths);
    Ok(())
}

/// Lays out again the texts whose linked path changed (or that were moved off
/// it), and unlinks those whose path is gone (they keep the last copy). Returns
/// whether anything changed. A text that cannot be laid out (its font is not
/// installed here) keeps its geometry.
pub fn follow_paths(doc: &mut Document, fonts: &Fonts) -> bool {
    let linked: Vec<(ObjectId, ObjectId)> = doc
        .objects
        .iter()
        .filter_map(|o| o.text.as_ref().and_then(TextSpec::linked_path).map(|p| (o.id, p)))
        .collect();
    let mut changed = false;
    for (id, link) in linked {
        let source = doc.object(link).and_then(baseline_of).cloned();
        let Some(o) = doc.object_mut(id) else { continue };
        let Some(mut spec) = o.text.clone() else { continue };
        let Some(tp) = spec.path.as_mut() else { continue };
        match source {
            None => {
                tp.object = None;
                o.text = Some(spec);
                changed = true;
            }
            Some(path) => {
                if tp.path == path && o.transform == Matrix::IDENTITY {
                    continue;
                }
                tp.path = path;
                if apply(o, spec, fonts).is_ok() {
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Copies made in one step (`(original, copy)` ids): a copied text follows the copy
/// of its path when that was copied too, else nothing (it keeps the last copy of
/// the path and moves on its own).
pub fn relink_copies(doc: &mut Document, copies: &[(ObjectId, ObjectId)]) {
    for &(_, new) in copies {
        let Some(o) = doc.object_mut(new) else { continue };
        let Some(tp) = o.text.as_mut().and_then(|t| t.path.as_mut()) else {
            continue;
        };
        tp.object = tp
            .object
            .and_then(|link| copies.iter().find(|(old, _)| *old == link).map(|(_, n)| *n));
    }
}

/// A copy of a text that follows a path no longer follows it (it keeps the last
/// copy of the path and can be moved on its own).
pub fn unlink(o: &mut Object) {
    if let Some(tp) = o.text.as_mut().and_then(|t| t.path.as_mut()) {
        tp.object = None;
    }
}

#[cfg(test)]
mod tests {
    use freekerf_document::Color;
    use freekerf_geometry::Point;

    use super::*;

    #[test]
    fn texts_follow_their_path() {
        let fonts = Fonts::builtin();
        let mut doc = Document::new();
        let layer = doc.layer_for_color(Color::BLACK, 1000, 1000);
        let line = Path::polyline(&[Point::ZERO, Point::new(100.0, 0.0)], false);
        let path_id = doc
            .add_object(
                layer,
                ObjectKind::Shape {
                    paths: vec![line.clone()],
                },
            )
            .unwrap();
        let text_id = doc
            .add_object(layer, ObjectKind::Shape { paths: Vec::new() })
            .unwrap();
        let mut spec = TextSpec::new("Hi", 5.0, DEFAULT_FONT);
        spec.path = Some(TextPath {
            object: Some(path_id),
            path: line,
            offset_mm: 0.0,
            distance_mm: 0.0,
            flip: false,
        });
        apply(doc.object_mut(text_id).unwrap(), spec, &fonts).unwrap();
        assert!(!follow_paths(&mut doc, &fonts), "nothing moved");
        // the path moves: the text goes with it
        doc.object_mut(path_id).unwrap().translate(0.0, 20.0);
        assert!(follow_paths(&mut doc, &fonts));
        assert!(doc.object(text_id).unwrap().bounds().min_y > 19.0);
        // the text alone was moved: back on the path
        doc.object_mut(text_id).unwrap().translate(50.0, 50.0);
        assert!(follow_paths(&mut doc, &fonts));
        let b = doc.object(text_id).unwrap().bounds();
        assert!(b.min_y > 19.0 && b.max_y < 26.0 && b.min_x < 1.5, "{b:?}");
        // the path is gone: the text stays, unlinked
        doc.remove_object(path_id).unwrap();
        assert!(follow_paths(&mut doc, &fonts));
        let o = doc.object(text_id).unwrap();
        assert_eq!(o.text.as_ref().unwrap().linked_path(), None);
        assert!(o.text.as_ref().unwrap().path.is_some());
        let mut copy = o.clone();
        unlink(&mut copy);
        assert!(baseline_of(&copy).is_none(), "a text is not a baseline");
        // copies: a text copied with its path follows the copy, alone it follows nothing
        let mut link = |text: ObjectId, path: ObjectId| {
            let t = doc.object_mut(text).unwrap().text.as_mut().unwrap();
            t.path.as_mut().unwrap().object = Some(path);
        };
        link(text_id, ObjectId(99));
        relink_copies(&mut doc, &[(ObjectId(99), ObjectId(5)), (ObjectId(1), text_id)]);
        assert_eq!(
            doc.object(text_id).unwrap().text.as_ref().unwrap().linked_path(),
            Some(ObjectId(5))
        );
        relink_copies(&mut doc, &[(ObjectId(1), text_id)]);
        assert_eq!(
            doc.object(text_id).unwrap().text.as_ref().unwrap().linked_path(),
            None
        );
    }
}
