//! Separate, atomically replaced crash-recovery projects. These never replace
//! the user's document path or mark its revision saved.
use crate::document::{Document, DocumentContent, FileFormat};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const DISK_BUDGET: u64 = 1024 * 1024 * 1024;
pub struct Store {
    dir: PathBuf,
    _lease: File,
    active: AtomicBool,
}
pub struct Recovered {
    pub snapshot: PathBuf,
    pub document: Document,
    pub warning: Option<String>,
    pub text: Option<serde_json::Value>,
}
impl Store {
    pub fn open(root: &Path) -> Result<(Arc<Self>, Vec<Recovered>, Vec<String>)> {
        fs::create_dir_all(root)?;
        let dir = tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(root)?
            .keep();
        let lease = File::create(dir.join("owner.lock"))?;
        lease.lock()?;
        let mut recovered = Vec::new();
        let mut errors = Vec::new();
        let mut claimed_count = 0;
        let mut recovered_bytes = 0u64;
        for entry in fs::read_dir(root)?.take(256) {
            let entry = entry?;
            if entry.path() == dir || !entry.file_type()?.is_dir() {
                continue;
            }
            let Ok(lock) = File::options()
                .read(true)
                .write(true)
                .open(entry.path().join("owner.lock"))
            else {
                continue;
            };
            if lock.try_lock().is_err() {
                continue;
            }
            if entry.path().join("closed").is_file() {
                let _ = fs::remove_dir_all(entry.path());
                continue;
            }
            for snapshot in fs::read_dir(entry.path())?.take(256) {
                let snapshot = snapshot?;
                if !snapshot.file_type()?.is_file()
                    || snapshot.path().extension().is_none_or(|ext| ext != "lime")
                {
                    continue;
                }
                // Read while holding the old session lease. Unreadable files
                // stay in their original directory, including on clean exit.
                match Document::open(&snapshot.path()) {
                    Ok(mut document) => {
                        let bytes = document
                            .content
                            .layers()
                            .iter()
                            .flat_map(|layer| layer.storage())
                            .map(|image| image.as_raw().len() as u64)
                            .sum::<u64>();
                        if recovered_bytes.saturating_add(bytes) > crate::document::MAX_STACK_BYTES
                        {
                            errors.push(format!("Recovery copy {} was left on disk because the recovery memory limit was reached.",snapshot.path().display()));
                            continue;
                        }
                        let claimed = dir.join(format!("recovered-{claimed_count}.lime"));
                        claimed_count += 1;
                        if let Err(error) = fs::rename(snapshot.path(), &claimed) {
                            errors.push(error.to_string());
                            continue;
                        }
                        recovered_bytes += bytes;
                        let embedded = crate::project::recovery_metadata(&claimed).ok().flatten();
                        let title = embedded
                            .as_ref()
                            .map(|entry| entry.name.clone())
                            .unwrap_or_else(|| {
                                fs::read_to_string(snapshot.path().with_extension("name"))
                                    .unwrap_or_else(|_| "Untitled".into())
                            });
                        let _ = fs::write(claimed.with_extension("name"), &title);
                        let mut text = embedded.as_ref().and_then(|entry| entry.text.clone());
                        let mut warning = None;
                        if embedded.is_none() {
                            let text_path = snapshot.path().with_extension("text.json");
                            if text_path
                                .metadata()
                                .is_ok_and(|metadata| metadata.len() <= 512 * 1024)
                            {
                                text = fs::read(&text_path)
                                    .ok()
                                    .and_then(|bytes| serde_json::from_slice(&bytes).ok());
                            }
                        }
                        if let Some(text) = &text {
                            let destination = claimed.with_extension("text.json");
                            if fs::write(&destination, serde_json::to_vec_pretty(text)?).is_ok() {
                                warning = Some(format!(
                                    "Recovered a text draft. Its source copy is at {}.",
                                    destination.display()
                                ));
                            } else {
                                warning = Some(
                                    "Recovered a text draft embedded in the recovery project."
                                        .into(),
                                );
                            }
                        }
                        document.path = None;
                        document.format = FileFormat::Project;
                        document.name =
                            format!("Recovered {}", title.chars().take(120).collect::<String>());
                        recovered.push(Recovered {
                            snapshot: claimed,
                            document,
                            warning,
                            text,
                        });
                    }
                    Err(error) => errors.push(format!(
                        "Recovery copy {} could not be opened: {error:#}. The file is preserved.",
                        snapshot.path().display()
                    )),
                }
            }
        }
        Ok((
            Arc::new(Self {
                dir,
                _lease: lease,
                active: AtomicBool::new(true),
            }),
            recovered,
            errors,
        ))
    }
    pub fn snapshot(
        &self,
        id: usize,
        name: &str,
        content: &DocumentContent,
        text: Option<&serde_json::Value>,
        alive: &AtomicBool,
    ) -> Result<()> {
        if !self.active.load(Ordering::Acquire) || !alive.load(Ordering::Acquire) {
            return Ok(());
        }
        let destination = self.dir.join(format!("tab-{id}.lime"));
        let mut output = tempfile::NamedTempFile::new_in(&self.dir)?;
        crate::project::write_recovery(&mut output, content, name, text)?;
        output.flush()?;
        output.as_file().sync_all()?;
        let current = fs::read_dir(&self.dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path() != destination)
            .filter_map(|entry| entry.metadata().ok())
            .map(|meta| meta.len())
            .sum::<u64>();
        ensure!(
            current <= DISK_BUDGET,
            "Recovery storage is full. Save or close some images to free space."
        );
        if !self.active.load(Ordering::Acquire) || !alive.load(Ordering::Acquire) {
            return Ok(());
        }
        output.persist(&destination).map_err(|error| error.error)?;
        // The project contains the authoritative name and draft atomically.
        // Sidecars are optional readable copies and cannot invalidate that state.
        let _ = fs::write(destination.with_extension("name"), name);
        if let Some(text) = text {
            let _ = fs::write(
                destination.with_extension("text.json"),
                serde_json::to_vec_pretty(text)?,
            );
        } else {
            let _ = fs::remove_file(destination.with_extension("text.json"));
        }
        if !self.active.load(Ordering::Acquire) || !alive.load(Ordering::Acquire) {
            self.remove(id);
        }
        Ok(())
    }
    pub fn remove(&self, id: usize) {
        for extension in ["lime", "name", "text.json"] {
            let _ = fs::remove_file(self.dir.join(format!("tab-{id}.{extension}")));
        }
    }
    pub fn clean_exit(&self) {
        self.active.store(false, Ordering::Release);
        // A clean-close marker also prevents recovery if removing open files
        // is delayed by the platform or a background writer during shutdown.
        if let Ok(mut marker) = File::create(self.dir.join("closed")) {
            let _ = marker.write_all(b"closed");
            let _ = marker.sync_all();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}
pub fn default_root() -> Result<PathBuf> {
    let config = crate::gradient::library::default_path()
        .context("Application support directory is unavailable")?;
    Ok(config
        .parent()
        .context("Application support directory is unavailable")?
        .join("Recovery"))
}
