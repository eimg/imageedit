use super::Workspace;
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
    last: Vec<(usize, u64, u64)>,
    alive: HashMap<usize, Arc<AtomicBool>>,
    reported_error: bool,
    originals: HashMap<usize, std::path::PathBuf>,
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
            || !(self
                .tabs
                .iter()
                .any(|tab| tab.dirty() || tab.pending_text.is_some())
                || self.text_draft.is_some())
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
                    || tab.pending_text.is_some()
                    || self
                        .text_draft
                        .as_ref()
                        .is_some_and(|draft| draft.recovery_tab() == tab.id)
            })
            .map(|tab| {
                (
                    tab.id,
                    tab.history.generation(),
                    if self
                        .text_draft
                        .as_ref()
                        .is_some_and(|draft| draft.recovery_tab() == tab.id)
                    {
                        self.text_generation
                    } else {
                        0
                    },
                )
            })
            .collect();
        if signatures == self.recovery.last {
            return;
        }
        let mut snapshots = Vec::new();
        for &(id, _, _) in &signatures {
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
                continue;
            };
            let alive = self
                .recovery
                .alive
                .entry(id)
                .or_insert_with(|| Arc::new(AtomicBool::new(true)))
                .clone();
            let draft = self
                .text_draft
                .as_ref()
                .filter(|draft| draft.recovery_tab() == id)
                .cloned()
                .or_else(|| tab.pending_text.clone());
            snapshots.push((
                id,
                tab.document.name.clone(),
                tab.document.content.clone(),
                tab.active_layer,
                draft,
                alive,
                self.recovery.originals.get(&id).cloned(),
            ));
        }
        // Saved documents no longer need a recovery copy. No saved revision is changed.
        let saved: Vec<_> = self
            .recovery
            .alive
            .keys()
            .copied()
            .filter(|id| !signatures.iter().any(|signature| signature.0 == *id))
            .collect();
        for id in saved {
            self.remove_recovery_tab(id, cx);
        }
        self.recovery.busy = true;
        let job = cx.background_executor().spawn(async move {
            for (id, name, mut content, mut active, draft, alive, original) in snapshots {
                let text =
                    draft.and_then(|draft| draft.recovery_content(&mut content, &mut active));
                store.snapshot(id, &name, &content, text.as_ref(), &alive)?;
                if let Some(original) = original {
                    for path in [
                        original.clone(),
                        original.with_extension("name"),
                        original.with_extension("text.json"),
                    ] {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, _, cx| {
                workspace.recovery.busy = false;
                match result {
                    Ok(()) => {
                        workspace.recovery.last = signatures;
                        workspace.recovery.reported_error = false;
                    }
                    Err(error) if !workspace.recovery.reported_error => {
                        workspace
                            .errors
                            .push(format!("Could not update recovery copy: {error:#}"));
                        workspace.recovery.reported_error = true;
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn remove_recovery_tab(&mut self, id: usize, cx: &Context<Self>) {
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
