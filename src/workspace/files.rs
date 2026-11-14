use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::{Context, PathPromptOptions, PromptButton, PromptLevel, Window};
use image::Rgba;

use super::Workspace;
use crate::document::{
    Document, FileFormat, export_image, export_pdf, file_name, save_format, save_image,
};

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
    Export(ExportFormat),
}

#[derive(Clone, Copy)]
pub(super) enum ExportFormat {
    Raster {
        format: FileFormat,
        jpeg_quality: u8,
    },
    Pdf {
        dpi: u32,
    },
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Raster { format, .. } => format.extension(),
            Self::Pdf { .. } => "pdf",
        }
    }

    fn matches_path(self, path: &Path) -> bool {
        match self {
            Self::Raster { format, .. } => save_format(path).ok() == Some(format),
            Self::Pdf { .. } => crate::pdf::is_pdf_path(path),
        }
    }
}

impl WriteKind {
    fn destination_reserved(self, source_tab: bool, open: bool, writing: bool) -> bool {
        writing || (open && (!source_tab || matches!(self, Self::Export(_))))
    }

    fn validate_document(self, document: &Document) -> Result<()> {
        anyhow::ensure!(
            !matches!(self, Self::Save(format) if format != FileFormat::Project)
                || document.can_save_as_raster(),
            "This image needs a .lime project to preserve its editing information. Use Export for a raster copy."
        );
        Ok(())
    }
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
    pub(super) fn pending_edit_name(&self) -> Option<&'static str> {
        if self.has_pending_text() {
            Some("text")
        } else if self.gradient.is_some() {
            Some("gradient")
        } else if self
            .warp
            .as_ref()
            .is_some_and(super::transform_ui::WarpSession::changed)
        {
            Some("transform")
        } else if self.crop.is_some() {
            Some("crop")
        } else if self
            .scale_drag
            .as_ref()
            .is_some_and(super::scale_ui::ScaleDrag::changed)
        {
            Some("scale")
        } else if !self.free_points.is_empty() {
            Some("free selection")
        } else if self.move_drag.is_some()
            || self.scale_pointer_dragging()
            || self.guide_drag.is_some()
            || self.stroke.is_some()
            || self.path_gesture.is_some()
            || self.opacity_drag.is_some()
        {
            Some("gesture")
        } else if self.adjustment.is_some() || self.filter.is_some() {
            Some("effect")
        } else {
            None
        }
    }

    pub(super) fn refuse_pending_file_edit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(edit) = self.pending_edit_name() else {
            return false;
        };
        if edit == "free selection" {
            self.notice(
                "Finish the free selection with Return, or cancel it with Escape, before saving or exporting.",
                cx,
            );
        } else {
            self.notice(
                &format!("Apply or discard the {edit} preview before saving or exporting."),
                cx,
            );
        }
        true
    }

    pub fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.form.is_some() || self.pdf_job.is_some() {
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
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.save(mode, window, cx));
            return;
        }
        if self.refuse_pending_file_edit(cx) {
            return;
        }
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
        format: ExportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.export(format, window, cx);
            });
            return;
        }
        if self.refuse_pending_file_edit(cx) {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        if self.dialog_open || self.form.is_some() || tab.busy() {
            return;
        }
        let id = tab.id;
        let mut suggested = suggested_export_name(&tab.document, format);
        if tab.document.path.is_some() && suggested == tab.document.name {
            let stem = Path::new(&suggested)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&suggested);
            suggested = format!("{stem}-export.{}", format.extension());
        }
        self.prompt_destination(suggested, window, cx, move |workspace, path, window, cx| {
            match prepare_export(&path, format) {
                Ok(path) => {
                    let kind = WriteKind::Export(format);
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
        if self.tabs[index].busy() {
            return;
        }
        // A pending paste/import can finish while the native save dialog is open.
        // Recheck the current content before a raster save can mark it as saved.
        if let Err(error) = kind.validate_document(&self.tabs[index].document) {
            self.errors
                .push(format!("Could not save {}: {error}", file_name(&path)));
            cx.notify();
            return;
        }
        if self.tabs.iter().any(|tab| {
            kind.destination_reserved(
                tab.id == id,
                tab.document.path.as_ref() == Some(&path),
                tab.saving.as_ref() == Some(&path),
            )
        }) {
            self.errors.push(format!(
                "{} is already open or being written by Lime.",
                file_name(&path)
            ));
            cx.notify();
            return;
        }

        let tab = &mut self.tabs[index];
        tab.saving = Some(path.clone());
        tab.exporting = matches!(kind, WriteKind::Export(_));
        tab.notice = None;
        let revision = tab.history.revision();
        let content = tab.document.content.clone();
        let job = cx.background_executor().spawn(async move {
            let result = match kind {
                WriteKind::Save(format) => save_image(&content, &path, format),
                WriteKind::Export(ExportFormat::Raster {
                    format,
                    jpeg_quality,
                }) => export_image(&content, &path, format, jpeg_quality),
                WriteKind::Export(ExportFormat::Pdf { dpi }) => export_pdf(&content, &path, dpi),
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
                    (WriteKind::Export(_), Ok(())) => {
                        tab.notice = Some(format!("Exported {name}"));
                    }
                    (WriteKind::Save(_), Err(error)) => workspace
                        .errors
                        .push(format!("Could not save {name}: {error:#}")),
                    (WriteKind::Export(_), Err(error)) => workspace
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
        if !tab.dirty() && !(self.active == Some(index) && self.pending_edit_name().is_some()) {
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
        if self.defer_settings_exit(quit, window, cx) {
            return false;
        }
        if self.gradient_store.saving {
            self.gradient_store.exit_after_save =
                Some(quit || self.gradient_store.exit_after_save.unwrap_or(false));
            return false;
        }
        if self.dialog_open
            || self.creating
            || self.pdf_job.is_some()
            || self.tabs.iter().any(|tab| tab.busy())
        {
            return false;
        }
        let unsaved = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| {
                tab.dirty() || (self.active == Some(*index) && self.pending_edit_name().is_some())
            })
            .count();
        if unsaved == 0 {
            self.clean_recovery_exit();
            return true;
        }

        let message = if unsaved == 1 {
            "Discard changes to the unsaved image?".to_owned()
        } else {
            format!("Discard changes to {unsaved} unsaved images?")
        };
        self.confirm_discard(&message, window, cx, move |workspace, window, cx| {
            workspace.clean_recovery_exit();
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
    let name = Path::new(&document.name);
    let current_format = save_format(name).ok();
    let preferred_project_extension = name
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(crate::project::EXTENSION));
    if current_format == Some(format)
        && (format != FileFormat::Project || preferred_project_extension)
    {
        return document.name.clone();
    }
    let stem = document
        .path
        .as_deref()
        .or_else(|| current_format.map(|_| name))
        .and_then(Path::file_stem)
        .and_then(|value| value.to_str())
        .unwrap_or(&document.name);
    format!("{stem}.{}", format.extension())
}

fn prepare_destination(path: &Path, project_only: bool) -> Result<(PathBuf, FileFormat)> {
    let format = save_format(path)?;
    anyhow::ensure!(
        !project_only || format == FileFormat::Project,
        "Use a .lime filename. To save a PNG, JPEG, or WebP copy, use File → Export."
    );
    Ok((destination_for_format(path, format)?, format))
}

fn suggested_export_name(document: &Document, format: ExportFormat) -> String {
    if let ExportFormat::Raster { format, .. } = format {
        return suggested_name(document, format);
    }
    let name = Path::new(&document.name);
    if crate::pdf::is_pdf_path(name) {
        return document.name.clone();
    }
    let stem = document
        .path
        .as_deref()
        .or_else(|| save_format(name).ok().map(|_| name))
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
        .unwrap_or(&document.name);
    if crate::pdf::is_pdf_path(Path::new(stem)) {
        stem.to_owned()
    } else {
        format!("{stem}.pdf")
    }
}

fn prepare_export(path: &Path, format: ExportFormat) -> Result<PathBuf> {
    anyhow::ensure!(
        format.matches_path(path),
        "Use a .{} filename for this export.",
        format.extension(),
    );
    let destination = canonical_destination(path)?;
    anyhow::ensure!(
        format.matches_path(&destination),
        "The destination resolves to a file with a different extension."
    );
    Ok(destination)
}

fn destination_for_format(path: &Path, format: FileFormat) -> Result<PathBuf> {
    let destination = canonical_destination(path)?;
    anyhow::ensure!(
        save_format(&destination).ok() == Some(format),
        "The destination resolves to a file with a different extension."
    );
    Ok(destination)
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
    fn raster_save_rechecks_content_that_changed_while_the_dialog_was_open() {
        let mut document = Document::from_pixels("Image.png".into(), image::RgbaImage::new(2, 2));
        document.format = FileFormat::Png;
        let save = WriteKind::Save(FileFormat::Png);
        assert!(save.validate_document(&document).is_ok());

        let original = document.content.clone();
        document
            .content
            .duplicate_layer(document.content.top_layer_id())
            .unwrap();
        assert!(save.validate_document(&document).is_err());
        assert!(
            WriteKind::Save(FileFormat::Project)
                .validate_document(&document)
                .is_ok()
        );
        assert!(
            WriteKind::Export(ExportFormat::Raster {
                format: FileFormat::Png,
                jpeg_quality: 92
            })
            .validate_document(&document)
            .is_ok()
        );

        document.content = original;
        document
            .content
            .set_paths(
                vec![crate::paths::VectorPath {
                    id: crate::paths::PathId(1),
                    visible: true,
                    closed: false,
                    anchors: vec![crate::paths::Anchor::corner(
                        crate::paths::AnchorId(1),
                        (0., 0.),
                    )],
                }],
                Some(crate::paths::PathId(1)),
            )
            .unwrap();
        assert!(save.validate_document(&document).is_err());
    }

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
    fn writes_respect_open_files_and_in_flight_destinations() {
        let save = WriteKind::Save(FileFormat::Png);
        let export = WriteKind::Export(ExportFormat::Raster {
            format: FileFormat::Png,
            jpeg_quality: 92,
        });
        assert!(!save.destination_reserved(true, true, false));
        assert!(save.destination_reserved(false, true, false));
        assert!(export.destination_reserved(true, true, false));
        assert!(export.destination_reserved(false, true, false));
        for kind in [
            save,
            export,
            WriteKind::Export(ExportFormat::Pdf { dpi: 300 }),
        ] {
            assert!(kind.destination_reserved(false, false, true));
            assert!(!kind.destination_reserved(false, false, false));
        }
        let pdf = WriteKind::Export(ExportFormat::Pdf { dpi: 300 });
        assert!(pdf.destination_reserved(true, true, false));
        assert!(pdf.destination_reserved(false, true, false));
    }

    #[cfg(unix)]
    #[test]
    fn aliases_cannot_write_a_different_format_over_their_target() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("original.imageedit");
        std::fs::write(&project, b"original").unwrap();
        let alias = directory.path().join("alias.png");
        std::os::unix::fs::symlink(&project, &alias).unwrap();
        assert!(prepare_destination(&alias, false).is_err());
        assert!(
            prepare_export(
                &alias,
                ExportFormat::Raster {
                    format: FileFormat::Png,
                    jpeg_quality: 92
                }
            )
            .is_err()
        );
        let pdf_alias = directory.path().join("alias.pdf");
        std::os::unix::fs::symlink(&project, &pdf_alias).unwrap();
        assert!(prepare_export(&pdf_alias, ExportFormat::Pdf { dpi: 300 }).is_err());
    }

    #[test]
    fn pdf_is_export_only_and_keeps_sensible_source_names() {
        let directory = tempfile::tempdir().unwrap();
        let format = ExportFormat::Pdf { dpi: 300 };
        let pdf = directory.path().join("export.PDF");
        assert!(prepare_export(&pdf, format).is_ok());
        assert!(prepare_destination(&pdf, false).is_err());
        assert!(prepare_export(&directory.path().join("export.png"), format).is_err());
        let mut document = Document::from_content(
            "source.pdf".into(),
            crate::document::DocumentContent::single_layer(
                "Page 1".into(),
                image::RgbaImage::new(1, 1),
            ),
        );
        for (name, expected) in [
            ("source.pdf", "source.pdf"),
            ("source.PNG", "source.pdf"),
            ("art.lime", "art.pdf"),
            ("source.pdf.lime", "source.pdf"),
            ("Untitled 1", "Untitled 1.pdf"),
            ("source (2)", "source (2).pdf"),
        ] {
            document.name = name.into();
            assert_eq!(suggested_export_name(&document, format), expected);
        }
        document.path = Some(directory.path().join("saved.lime"));
        assert_eq!(suggested_export_name(&document, format), "saved.pdf");
        document.path = Some(directory.path().join("saved.pdf.lime"));
        assert_eq!(suggested_export_name(&document, format), "saved.pdf");
    }

    #[test]
    fn destinations_reject_mismatched_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let path = |name: &str| directory.path().join(name);
        assert!(prepare_destination(&path("a.png"), true).is_err());
        assert!(prepare_destination(&path("a.txt"), false).is_err());
        for name in ["a.lime", "a.LIME", "a.imageedit"] {
            assert_eq!(
                prepare_destination(&path(name), true).unwrap().1,
                FileFormat::Project
            );
        }
        let (saved, format) = prepare_destination(&path("a.png"), false).unwrap();
        assert_eq!(format, FileFormat::Png);
        assert_eq!(
            saved,
            directory.path().canonicalize().unwrap().join("a.png")
        );

        let png = ExportFormat::Raster {
            format: FileFormat::Png,
            jpeg_quality: 92,
        };
        let jpeg = ExportFormat::Raster {
            format: FileFormat::Jpeg,
            jpeg_quality: 92,
        };
        assert!(prepare_export(&path("a.imageedit"), png).is_err());
        assert!(prepare_export(&path("a.lime"), png).is_err());
        assert!(prepare_export(&path("a.png"), jpeg).is_err());
        assert!(prepare_export(&path("a.jpeg"), jpeg).is_ok());
        assert!(prepare_export(&path("missing/a.png"), png).is_err());
    }

    #[test]
    fn project_save_as_suggests_lime_including_for_legacy_projects() {
        let mut document = Document::blank("Untitled 1".into(), 1, 1, Rgba([0; 4]));
        assert_eq!(
            suggested_name(&document, FileFormat::Project),
            "Untitled 1.lime"
        );
        for name in ["photo.png", "photo.imageedit", "photo.lime", "photo.LIME"] {
            document.name = name.into();
            document.path = Some(PathBuf::from(name));
            let expected = if name.ends_with(".LIME") {
                name
            } else {
                "photo.lime"
            };
            assert_eq!(suggested_name(&document, FileFormat::Project), expected);
        }
        document.name = "photo.jpeg".into();
        assert_eq!(suggested_name(&document, FileFormat::Jpeg), "photo.jpeg");
    }
}
