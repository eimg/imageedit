use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail, ensure};
use image::Rgba;
use serde::{Deserialize, Serialize};

use super::{ColorRamp, ColorStop};
use crate::paint::GradientShape;

pub const MAX_PRESETS: usize = 32;
pub const MAX_NAME_CHARS: usize = 64;
const MAX_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetValue {
    pub ramp: ColorRamp,
    pub shape: GradientShape,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub id: u64,
    pub name: String,
    pub value: PresetValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    next_id: u64,
    presets: Vec<Preset>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            next_id: 1,
            presets: Vec::new(),
        }
    }
}

impl Library {
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    pub fn get(&self, id: u64) -> Option<&Preset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    pub fn suggested_name(&self) -> String {
        (1..=MAX_PRESETS + 1)
            .map(|index| format!("Gradient {index}"))
            .find(|name| {
                !self
                    .presets
                    .iter()
                    .any(|preset| preset.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or_else(|| "Gradient".into())
    }

    fn name(&self, text: &str, except: Option<u64>) -> Result<String> {
        let name = checked_name(text)?;
        let key = name.to_lowercase();
        ensure!(
            !self
                .presets
                .iter()
                .any(|preset| Some(preset.id) != except && preset.name.to_lowercase() == key),
            "A gradient with that name already exists."
        );
        Ok(name)
    }

    pub fn insert(&mut self, name: &str, value: PresetValue) -> Result<u64> {
        ensure!(
            self.presets.len() < MAX_PRESETS,
            "The gradient library can hold at most {MAX_PRESETS} gradients."
        );
        let name = self.name(name, None)?;
        let next = self
            .next_id
            .checked_add(1)
            .context("Gradient identifiers are exhausted.")?;
        let id = self.next_id;
        self.presets.push(Preset { id, name, value });
        self.next_id = next;
        Ok(id)
    }

    pub fn rename(&mut self, id: u64, name: &str) -> Result<bool> {
        let name = self.name(name, Some(id))?;
        let preset = self
            .presets
            .iter_mut()
            .find(|preset| preset.id == id)
            .context("That gradient is no longer available.")?;
        if preset.name == name {
            return Ok(false);
        }
        preset.name = name;
        Ok(true)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.id != id);
        self.presets.len() != before
    }
}

fn checked_name(text: &str) -> Result<String> {
    let name = text.trim();
    ensure!(!name.is_empty(), "Enter a gradient name.");
    ensure!(
        name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(char::is_control),
        "Use at most {MAX_NAME_CHARS} characters without control characters."
    );
    Ok(name.to_owned())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireLibrary {
    version: u32,
    next_id: u64,
    presets: Vec<WirePreset>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePreset {
    id: u64,
    name: String,
    stops: Vec<WireStop>,
    shape: WireShape,
    reverse: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireStop {
    position: u16,
    color: [u8; 4],
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireShape {
    Linear,
    Radial,
}

pub fn decode(bytes: &[u8]) -> Result<Library> {
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "The gradient library is too large."
    );
    let wire: WireLibrary =
        serde_json::from_slice(bytes).context("The gradient library is invalid")?;
    ensure!(
        wire.version == 1,
        "This gradient library version is not supported."
    );
    ensure!(
        wire.presets.len() <= MAX_PRESETS && wire.next_id > 0,
        "Invalid gradient library limits."
    );
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut presets = Vec::with_capacity(wire.presets.len());
    for preset in wire.presets {
        ensure!(
            preset.id > 0 && preset.id < wire.next_id && ids.insert(preset.id),
            "Invalid gradient identifiers."
        );
        let name = checked_name(&preset.name)?;
        ensure!(
            names.insert(name.to_lowercase()),
            "Duplicate gradient names."
        );
        let stops: Vec<_> = preset
            .stops
            .into_iter()
            .map(|stop| ColorStop {
                position: stop.position,
                color: Rgba(stop.color),
            })
            .collect();
        let ramp = ColorRamp::from_stops(&stops).context("Invalid gradient color stops.")?;
        presets.push(Preset {
            id: preset.id,
            name,
            value: PresetValue {
                ramp,
                shape: match preset.shape {
                    WireShape::Linear => GradientShape::Linear,
                    WireShape::Radial => GradientShape::Radial,
                },
                reverse: preset.reverse,
            },
        });
    }
    Ok(Library {
        next_id: wire.next_id,
        presets,
    })
}

pub fn encode(library: &Library) -> Result<Vec<u8>> {
    let wire = WireLibrary {
        version: 1,
        next_id: library.next_id,
        presets: library
            .presets
            .iter()
            .map(|preset| WirePreset {
                id: preset.id,
                name: preset.name.clone(),
                stops: preset
                    .value
                    .ramp
                    .stops()
                    .iter()
                    .map(|stop| WireStop {
                        position: stop.position,
                        color: stop.color.0,
                    })
                    .collect(),
                shape: match preset.value.shape {
                    GradientShape::Linear => WireShape::Linear,
                    GradientShape::Radial => WireShape::Radial,
                },
                reverse: preset.value.reverse,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&wire)?;
    // Keep the same validation contract in both directions.
    decode(&bytes)?;
    Ok(bytes)
}

pub fn default_path() -> Option<PathBuf> {
    let path = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|root| {
            PathBuf::from(root).join("Library/Application Support/com.eimg.lime/gradients.json")
        })
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("Lime/gradients.json"))
    } else {
        xdg_library_path(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
    };
    path.filter(|path| path.is_absolute())
}

fn xdg_library_path(
    config: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    config
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })
        .map(|root| root.join("lime/gradients.json"))
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("The gradient library could not be opened"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "The gradient library is too large."
    );
    Ok(Some(bytes))
}

pub fn load(path: &Path) -> Result<(Library, Option<Vec<u8>>)> {
    let bytes = read_bytes(path)?;
    let library = bytes
        .as_ref()
        .map_or_else(|| Ok(Library::default()), |bytes| decode(bytes))?;
    Ok((library, bytes))
}

pub fn save(path: &Path, expected: Option<&[u8]>, library: &Library) -> Result<Vec<u8>> {
    let bytes = encode(library)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("No gradient library directory is available.")?;
    fs::create_dir_all(parent).context("The gradient library directory could not be created")?;
    // The OS releases this advisory lock after a crash, so no stale lock cleanup
    // is needed. It serializes Lime writers without blocking the UI thread.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join("gradients.lock"))?;
    lock.try_lock()
        .context("Another Lime window is saving gradients. Reload and try again")?;
    let current = read_bytes(path)?;
    if current.as_deref() != expected {
        bail!("The gradient library changed outside this window. Reload it before saving.");
    }
    if let Some(bytes) = &current {
        decode(bytes).context("The existing gradient library is not writable by this version")?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    if read_bytes(path)?.as_deref() != expected {
        bail!("The gradient library changed outside this window. Reload it before saving.");
    }
    temporary
        .persist(path)
        .context("The gradient library could not be replaced")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn xdg_settings_fall_back_to_home_for_empty_or_relative_config_roots() {
        let home = std::path::absolute("xdg-test-home").unwrap();
        let config = std::path::absolute("xdg-test-config").unwrap();
        let path = |config: Option<&Path>, home: Option<&Path>| {
            xdg_library_path(
                config.map(|path| path.as_os_str().to_owned()),
                home.map(|path| path.as_os_str().to_owned()),
            )
        };
        let fallback = Some(home.join(".config/lime/gradients.json"));
        for config in [
            None,
            Some(Path::new("")),
            Some(Path::new("relative/config")),
        ] {
            assert_eq!(path(config, Some(&home)), fallback);
        }
        assert_eq!(
            path(Some(&config), None),
            Some(config.join("lime/gradients.json"))
        );
        assert_eq!(
            path(
                Some(Path::new("relative")),
                Some(Path::new("relative/home"))
            ),
            None
        );
        assert_eq!(path(None, None), None);
    }

    fn value() -> PresetValue {
        PresetValue {
            ramp: ColorRamp::from_stops(&[
                ColorStop {
                    position: 0,
                    color: Rgba([30, 80, 120, 0]),
                },
                ColorStop {
                    position: 375,
                    color: Rgba([250, 40, 180, 7]),
                },
                ColorStop {
                    position: 1000,
                    color: Rgba([90, 200, 10, 255]),
                },
            ])
            .unwrap(),
            shape: GradientShape::Radial,
            reverse: true,
        }
    }

    #[test]
    fn named_gradients_round_trip_literal_colors_shape_direction_and_stable_ids() {
        let mut library = Library::default();
        let dawn = library.insert("  Dawn 🌅  ", value()).unwrap();
        let night = library.insert("Night", value()).unwrap();
        assert_eq!(library.get(dawn).unwrap().name, "Dawn 🌅");
        let decoded = decode(&encode(&library).unwrap()).unwrap();
        assert_eq!(decoded, library);
        for t in [0., 0.123, 0.375, 0.9, 1.] {
            assert_eq!(
                decoded.get(dawn).unwrap().value.ramp.sample(t),
                value().ramp.sample(t)
            );
        }
        assert_eq!(
            decoded.get(dawn).unwrap().value.ramp.stops()[0].color,
            Rgba([30, 80, 120, 0])
        );
        assert!(library.remove(dawn));
        assert!(library.rename(night, "Night Sky").unwrap());
        assert!(!library.rename(night, " Night Sky ").unwrap());
        let next = library.insert("Next", value()).unwrap();
        assert!(next > night && next > dawn);
        assert_eq!(decode(&encode(&library).unwrap()).unwrap(), library);
        assert!(!library.remove(dawn));
    }

    #[test]
    fn invalid_names_and_full_capacity_leave_the_library_unchanged() {
        let mut library = Library::default();
        let id = library.insert("Ocean", value()).unwrap();
        for name in [
            "".to_owned(),
            " ocean ".into(),
            "Bad\nName".into(),
            "x".repeat(MAX_NAME_CHARS + 1),
        ] {
            let previous = library.clone();
            assert!(library.insert(&name, value()).is_err());
            assert_eq!(library, previous);
        }
        library.insert("Other", value()).unwrap();
        let previous = library.clone();
        assert!(library.rename(id, "OTHER").is_err());
        assert!(library.rename(u64::MAX, "Missing").is_err());
        assert_eq!(library, previous);
        for index in 2..MAX_PRESETS {
            library
                .insert(
                    &format!("{index:02}{}", "🟩".repeat(MAX_NAME_CHARS - 2)),
                    value(),
                )
                .unwrap();
        }
        let full = library.clone();
        assert!(library.insert("Too Many", value()).is_err());
        assert_eq!(library, full);
        assert_eq!(decode(&encode(&library).unwrap()).unwrap(), full);
    }

    #[test]
    fn malformed_future_and_invalid_gradient_data_are_rejected_before_use() {
        let mut library = Library::default();
        library.insert("One", value()).unwrap();
        let valid: serde_json::Value = serde_json::from_slice(&encode(&library).unwrap()).unwrap();
        let mut invalid = Vec::new();
        for version in [0, 2] {
            let mut wire = valid.clone();
            wire["version"] = json!(version);
            invalid.push(wire);
        }
        for next in [0, 1] {
            let mut wire = valid.clone();
            wire["next_id"] = json!(next);
            invalid.push(wire);
        }
        let mut wire = valid.clone();
        wire["unknown"] = json!(true);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["shape"] = json!("conical");
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["stops"][0]["position"] = json!(1);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["stops"][2]["position"] = json!(999);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["stops"][1]["position"] = json!(0);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["stops"][1]["color"] = json!([0, 0, 0, 256]);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"][0]["stops"] = json!([]);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"] = json!(vec![valid["presets"][0].clone(); MAX_PRESETS + 1]);
        invalid.push(wire);
        let mut wire = valid.clone();
        wire["presets"] = json!([valid["presets"][0], valid["presets"][0]]);
        invalid.push(wire);
        for wire in invalid {
            assert!(decode(&serde_json::to_vec(&wire).unwrap()).is_err());
        }
        for bytes in [b"not json".as_slice(), b"{", &[255, 255]] {
            assert!(decode(bytes).is_err());
        }
        assert!(decode(&vec![b' '; MAX_BYTES as usize + 1]).is_err());
    }

    #[test]
    fn filesystem_round_trips_new_renamed_and_deleted_presets_without_temp_leftovers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/gradients.json");
        assert_eq!(load(&path).unwrap(), (Library::default(), None));
        let mut library = Library::default();
        let id = library.insert("Saved", value()).unwrap();
        let first = save(&path, None, &library).unwrap();
        assert_eq!(load(&path).unwrap(), (library.clone(), Some(first.clone())));
        assert!(library.rename(id, "Renamed").unwrap());
        let renamed = save(&path, Some(&first), &library).unwrap();
        assert_eq!(fs::read(&path).unwrap(), renamed);
        assert!(library.remove(id));
        let empty = save(&path, Some(&renamed), &library).unwrap();
        let (mut reopened, bytes) = load(&path).unwrap();
        assert_eq!(bytes, Some(empty));
        assert!(reopened.presets().is_empty());
        assert!(reopened.insert("New after restart", value()).unwrap() > id);
        let mut names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["gradients.json", "gradients.lock"]);
    }

    #[test]
    fn stale_or_locked_writers_cannot_replace_another_windows_library() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("gradients.json");
        let mut first = Library::default();
        let id = first.insert("Original", value()).unwrap();
        let original = save(&path, None, &first).unwrap();
        let mut second = first.clone();
        second.rename(id, "From another window").unwrap();
        let latest = save(&path, Some(&original), &second).unwrap();
        first.rename(id, "Stale change").unwrap();
        assert!(save(&path, Some(&original), &first).is_err());
        assert_eq!(fs::read(&path).unwrap(), latest);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join("gradients.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(save(&path, Some(&latest), &first).is_err());
        assert_eq!(fs::read(&path).unwrap(), latest);
        drop(lock);
        assert!(save(&path, Some(&latest), &first).is_ok());
    }

    #[test]
    fn corrupt_or_unsupported_existing_files_and_io_failures_are_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("gradients.json");
        let mut library = Library::default();
        library.insert("New", value()).unwrap();
        for bytes in [
            b"broken".to_vec(),
            br#"{"version":2,"next_id":1,"presets":[]}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(load(&path).is_err());
            assert!(save(&path, Some(&bytes), &library).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let parent = directory.path().join("not-a-directory");
        fs::write(&parent, b"keep me").unwrap();
        assert!(save(&parent.join("gradients.json"), None, &library).is_err());
        assert_eq!(fs::read(parent).unwrap(), b"keep me");
    }
}
