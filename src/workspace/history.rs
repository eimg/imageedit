use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

use image::RgbaImage;

use crate::document::{DocumentContent, LayerId};
use crate::selection::Selection;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub entries: usize,
    pub tab_bytes: u64,
    pub app_bytes: u64,
}

pub(super) const LIMITS: Limits = Limits {
    entries: 100,
    tab_bytes: 256 * 1024 * 1024,
    app_bytes: 512 * 1024 * 1024,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Revision(u64);

#[derive(Clone)]
pub(super) struct State {
    pub content: DocumentContent,
    pub active_layer: LayerId,
    pub selection: Selection,
}

struct Entry {
    state: State,
    revision: Revision,
    // Orders entries across tabs for app-wide eviction.
    stamp: u64,
}

pub(super) struct History {
    undo: VecDeque<Entry>,
    // The next redo target is last.
    redo: Vec<Entry>,
    revision: Revision,
    saved: Option<Revision>,
    next_revision: u64,
    generation: u64,
}

impl History {
    pub fn new(saved: bool) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            revision: Revision(0),
            saved: saved.then_some(Revision(0)),
            next_revision: 1,
            generation: 0,
        }
    }

    // Advances on every change to the current state, including undo and redo.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    // Identifies the content that Save writes.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.revision)
    }

    pub fn mark_saved(&mut self, revision: Revision) {
        self.saved = Some(revision);
    }

    pub fn undo_target(&self) -> Option<(&State, Revision)> {
        self.undo.back().map(|entry| (&entry.state, entry.revision))
    }

    pub fn redo_target(&self) -> Option<(&State, Revision)> {
        self.redo.last().map(|entry| (&entry.state, entry.revision))
    }

    pub fn commit(&mut self, previous: State, stamp: u64, persistent: bool) {
        self.redo.clear();
        self.undo.push_back(Entry {
            state: previous,
            revision: self.revision,
            stamp,
        });
        if persistent {
            self.revision = Revision(self.next_revision);
            self.next_revision += 1;
        }
        self.generation += 1;
    }

    pub fn undo(&mut self, current: State, stamp: u64) -> Option<State> {
        let entry = self.undo.pop_back()?;
        self.redo.push(Entry {
            state: current,
            revision: self.revision,
            stamp,
        });
        Some(self.restore(entry))
    }

    pub fn redo(&mut self, current: State, stamp: u64) -> Option<State> {
        let entry = self.redo.pop()?;
        self.undo.push_back(Entry {
            state: current,
            revision: self.revision,
            stamp,
        });
        Some(self.restore(entry))
    }

    fn restore(&mut self, entry: Entry) -> State {
        self.revision = entry.revision;
        self.generation += 1;
        entry.state
    }

    pub fn enforce_tab_limits(
        &mut self,
        current: &DocumentContent,
        selection: &Selection,
        limits: &Limits,
    ) {
        while self.undo.len() + self.redo.len() > limits.entries && self.evict_oldest() {}
        while self.retained_bytes(current, selection) > limits.tab_bytes && self.evict_oldest() {}
    }

    // Shared pixels and masks count once; path and text metadata is owned by each snapshot.
    pub fn retained_bytes(&self, current: &DocumentContent, selection: &Selection) -> u64 {
        let live: HashSet<*const RgbaImage> = current
            .layers()
            .iter()
            .map(|layer| Arc::as_ptr(&layer.pixels))
            .collect();
        let mut seen = HashSet::new();
        let entries = || self.undo.iter().chain(&self.redo);
        let layer_bytes: u64 = entries()
            .flat_map(|entry| entry.state.content.layers())
            .filter(|layer| {
                let pixels = Arc::as_ptr(&layer.pixels);
                !live.contains(&pixels) && seen.insert(pixels)
            })
            .map(|layer| layer.pixels.as_raw().len() as u64)
            .sum();
        let mut seen_masks = HashSet::new();
        if let Some((pixels, _)) = selection.mask_storage() {
            seen_masks.insert(pixels);
        }
        let mask_bytes: u64 = entries()
            .filter_map(|entry| entry.state.selection.mask_storage())
            .filter_map(|(pixels, bytes)| seen_masks.insert(pixels).then_some(bytes))
            .sum();
        let metadata_bytes: u64 = entries()
            .map(|entry| {
                let content = &entry.state.content;
                let paths = content.paths();
                let paths_bytes = std::mem::size_of_val(paths) as u64
                    + paths
                        .iter()
                        .map(|path| std::mem::size_of_val(path.anchors.as_slice()) as u64)
                        .sum::<u64>();
                let text_bytes: u64 = content
                    .layers()
                    .iter()
                    .filter_map(|layer| layer.text.as_ref())
                    .map(|text| {
                        (text.source.len() + text.family.len() + text.resolved_family.len()) as u64
                    })
                    .sum();
                paths_bytes + text_bytes
            })
            .sum();
        layer_bytes + mask_bytes + metadata_bytes
    }

    fn oldest_stamp(&self) -> Option<u64> {
        let undo = self.undo.front().map(|entry| entry.stamp);
        let redo = self.redo.first().map(|entry| entry.stamp);
        undo.into_iter().chain(redo).min()
    }

    fn evict_oldest(&mut self) -> bool {
        match (self.undo.front(), self.redo.first()) {
            (Some(undo), Some(redo)) if redo.stamp < undo.stamp => {
                self.redo.remove(0);
            }
            (Some(_), _) => {
                self.undo.pop_front();
            }
            (None, Some(_)) => {
                self.redo.remove(0);
            }
            (None, None) => return false,
        }
        true
    }
}

