//! Personal RGB swatches and shared recent colors, separate from image history.
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
#[cfg(test)]
use std::{
    fs::{self, OpenOptions},
    io::Write,
};

pub const DEFAULT_SWATCHES: [u32; 2] = [0x000000, 0xffffff];
pub const RECENT_LIMIT: usize = 12;
const MAX_BYTES: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Memory {
    pub swatches: [u32; 2],
    pub recent: Vec<u32>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            swatches: DEFAULT_SWATCHES,
            recent: Vec::new(),
        }
    }
}

/// Bounded edits can be replayed over a late startup load or another writer's
/// saved colors without replacing untouched swatches or losing recent choices.
#[derive(Clone, Debug, Default)]
pub struct Changes {
    swatches: [Option<u32>; 2],
    recent: Vec<u32>,
}

fn remember(recent: &mut Vec<u32>, color: u32) {
    recent.retain(|&value| value != color);
    recent.insert(0, color);
    recent.truncate(RECENT_LIMIT);
}

impl Changes {
    pub fn color(color: u32) -> Self {
        Self {
            recent: vec![color & 0xffffff],
            ..Self::default()
        }
    }

    pub fn swatch(index: usize, color: u32) -> Self {
        let mut changes = Self::color(color);
        changes.swatches[index] = Some(color & 0xffffff);
        changes
    }

    pub fn swatches(swatches: [u32; 2]) -> Self {
        Self {
            swatches: swatches.map(|color| Some(color & 0xffffff)),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.swatches.iter().all(Option::is_none) && self.recent.is_empty()
    }

    pub fn merge(&mut self, newer: &Self) {
        for (current, new) in self.swatches.iter_mut().zip(newer.swatches) {
            if new.is_some() {
                *current = new;
            }
        }
        for &color in newer.recent.iter().rev() {
            remember(&mut self.recent, color);
        }
    }
}

impl Memory {
    pub fn valid(&self) -> bool {
        self.recent.len() <= RECENT_LIMIT
            && self
                .swatches
                .iter()
                .chain(&self.recent)
                .all(|&color| color <= 0xffffff)
            && self
                .recent
                .iter()
                .enumerate()
                .all(|(index, color)| !self.recent[..index].contains(color))
    }

