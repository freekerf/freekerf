//! The application state as data (PLAN §9.1): the document, the selection, the
//! view ([`crate::view`]: mode, tool, panel, dialogs, toasts, viewport — rendered by
//! the desktop app) and
//! the undo history, changed only through actions ([`crate::actions`]). Every
//! change bumps the revision and records a [`SessionEvent`] for the front ends.

use std::collections::BTreeSet;

use freekerf_document::{Document, DocumentError, Layer, LayerId, Object, ObjectId, TextSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Most undo steps kept.
pub const HISTORY_LIMIT: usize = 100;

pub use crate::view::{Mode, ViewState, Viewport};

/// What changed (events for the front ends, carried with the revision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// The document.
    Document,
    /// The selection or the current layer.
    Selection,
    /// The view.
    View,
    /// The undo history (steps added, undone, redone).
    History,
    /// The clipboard (`document.copy`, `document.cut`).
    Clipboard,
}

/// A change and the revision it produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionEvent {
    /// Revision after the change.
    pub revision: u64,
    /// What changed.
    pub change: Change,
}

/// An undoable state: the document and the selection.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    label: String,
    document: Document,
    selection: BTreeSet<ObjectId>,
}

/// An open transaction: changes inside it form one undo step.
#[derive(Debug, Clone)]
struct Transaction {
    before: Step,
    view: ViewState,
    changed: bool,
}

/// Objects copied with `document.copy` or `document.cut`, with their layers. Not
/// part of the document nor of the undo history: it survives opening another one.
#[derive(Debug, Clone, Default)]
struct Clipboard {
    objects: Vec<Object>,
    layers: Vec<Layer>,
    /// The document (generation) the objects come from.
    origin: u64,
    /// The document of the last paste and how many pastes it had: each paste is
    /// offset once more, so copies do not pile up.
    target: u64,
    next: u32,
}

/// The session.
#[derive(Debug, Clone, Default)]
pub struct Session {
    document: Document,
    selection: BTreeSet<ObjectId>,
    view: ViewState,
    undo: Vec<Step>,
    redo: Vec<Step>,
    transaction: Option<Transaction>,
    events: Vec<SessionEvent>,
    revision: u64,
    dirty: bool,
    current_layer: Option<LayerId>,
    document_revision: u64,
    clipboard: Clipboard,
    /// Grows with every replaced document (new, opened).
    generation: u64,
}

/// The state as a front end or an agent reads it (`session.snapshot`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Snapshot {
    /// Revision (grows with every change).
    pub revision: u64,
    /// Revision of the last document change (the geometry is the same while it is).
    pub document_revision: u64,
    /// Unsaved changes.
    pub dirty: bool,
    /// Selected object ids.
    pub selection: Vec<u64>,
    /// The layer new objects go to (see [`Session::current_layer`]).
    pub current_layer: Option<u32>,
    /// View.
    pub view: ViewState,
    /// Labels of the steps that can be undone, oldest first.
    pub undo: Vec<String>,
    /// Labels of the steps that can be redone, next first.
    pub redo: Vec<String>,
    /// Layers and objects, without geometry.
    pub document: DocumentSummary,
    /// Objects on the clipboard (`document.copy`, `document.cut`; `document.paste` adds them).
    pub clipboard: usize,
}

/// A layer without its objects.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LayerSummary {
    /// Id.
    pub id: u32,
    /// `#RRGGBB`.
    pub color: String,
    /// Name.
    pub name: String,
    /// Mode (`line`, `fill`, `fill_and_line`, `cut`, `ignore`).
    pub mode: freekerf_document::LayerMode,
    /// Speed, mm/min.
    pub speed_mm_min: u32,
    /// `S` power.
    pub power: u32,
    /// Passes.
    pub passes: u32,
    /// Shown and planned.
    pub visible: bool,
    /// Hatch filling (fill modes).
    pub fill: freekerf_document::FillSettings,
    /// Laser on command: `machine` (the machine's, `M3`), `constant` (`M3`) or
    /// `dynamic` (`M4`).
    pub laser_mode: freekerf_document::LaserMode,
    /// Objects on it.
    pub objects: usize,
}

