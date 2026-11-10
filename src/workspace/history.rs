use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

use image::RgbaImage;

use crate::document::{DocumentContent, LayerId};

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

    pub fn undo_target(&self) -> Option<&State> {
        self.undo.back().map(|entry| &entry.state)
    }

    pub fn redo_target(&self) -> Option<&State> {
        self.redo.last().map(|entry| &entry.state)
    }

    pub fn commit(&mut self, previous: State, stamp: u64) {
        self.redo.clear();
        self.undo.push_back(Entry {
            state: previous,
            revision: self.revision,
            stamp,
        });
        self.revision = Revision(self.next_revision);
        self.next_revision += 1;
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

    pub fn enforce_tab_limits(&mut self, current: &DocumentContent, limits: &Limits) {
        while self.undo.len() + self.redo.len() > limits.entries && self.evict_oldest() {}
        while self.retained_bytes(current) > limits.tab_bytes && self.evict_oldest() {}
    }

    // Counts pixel buffers that only history keeps alive, charging each shared buffer once.
    pub fn retained_bytes(&self, current: &DocumentContent) -> u64 {
        let live: HashSet<*const RgbaImage> = current
            .layers()
            .iter()
            .map(|layer| Arc::as_ptr(&layer.pixels))
            .collect();
        let mut seen = HashSet::new();
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|entry| entry.state.content.layers())
            .filter(|layer| {
                let pixels = Arc::as_ptr(&layer.pixels);
                !live.contains(&pixels) && seen.insert(pixels)
            })
            .map(|layer| layer.pixels.as_raw().len() as u64)
            .sum()
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
    histories: impl IntoIterator<Item = (&'a mut History, &'a DocumentContent)>,
    limits: &Limits,
) {
    let mut histories: Vec<_> = histories.into_iter().collect();
    loop {
        let total: u64 = histories
            .iter()
            .map(|(history, current)| history.retained_bytes(current))
            .sum();
        if total <= limits.app_bytes {
            return;
        }
        let Some((history, _)) = histories
            .iter_mut()
            .filter(|(history, _)| history.oldest_stamp().is_some())
            .min_by_key(|(history, _)| history.oldest_stamp())
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
                self.history.commit(previous, stamp);
                self.history
                    .enforce_tab_limits(&self.current.content, limits);
            }
        }

        fn fill(&mut self, color: u8, limits: &Limits) {
            let layer = self.current.active_layer;
            self.edit(
                |content| content.fill(layer, 0, 0, Rgba([color, 0, 0, 255])),
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
        session.history.commit(shared.clone(), 1);
        session.history.commit(shared, 2);
        assert_eq!(session.history.retained_bytes(&session.current.content), 0);

        let limits = Limits {
            tab_bytes: 32,
            ..LIMITS
        };
        let mut session = Session::new(true, 2);
        for color in 1..=4 {
            session.fill(color, &limits);
        }
        assert_eq!(session.history.retained_bytes(&session.current.content), 32);
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
                (&mut first.history, &first.current.content),
                (&mut second.history, &second.current.content),
            ],
            &limits,
        );
        assert_eq!(first.history.retained_bytes(&first.current.content), 16);
        assert_eq!(second.history.retained_bytes(&second.current.content), 16);
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
}