    pub fn apply(&mut self, changes: &Changes) {
        for (swatch, color) in self.swatches.iter_mut().zip(changes.swatches) {
            if let Some(color) = color {
                *swatch = color;
            }
        }
        for &color in changes.recent.iter().rev() {
            remember(&mut self.recent, color);
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMemory {
    version: u32,
    swatches: [u32; 2],
    recent: Vec<u32>,
}

fn decode(bytes: &[u8]) -> Result<Memory> {
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Color memory is too large."
    );
    let wire: WireMemory = serde_json::from_slice(bytes).context("Invalid color memory")?;
    ensure!(wire.version == 1, "Unsupported color memory version.");
    ensure!(wire.recent.len() <= RECENT_LIMIT, "Too many recent colors.");
    ensure!(
        wire.swatches
            .iter()
            .chain(&wire.recent)
            .all(|&color| color <= 0xffffff),
        "Invalid RGB color."
    );
    let mut recent = Vec::new();
    for color in wire.recent.into_iter().rev() {
        remember(&mut recent, color);
    }
    Ok(Memory {
        swatches: wire.swatches,
        recent,
    })
}

pub fn default_path() -> Option<PathBuf> {
    crate::gradient::library::default_path().map(|path| path.with_file_name("colors.json"))
}

pub fn load(path: &Path) -> Result<Memory> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Memory::default()),
        Err(error) => return Err(error).context("Could not open color memory"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    decode(&bytes)
}

#[cfg(test)]
pub fn save_changes(path: &Path, changes: &Changes) -> Result<Memory> {
    let parent = path
        .parent()
        .context("No color memory directory is available.")?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.try_lock()
        .context("Another Lime instance is saving colors. Try choosing a color again")?;
    // Merge under the lock; never overwrite unreadable or newer-format files.
    let mut memory = load(path)?;
    memory.apply(changes);
    let bytes = serde_json::to_vec_pretty(&WireMemory {
        version: 1,
        swatches: memory.swatches,
        recent: memory.recent.clone(),
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .context("Could not replace color memory")?;
    Ok(memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_choices_deduplicate_and_survive_restart_with_both_swatches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colors.json");
        assert_eq!(load(&path).unwrap(), Memory::default());
        let mut changes = Changes::swatch(0, 0x123456);
        changes.merge(&Changes::swatch(1, 0xabcdef));
        for color in 0..20 {
            changes.merge(&Changes::color(color));
        }
        changes.merge(&Changes::color(15));
        let saved = save_changes(&path, &changes).unwrap();
        assert_eq!(saved.swatches, [0x123456, 0xabcdef]);
        assert_eq!(saved.recent.len(), RECENT_LIMIT);
        assert_eq!(&saved.recent[..3], &[15, 19, 18]);
        assert_eq!(saved.recent.iter().filter(|&&color| color == 15).count(), 1);
        assert_eq!(load(&path).unwrap(), saved);
    }

    #[test]
    fn late_load_and_queued_edits_preserve_choices_and_untouched_swatches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colors.json");
        let saved = save_changes(&path, &Changes::swatch(1, 0xabcdef)).unwrap();
        let first = Changes::swatch(0, 0x112233);
        let second = Changes::color(0x445566);
        let mut queued = first.clone();
        queued.merge(&second);
        let mut loaded = saved;
        loaded.apply(&queued);
        assert_eq!(loaded.swatches, [0x112233, 0xabcdef]);
        assert_eq!(loaded.recent, [0x445566, 0x112233, 0xabcdef]);
        save_changes(&path, &first).unwrap();
        let other = Changes::swatch(1, 0xaabbcc);
        save_changes(&path, &other).unwrap();
        let merged = save_changes(&path, &second).unwrap();
        assert_eq!(merged.swatches, [0x112233, 0xaabbcc]);
        assert_eq!(merged.recent, [0x445566, 0xaabbcc, 0x112233, 0xabcdef]);
    }

    #[test]
    fn swap_and_reset_persist_without_clearing_recent_colors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colors.json");
        let chosen = save_changes(&path, &Changes::swatch(0, 0x112233)).unwrap();
        let swapped = save_changes(&path, &Changes::swatches([0xffffff, 0x112233])).unwrap();
        assert_eq!(swapped.swatches, [0xffffff, 0x112233]);
        let reset = save_changes(&path, &Changes::swatches(DEFAULT_SWATCHES)).unwrap();
        assert_eq!(reset.swatches, DEFAULT_SWATCHES);
        assert_eq!(reset.recent, chosen.recent);
    }

    #[test]
    fn coalescing_many_choices_matches_immediate_edits_and_stays_bounded() {
        let mut initial = Memory::default();
        initial.apply(&Changes::swatch(1, 0xabcdef));
        let mut immediate = initial.clone();
        let mut pending = Changes::default();
        for index in 0..100 {
            let changes = match index % 4 {
                0 => Changes::swatch(0, index),
                1 => Changes::color(index % 17),
                2 => Changes::swatches([index, index + 1]),
                _ => Changes::swatch(1, index),
            };
            immediate.apply(&changes);
            pending.merge(&changes);
            assert!(pending.recent.len() <= RECENT_LIMIT);
        }
        initial.apply(&pending);
        assert_eq!(initial, immediate);
    }

    #[test]
    fn failed_writes_keep_existing_files_and_invalid_memory_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colors.json");
        let change = Changes::color(0x123456);
        for bytes in [
            b"broken".to_vec(),
            br#"{"version":2,"swatches":[0,16777215],"recent":[]}"#.to_vec(),
            br#"{"version":1,"swatches":[0,16777216],"recent":[]}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(save_changes(&path, &change).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_file(&path).unwrap();
        save_changes(&path, &change).unwrap();
        let before = fs::read(&path).unwrap();
        let lock = File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.lock().unwrap();
        assert!(save_changes(&path, &Changes::color(0xabcdef)).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