/// An object without its geometry.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ObjectSummary {
    /// Id.
    pub id: u64,
    /// Layer id.
    pub layer: u32,
    /// Name.
    pub name: String,
    /// `shape`, `text`, `image` or `unsupported`.
    pub kind: String,
    /// `[min_x, min_y, max_x, max_y]` in mm, none for unsupported elements.
    pub bounds: Option<[f64; 4]>,
    /// Every transform applied since the object was made, `[a, b, c, d, e, f]`:
    /// `x' = a·x + c·y + e`, `y' = b·x + d·y + f` (the geometry already includes it).
    pub transform: [f64; 6],
    /// The text of a text object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextSpec>,
    /// How an image object is engraved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageSummary>,
}

/// How an image is engraved (the raster dialog's main choices).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImageSummary {
    /// Line by line, dithering, vectorize, centerline or pass-through.
    pub tool: freekerf_document::RasterTool,
    /// Dithering algorithm (when dithering).
    pub dithering: freekerf_document::DitherMode,
    /// Lines per mm.
    pub lines_per_mm: f64,
}

/// The document without geometry.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocumentSummary {
    /// Layers in id order.
    pub layers: Vec<LayerSummary>,
    /// Objects in drawing order.
    pub objects: Vec<ObjectSummary>,
    /// Bounds of everything, mm.
    pub bounds: Option<[f64; 4]>,
}

fn rect(r: freekerf_geometry::Rect) -> Option<[f64; 4]> {
    (r.min_x <= r.max_x && r.min_y <= r.max_y).then_some([r.min_x, r.min_y, r.max_x, r.max_y])
}

/// The summary of a document.
#[must_use]
pub fn summary(doc: &Document) -> DocumentSummary {
    use freekerf_document::ObjectKind;
    DocumentSummary {
        layers: doc
            .layers
            .iter()
            .map(|l| LayerSummary {
                id: l.id.0,
                color: l.color.to_string(),
                name: l.name.clone(),
                mode: l.mode,
                speed_mm_min: l.speed_mm_min,
                power: l.power,
                passes: l.passes,
                visible: l.visible,
                fill: l.fill,
                laser_mode: l.laser_mode,
                objects: doc.objects_on(l.id).count(),
            })
            .collect(),
        objects: doc
            .objects
            .iter()
            .map(|o| ObjectSummary {
                id: o.id.0,
                layer: o.layer.0,
                name: o.name.clone(),
                kind: match o.kind {
                    ObjectKind::Shape { .. } if o.text.is_some() => "text",
                    ObjectKind::Shape { .. } => "shape",
                    ObjectKind::Image(_) => "image",
                    ObjectKind::Unsupported { .. } => "unsupported",
                }
                .into(),
                bounds: rect(o.bounds()),
                transform: {
                    let m = o.transform;
                    [m.m11, m.m12, m.m21, m.m22, m.offset_x, m.offset_y]
                },
                text: o.text.clone(),
                image: match &o.kind {
                    ObjectKind::Image(img) => Some(ImageSummary {
                        tool: img.settings.tool,
                        dithering: img.settings.dithering,
                        lines_per_mm: img.settings.lines_per_mm,
                    }),
                    _ => None,
                },
            })
            .collect(),
        bounds: rect(doc.bounds()),
    }
}

impl Session {
    /// An empty session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The document.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// The selected objects.
    #[must_use]
    pub const fn selection(&self) -> &BTreeSet<ObjectId> {
        &self.selection
    }

    /// The view.
    #[must_use]
    pub const fn view(&self) -> &ViewState {
        &self.view
    }

    /// The revision (grows with every change).
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Unsaved changes since the last [`Session::mark_saved`].
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Records that the document was saved.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Takes the events recorded since the last call.
    pub fn drain_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    fn emit(&mut self, change: Change) {
        self.revision += 1;
        if change == Change::Document {
            self.document_revision = self.revision;
        }
        self.events.push(SessionEvent {
            revision: self.revision,
            change,
        });
    }

    fn current(&self, label: &str) -> Step {
        Step {
            label: label.to_owned(),
            document: self.document.clone(),
            selection: self.selection.clone(),
        }
    }

