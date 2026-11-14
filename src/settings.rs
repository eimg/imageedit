//! Versioned personal settings. Document content never belongs in this store.
use crate::color_memory::{self, Changes, Memory};
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 128 * 1024;

pub trait Preferences: Default + Serialize + DeserializeOwned {
    fn validate(&self) -> Result<()>;

    /// Paths in each group are replaced together, never merged by JSON leaf.
    fn merge_groups() -> &'static [&'static [&'static str]] {
        &[]
    }

    /// Restore derived references after merging independently remembered values.
    fn reconcile_patch(&mut self) {}
}

#[derive(Clone)]
pub struct Snapshot<P> {
    pub preferences: P,
    pub colors: Memory,
}

#[derive(Clone, Default)]
pub struct Patch {
    pub preferences: Map<String, Value>,
    pub colors: Changes,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.preferences.is_empty() && self.colors.is_empty()
    }

    pub fn merge(&mut self, newer: &Self) {
        for (path, value) in &newer.preferences {
            self.insert(path.clone(), value.clone());
        }
        self.colors.merge(&newer.colors);
    }

    fn insert(&mut self, path: String, value: Value) {
        let prefix = format!("{path}/");
        self.preferences.retain(|key, _| !key.starts_with(&prefix));
        // Fold a newer leaf into an already queued replacement of its parent.
        for (parent, replacement) in &mut self.preferences {
            if let Some(suffix) = path.strip_prefix(&format!("{parent}/"))
                && let Some(target) = replacement.pointer_mut(&format!("/{suffix}"))
            {
                *target = value;
                return;
            }
        }
        self.preferences.insert(path, value);
    }

    pub fn record<P: Preferences>(&mut self, before: &P, after: &P) -> Result<()> {
        let Value::Object(before) = serde_json::to_value(before)? else {
            anyhow::bail!("Settings must be an object.");
        };
        let Value::Object(after) = serde_json::to_value(after)? else {
            anyhow::bail!("Settings must be an object.");
        };
        for (key, value) in &after {
            diff(self, key, before.get(key).unwrap_or(&Value::Null), value);
        }
        let before = Value::Object(before);
        let after = Value::Object(after);
        for group in P::merge_groups() {
            if group.iter().any(|path| {
                before.pointer(&format!("/{path}")) != after.pointer(&format!("/{path}"))
            }) {
                self.replace_values(&after, group)?;
            }
        }
        Ok(())
    }

    /// Record an explicit operation, including values already at their defaults
    /// locally that another instance may have changed on disk.
    pub fn replace<P: Preferences>(&mut self, preferences: &P, paths: &[&str]) -> Result<()> {
        self.replace_values(&serde_json::to_value(preferences)?, paths)
    }

    fn replace_values(&mut self, value: &Value, paths: &[&str]) -> Result<()> {
        for path in paths {
            let replacement = value
                .pointer(&format!("/{path}"))
                .with_context(|| format!("Unknown preference: {path}"))?;
            self.insert((*path).to_owned(), replacement.clone());
        }
        Ok(())
    }

    pub fn apply<P: Preferences>(&self, snapshot: &mut Snapshot<P>) -> Result<()> {
        let mut value = serde_json::to_value(&snapshot.preferences)?;
        for (path, replacement) in &self.preferences {
            let target = value
                .pointer_mut(&format!("/{path}"))
                .with_context(|| format!("Unknown preference: {path}"))?;
            *target = replacement.clone();
        }
        let mut preferences: P = serde_json::from_value(value)?;
        preferences.reconcile_patch();
        preferences.validate()?;
        snapshot.preferences = preferences;
        snapshot.colors.apply(&self.colors);
        ensure!(snapshot.colors.valid(), "Invalid remembered colors.");
        Ok(())
    }
}

fn diff(patch: &mut Patch, path: &str, before: &Value, after: &Value) {
    if before == after {
        return;
    }
    match (before, after) {
        (Value::Object(before), Value::Object(after)) if before.keys().eq(after.keys()) => {
            for (key, value) in after {
                diff(patch, &format!("{path}/{key}"), &before[key], value);
            }
        }
        (Value::Array(before), Value::Array(after))
            if before.len() == after.len() && !path.ends_with("/color") =>
        {
            for (index, (old, new)) in before.iter().zip(after).enumerate() {
                diff(patch, &format!("{path}/{index}"), old, new);
            }
        }
        _ => patch.insert(path.to_owned(), after.clone()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSettings {
    version: u32,
    #[serde(default)]
    colors: Memory,
    #[serde(default = "empty_preferences")]
    preferences: Value,
}

fn empty_preferences() -> Value {
    Value::Object(Map::new())
}

pub fn default_path() -> Option<PathBuf> {
    color_memory::default_path().map(|path| path.with_file_name("settings.json"))
}

pub fn load<P: Preferences>(path: &Path) -> Result<Snapshot<P>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The legacy file is left intact. The first successful settings write
            // copies its colors into the centralized store.
            return Ok(Snapshot {
                preferences: P::default(),
                colors: color_memory::load(&path.with_file_name("colors.json"))?,
            });
        }
        Err(error) => return Err(error).context("Could not open settings"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Settings file is too large."
    );
    let wire: WireSettings = serde_json::from_slice(&bytes).context("Invalid settings file")?;
    ensure!(wire.version == 1, "Unsupported settings version.");
    ensure!(wire.colors.valid(), "Invalid remembered colors.");
    let preferences: P = serde_json::from_value(wire.preferences)?;
    preferences.validate()?;
    Ok(Snapshot {
        preferences,
        colors: wire.colors,
    })
}

pub fn save<P: Preferences>(path: &Path, patch: &Patch) -> Result<Snapshot<P>> {
    let parent = path
        .parent()
        .context("No settings directory is available.")?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.try_lock()
        .context("Another Lime instance is saving settings")?;
    let mut snapshot = load::<P>(path)?;
    patch.apply(&mut snapshot)?;
    let bytes = serde_json::to_vec_pretty(&WireSettings {
        version: 1,
        colors: snapshot.colors.clone(),
        preferences: serde_json::to_value(&snapshot.preferences)?,
    })?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Settings file is too large."
    );
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .context("Could not replace settings")?;
    Ok(snapshot)
}

pub(crate) mod rgba {
    use image::Rgba;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(color: &Rgba<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        color.0.serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Rgba<u8>, D::Error> {
        <[u8; 4]>::deserialize(deserializer).map(Rgba)
    }
}
