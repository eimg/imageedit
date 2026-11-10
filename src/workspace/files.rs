use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::{Context, PathPromptOptions, PromptButton, PromptLevel, Window};
use image::Rgba;

use super::Workspace;
use crate::document::{Document, FileFormat, file_name, save_format, save_image};

pub(super) enum SaveMode {
    Current,
    As,
    ProjectAs,
}

impl Workspace {
    pub fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.form.is_some() {
            return;
        }
        self.dialog_open = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = prompt.await;
            let _ = workspace.update_in(cx, |workspace, _, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(Ok(Some(paths))) => workspace.open_paths(paths, cx),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => workspace
                        .errors
                        .push(format!("Could not open the file picker: {error}")),
                    Err(_) => workspace
                        .errors
                        .push("The file picker closed unexpectedly.".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn create_image(
        &mut self,
        width: u32,
        height: u32,
        background: Rgba<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = format!("Untitled {}", self.next_untitled);
        self.next_untitled += 1;
        self.creating = true;
        let job = cx
            .background_executor()
            .spawn(async move { Document::blank(name, width, height, background) });
        cx.spawn_in(window, async move |workspace, cx| {
            let document = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.creating = false;
                workspace.show_document(document, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn save(&mut self, mode: SaveMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if self.dialog_open || self.form.is_some() || tab.busy() {
            return;
        }
        let id = tab.id;
        let document = &tab.document;
        if matches!(mode, SaveMode::Current)
            && let Some(path) = &document.path
        {
            if tab.dirty {
                self.write_image(id, path.clone(), document.format, window, cx);
            }
            return;
        }

        let directory = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let project_only = matches!(mode, SaveMode::ProjectAs);
        let preferred = if project_only {
            FileFormat::Project
        } else {
            document.format
        };
        let suggested = if save_format(Path::new(&document.name)).ok() == Some(preferred) {
            document.name.clone()
        } else {
            let stem = document
                .path
                .as_ref()
                .and_then(|path| path.file_stem())
                .and_then(|value| value.to_str())
                .unwrap_or(&document.name);
            format!("{stem}.{}", preferred.extension())
        };
        let prompt = cx.prompt_for_new_path(&directory, Some(&suggested));
        self.dialog_open = true;
        cx.spawn_in(window, async move |workspace, cx| {
            let result = prompt.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => match prepare_destination(&path, project_only) {
                        Ok((path, format)) => workspace.write_image(id, path, format, window, cx),
                        Err(error) => workspace
                            .errors
                            .push(format!("Could not save {}: {error}", file_name(&path))),
                    },
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => workspace
                        .errors
                        .push(format!("Could not open the save dialog: {error}")),
                    Err(_) => workspace
                        .errors
                        .push("The save dialog closed unexpectedly.".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn write_image(
        &mut self,
        id: usize,
        path: PathBuf,
        format: FileFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        if self.tabs.iter().any(|tab| {
            tab.id != id
                && (tab.document.path.as_ref() == Some(&path) || tab.saving.as_ref() == Some(&path))
        }) {
            self.errors.push(format!(
                "{} is already open in another tab.",
                file_name(&path)
            ));
            cx.notify();
            return;
        }

        let tab = &mut self.tabs[index];
        tab.saving = Some(path.clone());
        let pixels = tab.document.pixels.clone();
        let layer_name = tab.document.layer_name.clone();
        let job = cx.background_executor().spawn(async move {
            let result = save_image(&pixels, &layer_name, &path, format);
            (path, result)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let (path, result) = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                let Some(index) = workspace.tab_index(id) else {
                    return;
                };
                let tab = &mut workspace.tabs[index];
                tab.saving = None;
                let name = file_name(&path);
                match result {
                    Ok(()) => {
                        cx.add_recent_document(&path);
                        tab.document.name = name;
                        tab.document.path = Some(path);
                        tab.document.format = format;
                        tab.dirty = false;
                        workspace.update_title(window);
                    }
                    Err(error) => workspace
                        .errors
                        .push(format!("Could not save {name}: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn request_close_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.tabs[index];
        if self.dialog_open || self.form.is_some() || tab.busy() {
            return;
        }
        if !tab.dirty {
            self.close_tab(index, window, cx);
            return;
        }

        let id = tab.id;
        let message = format!("Discard unsaved changes to {}?", tab.document.name);
        self.confirm_discard(&message, window, cx, move |workspace, window, cx| {
            if let Some(index) = workspace.tab_index(id) {
                workspace.close_tab(index, window, cx);
            }
        });
    }

    pub fn request_exit(
        &mut self,
        quit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.dialog_open || self.creating || self.tabs.iter().any(|tab| tab.busy()) {
            return false;
        }
        let unsaved = self.tabs.iter().filter(|tab| tab.dirty).count();
        if unsaved == 0 {
            return true;
        }

        let message = if unsaved == 1 {
            "Discard changes to the unsaved image?".to_owned()
        } else {
            format!("Discard changes to {unsaved} unsaved images?")
        };
        self.confirm_discard(&message, window, cx, move |_, window, cx| {
            if quit {
                cx.quit();
            } else {
                window.remove_window();
            }
        });
        false
    }

    fn confirm_discard(
        &mut self,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_discard: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.dialog_open = true;
        let prompt = window.prompt(
            PromptLevel::Warning,
            message,
            Some("Cancel to keep working or save your changes."),
            &[
                PromptButton::cancel("Cancel"),
                PromptButton::new("Discard Changes"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |workspace, cx| {
            let answer = prompt.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.dialog_open = false;
                if answer == Ok(1) {
                    on_discard(workspace, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn prepare_destination(path: &Path, project_only: bool) -> Result<(PathBuf, FileFormat)> {
    let format = save_format(path)?;
    anyhow::ensure!(
        !project_only || format == FileFormat::Project,
        "Use a .imageedit filename for the project."
    );
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = path.file_name().context("Choose a file name")?;
            let directory = path.parent().context("Choose a destination folder")?;
            directory.canonicalize()?.join(name)
        }
        Err(error) => return Err(error.into()),
    };
    Ok((path, format))
}