    fn push_undo(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Changes the document as one undo step labelled `label` (inside a transaction,
    /// part of the transaction's step). `f` returns whether it changed anything;
    /// on an error nothing is changed.
    ///
    /// # Errors
    /// `f`'s error (the document is restored).
    pub fn edit<E>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Document, &mut BTreeSet<ObjectId>) -> Result<bool, E>,
    ) -> Result<bool, E> {
        // inside a transaction the transaction's snapshot is the undo point: no
        // copy per change (a script may make thousands); actions validate before
        // they change anything, so an error leaves the document as it was
        if self.transaction.is_some() {
            let sel = self.selection.clone();
            let changed = f(&mut self.document, &mut self.selection)?;
            if changed {
                let doc = &self.document;
                self.selection.retain(|id| doc.object(*id).is_some());
                if let Some(t) = &mut self.transaction {
                    t.changed = true;
                }
                self.dirty = true;
                self.emit(Change::Document);
                if self.selection != sel {
                    self.emit(Change::Selection);
                }
            }
            return Ok(changed);
        }
        let before = self.current(label);
        match f(&mut self.document, &mut self.selection) {
            Ok(false) => Ok(false),
            Ok(true) => {
                // ids that no longer exist leave the selection
                let doc = &self.document;
                self.selection.retain(|id| doc.object(*id).is_some());
                self.push_undo(before.clone());
                self.emit(Change::History);
                self.dirty = true;
                if self.document != before.document {
                    self.emit(Change::Document);
                }
                if self.selection != before.selection {
                    self.emit(Change::Selection);
                }
                Ok(true)
            }
            Err(e) => {
                self.document = before.document;
                self.selection = before.selection;
                Err(e)
            }
        }
    }

    /// Replaces the selection (not an undo step).
    pub fn select(&mut self, ids: BTreeSet<ObjectId>) {
        let doc = &self.document;
        let ids: BTreeSet<ObjectId> = ids.into_iter().filter(|id| doc.object(*id).is_some()).collect();
        if ids != self.selection {
            self.selection = ids;
            self.emit(Change::Selection);
        }
    }

    /// The layer new objects go to: the one chosen with
    /// [`Session::set_current_layer`] while it exists, else the first layer.
    #[must_use]
    pub fn current_layer(&self) -> Option<LayerId> {
        self.current_layer
            .filter(|id| self.document.layer(*id).is_some())
            .or_else(|| self.document.layers.first().map(|l| l.id))
    }

    /// Chooses the layer new objects go to (not an undo step). Returns `false`
    /// for an unknown layer.
    pub fn set_current_layer(&mut self, id: LayerId) -> bool {
        if self.document.layer(id).is_none() {
            return false;
        }
        let before = self.current_layer();
        self.current_layer = Some(id);
        if before != Some(id) {
            self.emit(Change::Selection);
        }
        true
    }

    /// Changes the view (not an undo step).
    pub fn change_view(&mut self, f: impl FnOnce(&mut ViewState)) {
        let before = self.view.clone();
        f(&mut self.view);
        if self.view != before {
            self.emit(Change::View);
        }
    }

    /// Replaces the whole document (new, opened from a file): the history starts
    /// again.
    pub fn replace_document(&mut self, doc: Document) {
        self.document = doc;
        self.generation += 1;
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
        self.transaction = None;
        self.dirty = false;
        self.emit(Change::Document);
        self.emit(Change::Selection);
        self.emit(Change::History);
    }

    /// Puts copies of these objects (in drawing order) and their layers on the
    /// clipboard (not an undo step) and returns how many. Nothing to copy leaves
    /// the clipboard as it was. The first paste in this document is offset by one
    /// step, or none with `in_place` (a cut).
    pub fn copy(&mut self, ids: &BTreeSet<ObjectId>, in_place: bool) -> usize {
        let objects: Vec<Object> = self
            .document
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .cloned()
            .collect();
        if objects.is_empty() {
            return 0;
        }
        let layers = self
            .document
            .layers
            .iter()
            .filter(|l| objects.iter().any(|o| o.layer == l.id))
            .cloned()
            .collect();
        let n = objects.len();
        self.clipboard = Clipboard {
            objects,
            layers,
            origin: self.generation,
            target: self.generation,
            next: u32::from(!in_place),
        };
        self.emit(Change::Clipboard);
        n
    }

    /// Objects on the clipboard.
    #[must_use]
    pub const fn clipboard_len(&self) -> usize {
        self.clipboard.objects.len()
    }