pub(super) fn enforce_app_budget<'a>(
    histories: impl IntoIterator<Item = (&'a mut History, &'a DocumentContent, &'a Selection)>,
    limits: &Limits,
) {
    let mut histories: Vec<_> = histories.into_iter().collect();
    loop {
        let total: u64 = histories
            .iter()
            .map(|(history, current, selection)| history.retained_bytes(current, selection))
            .sum();
        if total <= limits.app_bytes {
            return;
        }
        let Some((history, _, _)) = histories
            .iter_mut()
            .filter(|(history, _, _)| history.oldest_stamp().is_some())
            .min_by_key(|(history, _, _)| history.oldest_stamp())
        else {
            return;
        };
        history.evict_oldest();
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::document::CropRect;

    struct Session {
        history: History,
        current: State,
        clock: u64,
    }

    impl Session {
        fn new(saved: bool, width: u32) -> Self {
            let content =
                DocumentContent::single_layer("Layer".into(), RgbaImage::new(width, width));
            Self {
                history: History::new(saved),
                current: State {
                    active_layer: content.top_layer_id(),
                    content,
                    selection: Selection::Unrestricted,
                },
                clock: 0,
            }
        }

        fn tick(&mut self) -> u64 {
            self.clock += 1;
            self.clock
        }

        fn edit(&mut self, edit: impl FnOnce(&mut DocumentContent) -> bool, limits: &Limits) {
            let mut next = self.current.clone();
            if edit(&mut next.content) {
                let previous = std::mem::replace(&mut self.current, next);
                let stamp = self.tick();
                self.history.commit(previous, stamp, true);
                self.history.enforce_tab_limits(
                    &self.current.content,
                    &self.current.selection,
                    limits,
                );
            }
        }

        fn fill(&mut self, color: u8, limits: &Limits) {
            let layer = self.current.active_layer;
            self.edit(
                |content| {
                    content
                        .fill(
                            layer,
                            0,
                            0,
                            Rgba([color, 0, 0, 255]),
                            &Selection::Unrestricted,
                            0,
                        )
                        .is_some()
                },
                limits,
            );
        }

        fn undo(&mut self) -> bool {
            let stamp = self.tick();
            let current = self.current.clone();
            self.history
                .undo(current, stamp)
                .map(|state| self.current = state)
                .is_some()
        }

        fn redo(&mut self) -> bool {
            let stamp = self.tick();
            let current = self.current.clone();
            self.history
                .redo(current, stamp)
                .map(|state| self.current = state)
                .is_some()
        }

        fn pixel(&self) -> [u8; 4] {
            self.current.content.composite().get_pixel(0, 0).0
        }
    }

    #[test]
    fn undo_and_redo_a_crop_then_a_fill_back_to_the_saved_revision() {
        let mut session = Session::new(true, 4);
        assert!(!session.history.is_dirty());
        let generation = session.history.generation();

        session.edit(
            |content| content.crop(CropRect::between((1, 1), (2, 2))),
            &LIMITS,
        );
        session.fill(200, &LIMITS);
        assert!(session.history.is_dirty());
        assert_eq!(session.current.content.size(), (2, 2));
        assert_eq!(session.pixel(), [200, 0, 0, 255]);

        assert!(session.undo());
        assert_eq!(session.pixel(), [0; 4]);
        assert_eq!(session.current.content.size(), (2, 2));
        assert!(session.undo());
        assert_eq!(session.current.content.size(), (4, 4));
        assert!(!session.history.is_dirty());
        assert!(!session.undo());

        assert!(session.redo());
        assert!(session.redo());
        assert_eq!(session.pixel(), [200, 0, 0, 255]);
        assert!(session.history.is_dirty());
        assert!(!session.redo());
        assert_eq!(session.history.generation(), generation + 6);
    }

    #[test]
    fn saving_after_undo_and_branching_by_editing() {
        let mut session = Session::new(false, 2);
        assert!(session.history.is_dirty());
        session.fill(10, &LIMITS);
        session.fill(20, &LIMITS);
        assert!(session.undo());
        session.history.mark_saved(session.history.revision());
        assert!(!session.history.is_dirty());

        assert!(session.redo());
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert!(!session.history.is_dirty());

        session.fill(30, &LIMITS);
        assert!(session.history.redo_target().is_none());
        assert!(!session.redo());
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert!(!session.history.is_dirty());
        assert_eq!(session.pixel(), [10, 0, 0, 255]);
    }

    #[test]
    fn a_no_op_edit_creates_no_entry() {
        let mut session = Session::new(true, 2);
        let generation = session.history.generation();
        session.edit(|_| false, &LIMITS);
        assert!(session.history.undo_target().is_none());
        assert_eq!(session.history.generation(), generation);
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn entry_cap_evicts_the_oldest_and_keeps_the_saved_identity() {
        let limits = Limits {
            entries: 3,
            ..LIMITS
        };
        let mut session = Session::new(true, 1);
        for color in 1..=5 {
            session.fill(color, &limits);
        }
        let mut undone = 0;
        while session.undo() {
            undone += 1;
        }
        assert_eq!(undone, 3);
        assert_eq!(session.pixel(), [2, 0, 0, 255]);
        assert!(session.history.is_dirty());

        while session.redo() {}
        session.history.mark_saved(session.history.revision());
        session.fill(6, &limits);
        assert!(session.undo());
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn byte_budget_charges_shared_pixels_once_and_keeps_the_current_state() {
        // Each 2 × 2 layer holds 16 bytes.
        let mut session = Session::new(true, 2);
        let shared = session.current.clone();
        session.history.commit(shared.clone(), 1, true);
        session.history.commit(shared, 2, true);
        assert_eq!(
            session
                .history
                .retained_bytes(&session.current.content, &session.current.selection),
            0
        );

        let limits = Limits {
            tab_bytes: 32,
            ..LIMITS
        };
        let mut session = Session::new(true, 2);
        for color in 1..=4 {
            session.fill(color, &limits);
        }
        assert_eq!(
            session
                .history
                .retained_bytes(&session.current.content, &session.current.selection),
            32
        );
        let mut undone = 0;
        while session.undo() {
            undone += 1;
        }
        assert_eq!(undone, 2);

        let tiny = Limits {
            tab_bytes: 8,
            ..LIMITS
        };
        session.fill(9, &tiny);
        assert!(session.history.undo_target().is_none());
        assert!(session.history.redo_target().is_none());
        assert_eq!(session.pixel(), [9, 0, 0, 255]);
    }

    #[test]
    fn history_charges_a_shared_selection_mask_once() {
        let mut session = Session::new(true, 4);
        let mut snapshot = session.current.clone();
        snapshot.selection = Selection::rectangle((1, 1), (1, 1), 4, 4).feather(1, 4, 4);
        let bytes = snapshot.selection.mask_storage().unwrap().1;
        session.history.commit(snapshot.clone(), 1, false);
        session.history.commit(snapshot.clone(), 2, false);
        assert_eq!(
            session
                .history
                .retained_bytes(&session.current.content, &session.current.selection),
            bytes
        );
        assert_eq!(
            session
                .history
                .retained_bytes(&session.current.content, &snapshot.selection),
            0
        );
    }

    #[test]
    fn app_budget_evicts_the_oldest_entry_across_tabs() {
        let limits = Limits {
            app_bytes: 32,
            ..LIMITS
        };
        let mut first = Session::new(true, 2);
        let mut second = Session::new(true, 2);
        second.clock = 100;
        first.fill(1, &LIMITS);
        second.fill(2, &LIMITS);
        first.fill(3, &LIMITS);
        enforce_app_budget(
            [
                (
                    &mut first.history,
                    &first.current.content,
                    &first.current.selection,
                ),
                (
                    &mut second.history,
                    &second.current.content,
                    &second.current.selection,
                ),
            ],
            &limits,
        );
        assert_eq!(
            first
                .history
                .retained_bytes(&first.current.content, &first.current.selection),
            16
        );
        assert_eq!(
            second
                .history
                .retained_bytes(&second.current.content, &second.current.selection),
            16
        );
        assert!(first.undo());
        assert_eq!(first.pixel(), [1, 0, 0, 255]);
        assert!(!first.undo());
        assert!(second.undo());
    }

    #[test]
    fn a_stale_completion_is_detected_by_generation() {
        let mut session = Session::new(true, 2);
        session.fill(1, &LIMITS);
        let started = session.history.generation();
        assert!(session.undo());
        assert_ne!(session.history.generation(), started);
    }

    #[test]
    fn selection_history_preserves_the_persistent_revision() {
        let mut session = Session::new(true, 2);
        let saved = session.history.revision();
        let generation = session.history.generation();
        let previous = session.current.clone();
        session.current.selection = Selection::Full;
        let stamp = session.tick();
        session.history.commit(previous, stamp, false);
        assert_eq!(session.history.revision(), saved);
        assert_ne!(session.history.generation(), generation);
        assert!(!session.history.is_dirty());

        session.fill(1, &LIMITS);
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert_eq!(session.current.selection, Selection::Full);
        assert!(!session.history.is_dirty());
        assert!(session.undo());
        assert_eq!(session.current.selection, Selection::Unrestricted);
        assert!(!session.history.is_dirty());
        assert!(session.redo());
        assert_eq!(session.current.selection, Selection::Full);
        assert!(!session.history.is_dirty());
        assert!(session.redo());
        assert!(session.history.is_dirty());
    }

    #[test]
    fn undoing_a_stroke_restores_every_painted_pixel() {
        use crate::paint::{Brush, Stroke, StrokeKind};
        use crate::preview::Dirty;

        let mut session = Session::new(true, 16);
        let id = session.current.active_layer;
        let selection = Selection::Rect(crate::preview::PixelRect::new(0, 0, 16, 1));
        session.current.selection = selection.clone();
        let image = session.current.content.layer(id).unwrap().pixels.clone();
        let mut stroke = Stroke::new(
            StrokeKind::Paint,
            Rgba([0, 0, 0, 255]),
            Brush {
                diameter: 1,
                spacing: 100,
                hardness: 100,
                opacity: 100,
            },
        );
        stroke.sample(0.5, 0.5, &image, |_, _| 255);
        stroke.sample(8.5, 0.5, &image, |_, _| 255);
        session.edit(
            |content| content.apply_stroke(id, &stroke) != Dirty::None,
            &LIMITS,
        );
        let painted = session.current.content.layer(id).unwrap();
        assert_eq!(painted.pixels.get_pixel(0, 0).0[3], 255);
        assert_eq!(painted.pixels.get_pixel(8, 0).0[3], 255);
        assert_eq!(session.current.selection, selection);
        assert!(session.undo());
        let restored = session.current.content.layer(id).unwrap();
        assert_eq!(restored.pixels.get_pixel(0, 0).0[3], 0);
        assert_eq!(restored.pixels.get_pixel(8, 0).0[3], 0);
        assert_eq!(session.current.selection, selection);
    }

    #[test]
    fn placing_a_rotated_layer_keeps_the_selection_and_undoes() {
        use crate::preview::Dirty;
        use crate::transform::{Resample, Warp, warp_layer};

        let mut session = Session::new(true, 4);
        let id = session.current.active_layer;
        let selection = Selection::Rect(crate::preview::PixelRect::new(1, 1, 2, 2));
        session.current.selection = selection.clone();
        let mut image = image::RgbaImage::new(3, 1);
        image.put_pixel(0, 0, Rgba([10, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([20, 0, 0, 255]));
        image.put_pixel(2, 0, Rgba([30, 0, 0, 255]));
        let placed =
            warp_layer(&image, (0, 0), Warp::Rotate(90.0), Resample::Nearest, None).unwrap();
        let offset = placed.offset;
        session.edit(
            |content| content.place_layer(id, placed.pixels, offset) != Dirty::None,
            &LIMITS,
        );
        assert_eq!(session.current.content.layer(id).unwrap().offset, (1, -1));
        assert_eq!(
            session
                .current
                .content
                .layer(id)
                .unwrap()
                .pixels
                .dimensions(),
            (1, 3)
        );
        assert_eq!(session.current.selection, selection);
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert_eq!(session.current.content.layer(id).unwrap().offset, (0, 0));
        assert_eq!(
            session
                .current
                .content
                .layer(id)
                .unwrap()
                .pixels
                .dimensions(),
            (4, 4)
        );
        assert_eq!(session.current.selection, selection);
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn undoing_a_layer_move_restores_the_offset_and_keeps_the_selection() {
        let mut session = Session::new(true, 4);
        let id = session.current.active_layer;
        session.current.selection = Selection::Rect(crate::preview::PixelRect::new(1, 1, 1, 1));
        session.edit(
            |content| content.set_offset(id, (2, -1)) != crate::preview::Dirty::None,
            &LIMITS,
        );
        assert_eq!(session.current.content.layer(id).unwrap().offset, (2, -1));
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert_eq!(session.current.content.layer(id).unwrap().offset, (0, 0));
        assert_eq!(
            session.current.selection,
            Selection::Rect(crate::preview::PixelRect::new(1, 1, 1, 1))
        );
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn undoing_a_pasted_layer_restores_a_saved_document() {
        let mut session = Session::new(true, 4);
        let id = session.current.active_layer;
        session.current.selection = Selection::Rect(crate::preview::PixelRect::new(0, 0, 2, 2));
        session.edit(
            |content| {
                content
                    .insert_centered_layers(
                        id,
                        vec![crate::document::ImageLayer {
                            name: "Pasted Layer".into(),
                            pixels: image::RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 128])),
                        }],
                    )
                    .added
                    > 0
            },
            &LIMITS,
        );
        assert_eq!(session.current.content.layers().len(), 2);
        assert_eq!(session.current.content.layers()[1].offset, (1, 1));
        assert!(session.history.is_dirty());
        assert!(session.undo());
        assert_eq!(session.current.content.layers().len(), 1);
        assert_eq!(
            session.current.selection,
            Selection::Rect(crate::preview::PixelRect::new(0, 0, 2, 2))
        );
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn undoing_a_canvas_resize_restores_the_saved_document() {
        let mut session = Session::new(true, 4);
        session.current.selection = Selection::Rect(crate::preview::PixelRect::new(0, 0, 2, 1));
        session.edit(
            |content| {
                content
                    .resize_canvas(6, 4, crate::transform::Anchor::East)
                    .is_some()
            },
            &LIMITS,
        );
        assert_eq!(session.current.content.size(), (6, 4));
        assert_eq!(
            session
                .current
                .content
                .layer(session.current.active_layer)
                .unwrap()
                .offset,
            (2, 0)
        );
        assert!(session.undo());
        assert_eq!(session.current.content.size(), (4, 4));
        assert_eq!(
            session
                .current
                .content
                .layer(session.current.active_layer)
                .unwrap()
                .offset,
            (0, 0)
        );
        assert_eq!(
            session.current.selection,
            Selection::Rect(crate::preview::PixelRect::new(0, 0, 2, 1))
        );
        assert!(!session.history.is_dirty());
    }

    #[test]
    fn undoing_a_gradient_restores_every_pixel_and_keeps_the_selection() {
        use crate::paint::{Gradient, GradientBlend, GradientShape};
        use crate::preview::Dirty;

        let mut session = Session::new(true, 4);
        let selection = Selection::Rect(crate::preview::PixelRect::new(1, 0, 2, 1));
        session.current.selection = selection.clone();
        let layer = session.current.active_layer;
        let gradient = Gradient {
            shape: GradientShape::Linear,
            blend: GradientBlend::ForegroundToBackground,
            start: (1.5, 0.5),
            end: (2.5, 0.5),
            foreground: Rgba([255, 0, 0, 255]),
            background: Rgba([0, 0, 255, 255]),
        };
        session.edit(
            |content| content.apply_gradient(layer, gradient, &selection) != Dirty::None,
            &LIMITS,
        );
        let pixels = session.current.content.layer(layer).unwrap().pixels.clone();
        assert_eq!(pixels.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(pixels.get_pixel(1, 0).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(2, 0).0, [0, 0, 255, 255]);
        assert_eq!(pixels.get_pixel(3, 0).0, [0, 0, 0, 0]);
        assert!(session.undo());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(1, 0)
                .0,
            [0, 0, 0, 0]
        );
        assert_eq!(session.current.selection, selection);
    }

    #[test]
    fn an_identity_adjustment_keeps_the_saved_document_and_invert_undoes() {
        use crate::adjustments::Adjustment;
        use crate::preview::Dirty;

        let mut session = Session::new(true, 2);
        let layer = session.current.active_layer;
        let selection = Selection::Unrestricted;
        session.edit(
            |content| {
                content.apply_adjustment(
                    layer,
                    Adjustment::BrightnessContrast {
                        brightness: 0,
                        contrast: 0,
                    },
                    &selection,
                ) != Dirty::None
            },
            &LIMITS,
        );
        assert!(!session.history.is_dirty());
        session.edit(
            |content| {
                content.apply_adjustment(layer, Adjustment::Invert, &selection) != Dirty::None
            },
            &LIMITS,
        );
        assert!(session.history.is_dirty());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            [255, 255, 255, 0]
        );
        assert!(session.undo());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 0]
        );
        assert!(!session.history.is_dirty());
        assert_eq!(session.current.selection, selection);
    }

    #[test]
    fn an_identity_levels_keeps_the_saved_document_and_a_curve_undoes() {
        use crate::adjustments::{Adjustment, Curve, Curves, Levels, MapChannel};
        use crate::preview::Dirty;

        let mut session = Session::new(true, 2);
        let layer = session.current.active_layer;
        let selection = Selection::Unrestricted;
        session.edit(
            |content| {
                content.apply_adjustment(layer, Adjustment::Levels(Levels::default()), &selection)
                    != Dirty::None
            },
            &LIMITS,
        );
        assert!(!session.history.is_dirty());
        let curves = Curves::default().set_curve(
            MapChannel::Value,
            Curve::identity().move_point(0, 0, 255).move_point(1, 0, 0),
        );
        session.edit(
            |content| {
                content.apply_adjustment(layer, Adjustment::Curves(curves), &selection)
                    != Dirty::None
            },
            &LIMITS,
        );
        assert!(session.history.is_dirty());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            [255, 255, 255, 0]
        );
        assert!(session.undo());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 0]
        );
        assert!(!session.history.is_dirty());
        assert_eq!(session.current.selection, selection);
    }

    #[test]
    fn an_identity_blur_keeps_the_saved_document_and_a_shadow_undoes() {
        use crate::filters::FilterSpec;
        use crate::preview::{Dirty, PixelRect};
        use image::Rgba;

        let mut session = Session::new(true, 2);
        let layer = session.current.active_layer;
        let selection = Selection::Unrestricted;
        session.edit(
            |content| {
                content.apply_filter(layer, FilterSpec::Blur { radius: 0 }, &selection)
                    != Dirty::None
            },
            &LIMITS,
        );
        assert!(!session.history.is_dirty());
        session.fill(200, &LIMITS);
        session.edit(
            |content| {
                content.apply_adjustment(
                    layer,
                    crate::adjustments::Adjustment::Invert,
                    &Selection::Rect(PixelRect::new(0, 0, 1, 1)),
                ) != Dirty::None
            },
            &LIMITS,
        );
        let before = session.current.content.layer(layer).unwrap().pixels.clone();
        session.edit(
            |content| {
                content.apply_filter(layer, FilterSpec::Blur { radius: 1 }, &selection)
                    != Dirty::None
            },
            &LIMITS,
        );
        assert!(session.history.is_dirty());
        assert_ne!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            before.get_pixel(0, 0).0
        );
        assert!(session.undo());
        assert_eq!(
            session
                .current
                .content
                .layer(layer)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            before.get_pixel(0, 0).0
        );

        let mut clean = Session::new(true, 2);
        clean.current.content = DocumentContent::single_layer(
            "Source".into(),
            image::RgbaImage::from_pixel(2, 2, Rgba([100, 100, 100, 255])),
        );
        let source = clean.current.active_layer;
        clean.edit(
            |content| {
                content
                    .place_drop_shadow(
                        source,
                        FilterSpec::Shadow {
                            radius: 0,
                            offset: (1, -1),
                            opacity: 100,
                            color: Rgba([0, 0, 0, 255]),
                        },
                    )
                    .is_ok()
            },
            &LIMITS,
        );
        assert_eq!(clean.current.content.layers().len(), 2);
        assert!(clean.history.is_dirty());
        assert!(clean.undo());
        assert_eq!(clean.current.content.layers().len(), 1);
        assert!(!clean.history.is_dirty());
        assert_eq!(clean.current.active_layer, source);
    }

    #[test]
    fn moving_a_handle_undoes_and_reopening_keeps_the_ids() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};

        let mut session = Session::new(true, 8);
        let open = VectorPath {
            id: PathId(2),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor::corner(AnchorId(5), (1.0, 1.0)),
                Anchor::corner(AnchorId(6), (4.0, 1.0)),
            ],
        };
        session.edit(|content| content.upsert_path(open), &LIMITS);
        assert!(session.history.is_dirty());
        let mut moved = session.current.content.paths()[0].clone();
        moved.anchors[1].handle_in = (3.0, 2.5);
        session.edit(|content| content.upsert_path(moved), &LIMITS);
        assert_eq!(
            session.current.content.paths()[0].anchors[1].handle_in,
            (3.0, 2.5)
        );
        assert!(session.undo());
        assert_eq!(
            session.current.content.paths()[0].anchors[1].handle_in,
            (4.0, 1.0)
        );
        assert_eq!(session.current.content.paths()[0].id, PathId(2));
        assert_eq!(
            session.current.content.paths()[0].anchors[0].id,
            AnchorId(5)
        );
        session.edit(
            |content| content.close_path(PathId(2)) || content.paths()[0].anchors.len() < 3,
            &LIMITS,
        );
        let mut closed = session.current.content.paths()[0].clone();
        closed.anchors.push(Anchor::corner(AnchorId(7), (2.0, 4.0)));
        closed.closed = true;
        session.edit(|content| content.upsert_path(closed), &LIMITS);
        assert!(session.current.content.paths()[0].closed);
        assert!(session.undo());
        assert!(!session.current.content.paths()[0].closed);
    }

    #[test]
    fn undoing_a_path_stroke_restores_pixels_and_keeps_the_path() {
        use crate::paint::Brush;
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};
        use crate::preview::{Dirty, PixelRect};

        let mut session = Session::new(true, 12);
        let id = session.current.active_layer;
        let selection = Selection::Rect(PixelRect::new(1, 1, 3, 3));
        session.current.selection = selection.clone();
        let path = VectorPath {
            id: PathId(4),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor::corner(AnchorId(1), (2.0, 2.0)),
                Anchor::corner(AnchorId(2), (2.0, 2.0)),
                Anchor::corner(AnchorId(3), (8.0, 2.0)),
            ],
        };
        session.edit(|content| content.upsert_path(path), &LIMITS);
        let brush = Brush {
            diameter: 1,
            spacing: 100,
            hardness: 100,
            opacity: 100,
        };
        session.edit(
            |content| {
                let line = crate::paths::stroke(content.active_path().unwrap());
                content.stroke_lines(id, std::slice::from_ref(&line), Rgba([0, 0, 0, 255]), brush)
                    != Dirty::None
            },
            &LIMITS,
        );
        assert_eq!(
            session
                .current
                .content
                .layer(id)
                .unwrap()
                .pixels
                .get_pixel(4, 2)
                .0[3],
            255
        );
        assert_eq!(session.current.selection, selection);
        assert!(session.undo());
        assert_eq!(
            session
                .current
                .content
                .layer(id)
                .unwrap()
                .pixels
                .get_pixel(4, 2)
                .0[3],
            0
        );
        assert_eq!(session.current.content.paths().len(), 1);
        assert_eq!(session.current.content.paths()[0].anchors.len(), 3);
        assert_eq!(session.current.selection, selection);
    }

    #[test]
    fn undoing_rasterize_restores_the_text_source_and_pixels() {
        use crate::preview::Dirty;

        let rendered = crate::text::render("Hi", "sans-serif", 24.0, Rgba([0, 0, 0, 255])).unwrap();
        let pixels = rendered.image.clone();
        let mut session = Session::new(true, 48);
        let id = session.current.active_layer;
        session.edit(
            |content| content.set_text(id, (4, 4), rendered) != Dirty::None,
            &LIMITS,
        );
        assert!(session.current.content.layer_is_text(id));
        session.edit(|content| content.rasterize_text(id), &LIMITS);
        assert!(session.current.content.layer(id).unwrap().text.is_none());
        assert_eq!(session.current.content.layer(id).unwrap().pixels, pixels);
        assert!(session.undo());
        let restored = session.current.content.layer(id).unwrap();
        assert_eq!(restored.text.as_ref().unwrap().source, "Hi");
        assert_eq!(restored.pixels, pixels);

        let mut plain = Session::new(true, 4);
        let plain_id = plain.current.active_layer;
        plain.edit(|content| content.rasterize_text(plain_id), &LIMITS);
        assert!(!plain.history.is_dirty());
    }

    #[test]
    fn history_budget_includes_owned_path_and_text_metadata() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};
        let mut session = Session::new(true, 4);
        let id = session.current.active_layer;
        session.current.content.upsert_path(VectorPath {
            id: PathId(0),
            visible: true,
            closed: false,
            anchors: vec![Anchor::corner(AnchorId(0), (1.0, 1.0))],
        });
        let rendered =
            crate::text::render("Some text", "sans-serif", 12.0, Rgba([0, 0, 0, 255])).unwrap();
        session.current.content.set_text(id, (0, 0), rendered);
        let snapshot = session.current.clone();
        session.history.commit(snapshot.clone(), 1, true);
        let once = session
            .history
            .retained_bytes(&session.current.content, &session.current.selection);
        assert!(once >= "Some text".len() as u64 + std::mem::size_of::<Anchor>() as u64);
        session.history.commit(snapshot, 2, true);
        assert_eq!(
            session
                .history
                .retained_bytes(&session.current.content, &session.current.selection),
            2 * once
        );
        session.history.enforce_tab_limits(
            &session.current.content,
            &session.current.selection,
            &Limits {
                tab_bytes: once,
                ..LIMITS
            },
        );
        assert_eq!(session.history.undo.len(), 1);
        assert!(session.current.content.layer_is_text(id));
    }
}
