use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::{Context, PathPromptOptions, PromptButton, PromptLevel, Window};
use image::Rgba;

use super::Workspace;
use crate::document::{Document, FileFormat, export_image, file_name, save_format, save_image};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SaveMode {
    Current,
    As,
    ProjectAs,
}

#[derive(Debug, PartialEq, Eq)]
enum SaveRoute {
    Overwrite,
    Prompt { project_only: bool },
}

#[derive(Clone, Copy)]
enum WriteKind {
    Save(FileFormat),
    Export {
        format: FileFormat,
        jpeg_quality: u8,
    },
}

// Raster formats are offered only while they hold everything in the document.
fn save_route(mode: SaveMode, has_path: bool, format: FileFormat, raster_ok: bool) -> SaveRoute {
    let project_only = mode == SaveMode::ProjectAs || !raster_ok;
    if mode == SaveMode::Current && has_path && (format == FileFormat::Project || raster_ok) {
        SaveRoute::Overwrite
    } else {
        SaveRoute::Prompt { project_only }
    }
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
        let route = save_route(
            mode,
            document.path.is_some(),
            document.format,
            document.can_save_as_raster(),
        );
        let project_only = match route {
            SaveRoute::Overwrite => {
                if tab.dirty()
                    && let Some(path) = document.path.clone()
                {
                    let kind = WriteKind::Save(document.format);
                    self.write_file(id, path, kind, window, cx);
                }
                return;
            }
            SaveRoute::Prompt { project_only } => project_only,
        };
        let preferred = if project_only {
            FileFormat::Project
        } else {
            document.format
        };
        let suggested = suggested_name(document, preferred);
        self.prompt_destination(suggested, window, cx, move |workspace, path, window, cx| {
            match prepare_destination(&path, project_only) {
                Ok((path, format)) => {
                    workspace.write_file(id, path, WriteKind::Save(format), window, cx)
                }
                Err(error) => workspace
                    .errors
                    .push(format!("Could not save {}: {error}", file_name(&path))),
            }
        });
    }

    pub(super) fn export(
        &mut self,
        format: FileFormat,
        jpeg_quality: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if self.dialog_open || self.form.is_some() || tab.busy() {
            return;
        }
        let id = tab.id;
        let suggested = suggested_name(&tab.document, format);
        self.prompt_destination(suggested, window, cx, move |workspace, path, window, cx| {
            match prepare_export(&path, format) {
                Ok(path) => {
                    let kind = WriteKind::Export {
                        format,
                        jpeg_quality,
                    };
                    workspace.write_file(id, path, kind, window, cx)
                }
                Err(error) => workspace
                    .errors
                    .push(format!("Could not export {}: {error}", file_name(&path))),
            }
        });
    }

    fn prompt_destination(
        &mut self,
        suggested: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_path: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(document) = self.active_document() else {
            return;
        };
        let directory = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let prompt = cx.prompt_for_new_path(&directory, Some(&suggested));
        self.dialog_open = true;
        cx.spawn_in(window, async move |workspace, cx| {
            let result = prompt.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => on_path(workspace, path, window, cx),
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

    fn write_file(
        &mut self,
        id: usize,
        path: PathBuf,
        kind: WriteKind,
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
        tab.exporting = matches!(kind, WriteKind::Export { .. });
        tab.notice = None;
        let revision = tab.history.revision();
        let content = tab.document.content.clone();
        let job = cx.background_executor().spawn(async move {
            let result = match kind {
                WriteKind::Save(format) => save_image(&content, &path, format),
                WriteKind::Export {
                    format,
                    jpeg_quality,
                } => export_image(&content, &path, format, jpeg_quality),
            };
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
                tab.exporting = false;
                let name = file_name(&path);
                match (kind, result) {
                    (WriteKind::Save(format), Ok(())) => {
                        cx.add_recent_document(&path);
                        tab.document.name = name;
                        tab.document.path = Some(path);
                        tab.document.format = format;
                        tab.history.mark_saved(revision);
                        workspace.update_title(window);
                    }
                    (WriteKind::Export { .. }, Ok(())) => {
                        tab.notice = Some(format!("Exported {name}"));
                    }
                    (WriteKind::Save(_), Err(error)) => workspace
                        .errors
                        .push(format!("Could not save {name}: {error:#}")),
                    (WriteKind::Export { .. }, Err(error)) => workspace
                        .errors
                        .push(format!("Could not export {name}: {error:#}")),
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
        if !tab.dirty() {
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
        let unsaved = self.tabs.iter().filter(|tab| tab.dirty()).count();
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

fn suggested_name(document: &Document, format: FileFormat) -> String {
    if save_format(Path::new(&document.name)).ok() == Some(format) {
        return document.name.clone();
    }
    let stem = document
        .path
        .as_ref()
        .and_then(|path| path.file_stem())
        .and_then(|value| value.to_str())
        .unwrap_or(&document.name);
    format!("{stem}.{}", format.extension())
}

fn prepare_destination(path: &Path, project_only: bool) -> Result<(PathBuf, FileFormat)> {
    let format = save_format(path)?;
    anyhow::ensure!(
        !project_only || format == FileFormat::Project,
        "Use a .imageedit filename. To save a PNG, JPEG, or WebP copy, use File → Export."
    );
    Ok((canonical_destination(path)?, format))
}

fn prepare_export(path: &Path, format: FileFormat) -> Result<PathBuf> {
    anyhow::ensure!(
        save_format(path).ok() == Some(format),
        "Use a .{} filename to export a {} image.",
        format.extension(),
        format.label()
    );
    canonical_destination(path)
}

fn canonical_destination(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = path.file_name().context("Choose a file name")?;
            let directory = path.parent().context("Choose a destination folder")?;
            Ok(directory.canonicalize()?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_routes_follow_format_and_content() {
        use FileFormat::{Png, Project};
        use SaveMode::{As, Current, ProjectAs};
        let prompt = |project_only| SaveRoute::Prompt { project_only };
        assert_eq!(save_route(Current, true, Png, true), SaveRoute::Overwrite);
        assert_eq!(
            save_route(Current, true, Project, false),
            SaveRoute::Overwrite
        );
        assert_eq!(save_route(Current, true, Png, false), prompt(true));
        assert_eq!(save_route(Current, false, Project, false), prompt(true));
        assert_eq!(save_route(As, true, Png, true), prompt(false));
        assert_eq!(save_route(As, true, Png, false), prompt(true));
        assert_eq!(save_route(As, true, Project, false), prompt(true));
        assert_eq!(save_route(ProjectAs, true, Png, true), prompt(true));
    }

    #[test]
    fn destinations_reject_mismatched_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let path = |name: &str| directory.path().join(name);
        assert!(prepare_destination(&path("a.png"), true).is_err());
        assert!(prepare_destination(&path("a.txt"), false).is_err());
        let (saved, format) = prepare_destination(&path("a.png"), false).unwrap();
        assert_eq!(format, FileFormat::Png);
        assert_eq!(
            saved,
            directory.path().canonicalize().unwrap().join("a.png")
        );

        assert!(prepare_export(&path("a.imageedit"), FileFormat::Png).is_err());
        assert!(prepare_export(&path("a.png"), FileFormat::Jpeg).is_err());
        assert!(prepare_export(&path("a.jpeg"), FileFormat::Jpeg).is_ok());
        assert!(prepare_export(&path("missing/a.png"), FileFormat::Png).is_err());
    }
}