    /// Adds the clipboard's objects as one undo step, selects them and returns
    /// their ids. Each goes to the layer of its colour (the one it came from when it
    /// still has that colour), created with the copied settings when missing. They
    /// are offset by `offset_mm`, or centred on `at_mm`, or else offset by `step_mm`
    /// once more for every paste in this document (none for the first paste in
    /// another document or after a cut).
    ///
    /// # Errors
    /// A layer could not be found (the document is left as it was).
    pub fn paste(
        &mut self,
        step_mm: [f64; 2],
        offset_mm: Option<[f64; 2]>,
        at_mm: Option<[f64; 2]>,
    ) -> Result<Vec<ObjectId>, DocumentError> {
        if self.clipboard.objects.is_empty() {
            return Ok(Vec::new());
        }
        if self.clipboard.target != self.generation {
            self.clipboard.target = self.generation;
            self.clipboard.next = u32::from(self.generation == self.clipboard.origin);
        }
        let centred = at_mm.and_then(|[x, y]| {
            let mut b = self.clipboard.objects.first()?.bounds();
            for o in &self.clipboard.objects {
                b.union(&o.bounds());
            }
            (!b.is_empty()).then(|| [x - (b.min_x + b.max_x) / 2.0, y - (b.min_y + b.max_y) / 2.0])
        });
        let [dx, dy] = offset_mm.or(centred).unwrap_or_else(|| {
            let n = f64::from(self.clipboard.next);
            [step_mm[0] * n, step_mm[1] * n]
        });
        let Clipboard { objects, layers, .. } = self.clipboard.clone();
        let mut new = Vec::new();
        self.edit("paste", |d, sel| {
            for mut o in objects {
                let Some(from) = layers.iter().find(|l| l.id == o.layer) else {
                    continue;
                };
                let layer = match d.layer(from.id) {
                    Some(l) if l.color == from.color => l.id,
                    _ => match d.layer_by_color(from.color) {
                        Some(l) => l.id,
                        None => d.add_layer(from.clone()),
                    },
                };
                o.translate(dx, dy);
                let id = d.add_object_with(layer, o.kind, o.name, o.source)?;
                if let Some(added) = d.object_mut(id) {
                    added.text = o.text;
                    added.transform = o.transform;
                }
                new.push(id);
            }
            *sel = new.iter().copied().collect();
            Ok(!new.is_empty())
        })?;
        self.clipboard.next += 1;
        Ok(new)
    }

