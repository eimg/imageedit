use super::Workspace;
use super::{
    history::Revision,
    text_ui::{RecoveryText, TextDraft},
};
use crate::document::{DocumentContent, LayerId};
use gpui::{Context, Window};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
pub(super) struct Recovery {
    initialized: bool,
    store: Option<Arc<crate::recovery::Store>>,
    busy: bool,
    timer: bool,
    last: HashMap<usize, Signature>,
    alive: HashMap<usize, Arc<AtomicBool>>,
    reported_error: bool,
    originals: HashMap<usize, std::path::PathBuf>,
}

#[derive(Clone, PartialEq)]
struct Signature {
    revision: Revision,
    text: Option<RecoveryText>,
}

struct Snapshot {
    id: usize,
    signature: Signature,
    name: String,
    content: DocumentContent,
    active: LayerId,
    draft: Option<TextDraft>,
    alive: Arc<AtomicBool>,
    original: Option<std::path::PathBuf>,
}

struct Outcome {
    id: usize,
    signature: Signature,
    alive: Arc<AtomicBool>,
    result: Result<(), String>,
}

fn write_snapshots(store: &crate::recovery::Store, snapshots: Vec<Snapshot>) -> Vec<Outcome> {
    snapshots
        .into_iter()
        .map(|mut snapshot| {
            let text = snapshot.draft.and_then(|draft| {
                draft.recovery_content(&mut snapshot.content, &mut snapshot.active)
            });
            let result = store
                .snapshot(
                    snapshot.id,
                    &snapshot.name,
                    &snapshot.content,
                    text.as_ref(),
                    &snapshot.alive,
                )
                .map_err(|error| {
                    format!(
                        "Could not update recovery copy for {}: {error:#}",
                        snapshot.name
                    )
                });
            if result.is_ok()
                && let Some(original) = snapshot.original
            {
                for path in [
                    original.clone(),
                    original.with_extension("name"),
                    original.with_extension("text.json"),
                ] {
                    let _ = std::fs::remove_file(path);
                }
            }
            Outcome {
                id: snapshot.id,
                signature: snapshot.signature,
                alive: snapshot.alive,
                result,
            }
        })
        .collect()
}

impl Recovery {
    fn needs_snapshot(&self, id: usize, signature: &Signature) -> bool {
        self.last.get(&id) != Some(signature)
    }

    fn finish(&mut self, outcomes: Vec<Outcome>) -> Vec<String> {
        let mut errors = Vec::new();
        for outcome in outcomes {
            // A late result cannot mark a closed/saved tab or its new lease up to date.
            if !self.alive.get(&outcome.id).is_some_and(|alive| {
                Arc::ptr_eq(alive, &outcome.alive) && alive.load(Ordering::Acquire)
            }) {
                continue;
            }
            match outcome.result {
                Ok(()) => {
                    self.last.insert(outcome.id, outcome.signature);
                }
                Err(error) => errors.push(error),
            }
        }
        errors
    }
}