    /// Undoes the last step. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        if self.transaction.is_some() {
            return None;
        }
        let step = self.undo.pop()?;
        let label = step.label.clone();
        let now = self.current(&label);
        self.redo.push(now);
        self.document = step.document;
        self.selection = step.selection;
        self.dirty = true;
        self.emit(Change::Document);
        self.emit(Change::Selection);
        self.emit(Change::History);
        Some(label)
    }

    /// Redoes the last undone step. Returns its label.
    pub fn redo(&mut self) -> Option<String> {
        if self.transaction.is_some() {
            return None;
        }
        let step = self.redo.pop()?;
        let label = step.label.clone();
        let now = self.current(&label);
        self.undo.push(now);
        self.document = step.document;
        self.selection = step.selection;
        self.dirty = true;
        self.emit(Change::Document);
        self.emit(Change::Selection);
        self.emit(Change::History);
        Some(label)
    }

    /// Starts a transaction (a script): every change until [`Session::commit`]
    /// forms one undo step labelled `label`. Returns `false` when one is already
    /// open (transactions do not nest).
    pub fn begin(&mut self, label: &str) -> bool {
        if self.transaction.is_some() {
            return false;
        }
        self.transaction = Some(Transaction {
            before: self.current(label),
            view: self.view.clone(),
            changed: false,
        });
        true
    }

    /// Whether a transaction is open.
    #[must_use]
    pub const fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Ends the transaction, keeping its changes as one undo step.
    pub fn commit(&mut self) {
        if let Some(t) = self.transaction.take()
            && t.changed
        {
            self.push_undo(t.before);
            self.emit(Change::History);
        }
    }

    /// Ends the transaction, undoing every document, selection and view change
    /// made inside it.
    pub fn rollback(&mut self) {
        if let Some(t) = self.transaction.take() {
            if self.document != t.before.document {
                self.document = t.before.document;
                self.emit(Change::Document);
            }
            if self.selection != t.before.selection {
                self.selection = t.before.selection;
                self.emit(Change::Selection);
            }
            if self.view != t.view {
                self.view = t.view;
                self.emit(Change::View);
            }
        }
    }

    /// The state for front ends and agents.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            revision: self.revision,
            document_revision: self.document_revision,
            dirty: self.dirty,
            selection: self.selection.iter().map(|id| id.0).collect(),
            current_layer: self.current_layer().map(|l| l.0),
            view: self.view.clone(),
            undo: self.undo.iter().map(|s| s.label.clone()).collect(),
            redo: self.redo.iter().rev().map(|s| s.label.clone()).collect(),
            document: summary(&self.document),
            clipboard: self.clipboard_len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use freekerf_document::{Color, Layer, LayerId, ObjectKind};

    use super::*;

    fn shape(s: &mut Session) -> ObjectId {
        let mut id = ObjectId(0);
        s.edit::<()>("add", |d, _| {
            let l = d.add_layer(Layer::new(LayerId(0), Color::BLACK, 1000, 100));
            id = d.add_object(l, ObjectKind::Shape { paths: Vec::new() }).unwrap();
            Ok(true)
        })
        .unwrap();
        id
    }

    #[test]
    fn edits_undo_redo_and_events() {
        let mut s = Session::new();
        let id = shape(&mut s);
        assert!(s.is_dirty());
        assert_eq!(s.document().objects.len(), 1);
        s.select([id].into());
        assert_eq!(s.selection().len(), 1);
        let ev = s.drain_events();
        assert!(ev.iter().any(|e| e.change == Change::Document));
        assert!(ev.iter().any(|e| e.change == Change::Selection));
        assert!(s.drain_events().is_empty());
        // deleting removes it from the selection
        s.edit::<()>("delete", |d, _| {
            d.remove_object(id).unwrap();
            Ok(true)
        })
        .unwrap();
        assert!(s.selection().is_empty());
        assert_eq!(s.undo().as_deref(), Some("delete"));
        assert_eq!(s.document().objects.len(), 1);
        assert_eq!(s.redo().as_deref(), Some("delete"));
        assert!(s.document().objects.is_empty());
        assert_eq!(s.snapshot().undo, ["add", "delete"]);
        // no change: no step; an error: nothing changed
        assert_eq!(s.edit::<()>("nothing", |_, _| Ok(false)), Ok(false));
        assert_eq!(
            s.edit("bad", |d, _| {
                d.layers.clear();
                Err("no")
            }),
            Err("no")
        );
        assert_eq!(s.document().layers.len(), 1);
        assert_eq!(s.snapshot().undo.len(), 2);
        assert!(s.redo().is_none());
        s.mark_saved();
        assert!(!s.is_dirty());
    }

    #[test]
    fn transactions_are_one_step_or_nothing() {
        let mut s = Session::new();
        assert!(s.begin("script"));
        assert!(!s.begin("nested"));
        assert!(s.in_transaction());
        let id = shape(&mut s);
        let _ = shape(&mut s);
        s.select([id].into());
        s.change_view(|v| v.panel = crate::view::Panel::Layers);
        assert!(s.undo().is_none(), "no undo inside a transaction");
        s.commit();
        assert_eq!(s.snapshot().undo, ["script"]);
        assert_eq!(s.document().objects.len(), 2);
        s.undo();
        assert!(s.document().objects.is_empty());

        // rollback restores document, selection and view
        s.redo();
        assert!(s.begin("failing"));
        let _ = shape(&mut s);
        s.select(BTreeSet::new());
        s.change_view(|v| v.mode = Mode::Produce);
        s.rollback();
        assert_eq!(s.document().objects.len(), 2);
        assert_eq!(s.view().mode, Mode::Design);
        assert_eq!(s.snapshot().undo, ["script"]);
        // an empty transaction leaves no step
        s.begin("empty");
        s.commit();
        assert_eq!(s.snapshot().undo, ["script"]);
        s.commit();
        s.rollback();
    }

    #[test]
    fn history_is_bounded_and_documents_replace() {
        let mut s = Session::new();
        for _ in 0..(HISTORY_LIMIT + 5) {
            let _ = shape(&mut s);
        }
        assert_eq!(s.snapshot().undo.len(), HISTORY_LIMIT);
        s.select([ObjectId(999_999)].into());
        assert!(s.selection().is_empty(), "unknown ids are not selected");
        s.replace_document(Document::new());
        assert!(s.snapshot().undo.is_empty() && !s.is_dirty());
        let snap = s.snapshot();
        assert!(snap.document.bounds.is_none());
        let id = shape(&mut s);
        let summary = s.snapshot().document;
        assert_eq!(summary.objects[0].id, id.0);
        assert_eq!(summary.layers[0].objects, 1);
    }
}