impl Workspace {
    pub(super) fn refresh_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recovery.initialized {
            self.recovery.initialized = true;
            let job = cx.background_executor().spawn(async {
                crate::recovery::default_root().and_then(|root| crate::recovery::Store::open(&root))
            });
            cx.spawn_in(window, async move |workspace, cx| {
                let result = job.await;
                let _ = workspace.update_in(cx, |workspace, window, cx| {
                    match result {
                        Ok((store, documents, errors)) => {
                            workspace.recovery.store = Some(store);
                            workspace.errors.extend(errors);
                            for recovered in documents {
                                let snapshot = recovered.snapshot;
                                workspace.show_document(recovered.document, window, cx);
                                if let Some(id) = workspace.active_tab().map(|tab|tab.id) {
                                    workspace.recovery.originals.insert(id,snapshot);
                                    if let Some(text)=recovered.text { workspace.restore_text_recovery(id,text); }
                                }
                                if let Some(tab) = workspace.active.map(|index| &mut workspace.tabs[index]) {
                                    tab.notice = Some(recovered.warning.unwrap_or_else(|| "Recovered after an interrupted session. Save As to choose a destination.".into()));
                                }
                            }
                            workspace.recover_pending_text(window,cx);
                        }
                        Err(error) => workspace.errors.push(format!("Crash recovery is unavailable: {error:#}")),
                    }
                    cx.notify();
                });
            }).detach();
        }
        if self.recovery.timer
            || self.recovery.store.is_none()
            || !(self.tabs.iter().any(|tab| {
                tab.dirty() || tab.pending_text.as_ref().is_some_and(TextDraft::changed)
            }) || self.text_draft.as_ref().is_some_and(TextDraft::changed)
                || !self.recovery.alive.is_empty()
                || !self.recovery.originals.is_empty())
        {
            return;
        }
        self.recovery.timer = true;
        cx.spawn_in(window, async move |workspace, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.recovery.timer = false;
                workspace.save_recovery(window, cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn save_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovery.busy {
            return;
        }
        let Some(store) = self.recovery.store.clone() else {
            return;
        };
        let signatures: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| {
                tab.dirty()
                    || tab.pending_text.as_ref().is_some_and(TextDraft::changed)
                    || self
                        .text_draft
                        .as_ref()
                        .is_some_and(|draft| draft.recovery_tab() == tab.id && draft.changed())
            })
            .map(|tab| {
                let text = self
                    .text_draft
                    .as_ref()
                    .filter(|draft| draft.recovery_tab() == tab.id)
                    .or(tab.pending_text.as_ref())
                    .filter(|draft| draft.changed())
                    .map(TextDraft::recovery_signature);
                (
                    tab.id,
                    Signature {
                        revision: tab.history.revision(),
                        text,
                    },
                )
            })
            .collect();
        let mut snapshots = Vec::new();
        for (id, signature) in &signatures {
            if !self.recovery.needs_snapshot(*id, signature) {
                continue;
            }
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == *id) else {
                continue;
            };
            let alive = self
                .recovery
                .alive
                .entry(*id)
                .or_insert_with(|| Arc::new(AtomicBool::new(true)))
                .clone();
            let draft = self
                .text_draft
                .as_ref()
                .filter(|draft| draft.recovery_tab() == *id)
                .cloned()
                .or_else(|| tab.pending_text.clone());
            snapshots.push(Snapshot {
                id: *id,
                signature: signature.clone(),
                name: tab.document.name.clone(),
                content: tab.document.content.clone(),
                active: tab.active_layer,
                draft,
                alive,
                original: self.recovery.originals.get(id).cloned(),
            });
        }
        // Saved documents no longer need a recovery copy. No saved revision is changed.
        let saved: std::collections::HashSet<_> = self
            .recovery
            .alive
            .keys()
            .chain(self.recovery.originals.keys())
            .copied()
            .filter(|id| !signatures.iter().any(|signature| signature.0 == *id))
            .collect();
        for id in saved {
            self.remove_recovery_tab(id, cx);
        }
        if snapshots.is_empty() {
            return;
        }
        self.recovery.busy = true;
        let job = cx
            .background_executor()
            .spawn(async move { write_snapshots(&store, snapshots) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, _, cx| {
                workspace.recovery.busy = false;
                let errors = workspace.recovery.finish(result);
                if errors.is_empty() {
                    workspace.recovery.reported_error = false;
                } else if !workspace.recovery.reported_error {
                    workspace.errors.extend(errors);
                    workspace.recovery.reported_error = true;
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn remove_recovery_tab(&mut self, id: usize, cx: &Context<Self>) {
        self.recovery.last.remove(&id);
        if let Some(original) = self.recovery.originals.remove(&id) {
            cx.background_executor()
                .spawn(async move {
                    for path in [
                        original.clone(),
                        original.with_extension("name"),
                        original.with_extension("text.json"),
                    ] {
                        let _ = std::fs::remove_file(path);
                    }
                })
                .detach();
        }
        if let Some(alive) = self.recovery.alive.remove(&id) {
            alive.store(false, Ordering::Release);
        }
        if let Some(store) = self.recovery.store.clone() {
            cx.background_executor()
                .spawn(async move {
                    store.remove(id);
                })
                .detach();
        }
    }
    pub(super) fn clean_recovery_exit(&mut self) {
        for alive in self.recovery.alive.values() {
            alive.store(false, Ordering::Release);
        }
        if let Some(store) = self.recovery.store.as_ref() {
            store.clean_exit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::history::{History, State};
    use super::*;
    use crate::selection::Selection;

    #[test]
    fn recovery_retries_failed_tabs_without_rewriting_successful_or_selection_only_changes() {
        let root = tempfile::tempdir().unwrap();
        let (store, _, _) = crate::recovery::Store::open(root.path()).unwrap();
        let content = DocumentContent::single_layer("Art".into(), image::RgbaImage::new(2, 2));
        let mut history = History::new(false);
        let signature = Signature {
            revision: history.revision(),
            text: None,
        };
        let mut recovery = Recovery::default();
        for id in 0..2 {
            recovery.alive.insert(id, Arc::new(AtomicBool::new(true)));
        }
        let snapshot = |id, name: String, signature: Signature| Snapshot {
            id,
            name,
            signature,
            content: content.clone(),
            active: content.top_layer_id(),
            draft: None,
            alive: recovery.alive[&id].clone(),
            original: None,
        };
        let outcomes = write_snapshots(
            &store,
            vec![
                snapshot(0, "x".repeat(4097), signature.clone()),
                snapshot(1, "Valid".into(), signature.clone()),
            ],
        );
        assert_eq!(outcomes.len(), 2);
        assert_eq!(recovery.finish(outcomes).len(), 1);
        assert!(recovery.needs_snapshot(0, &signature));
        assert!(!recovery.needs_snapshot(1, &signature));
        // Selection history advances its generation, but not the persisted revision.
        history.commit(
            State {
                content: content.clone(),
                active_layer: content.top_layer_id(),
                selection: Selection::Unrestricted,
            },
            1,
            false,
        );
        assert!(!recovery.needs_snapshot(
            1,
            &Signature {
                revision: history.revision(),
                text: None
            }
        ));
        let retry = Snapshot {
            id: 0,
            signature: signature.clone(),
            name: "Repaired".into(),
            content: content.clone(),
            active: content.top_layer_id(),
            draft: None,
            alive: recovery.alive[&0].clone(),
            original: None,
        };
        assert!(
            recovery
                .finish(write_snapshots(&store, vec![retry]))
                .is_empty()
        );
        assert!(!recovery.needs_snapshot(0, &signature));
        history.commit(
            State {
                content: content.clone(),
                active_layer: content.top_layer_id(),
                selection: Selection::Unrestricted,
            },
            2,
            true,
        );
        let next = Signature {
            revision: history.revision(),
            text: None,
        };
        assert!(recovery.needs_snapshot(0, &next));
        assert!(!recovery.needs_snapshot(1, &signature));
        // A result from an expired lease cannot suppress a future retry.
        let expired = recovery
            .alive
            .insert(0, Arc::new(AtomicBool::new(true)))
            .unwrap();
        recovery.finish(vec![Outcome {
            id: 0,
            signature: next.clone(),
            alive: expired,
            result: Ok(()),
        }]);
        assert!(recovery.needs_snapshot(0, &next));
        drop(store);
        let (reopened, documents, errors) = crate::recovery::Store::open(root.path()).unwrap();
        assert!(errors.is_empty());
        assert_eq!(documents.len(), 2);
        reopened.clean_exit();
    }
}
