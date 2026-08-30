//! Saved configurations on disk.
//!
//! A preset is the module's own configuration dump — the exact `08H` message bytes — so
//! the files are ordinary `.syx`, inspectable and interchangeable with anything else that
//! speaks the protocol. The dumps the hardware probe archives are loadable as presets
//! without conversion, which matters: the archive taken before a risky change is the thing
//! you want to load when it goes wrong.

use std::path::{Path, PathBuf};

use es9_protocol::{Config, Incoming, decode, encode};
use serde::Serialize;

/// A preset on disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    /// Display name, which is the file stem.
    pub name: String,
    /// Size of the stored dump in bytes.
    pub bytes: usize,
    /// Seconds since the Unix epoch, or `None` if the filesystem did not say.
    pub saved_at: Option<u64>,
    /// Whether the file parsed as a firmware 1.3 configuration.
    ///
    /// Listed rather than hidden: a file that will not load should be visible and
    /// explicable, not silently missing.
    pub loadable: bool,
    /// Why it will not load, when it will not.
    pub problem: Option<String>,
}

/// Where presets live.
///
/// `%APPDATA%\es9-mixer\presets` on Windows, falling back to a directory beside the
/// executable if the environment has no `APPDATA` at all.
pub fn directory() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("es9-mixer").join("presets")
}

/// Rejects anything that is not a plain file name.
///
/// Preset names come from a text box and end up as paths, so they are constrained here
/// rather than trusted. Nothing with a separator, a parent reference or a control
/// character gets through.
fn safe_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("a preset needs a name".into());
    }
    if trimmed.len() > 64 {
        return Err("that name is too long".into());
    }
    if trimmed.starts_with('.')
        || trimmed.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
    {
        return Err("a preset name cannot contain path characters".into());
    }
    Ok(trimmed.to_string())
}

fn path_for(name: &str) -> Result<PathBuf, String> {
    Ok(directory().join(format!("{}.syx", safe_name(name)?)))
}

/// Lists the presets, newest first.
pub fn list() -> Vec<PresetInfo> {
    let dir = directory();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PresetInfo> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "syx"))
        .map(|e| describe(&e.path()))
        .collect();
    // Newest first, then by name so the order is stable when timestamps tie.
    out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.name.cmp(&b.name)));
    out
}

fn describe(path: &Path) -> PresetInfo {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let meta = std::fs::metadata(path).ok();
    let saved_at = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let bytes = meta.as_ref().map(|m| m.len() as usize).unwrap_or(0);

    let (loadable, problem) = match read(path) {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e)),
    };
    PresetInfo {
        name,
        bytes,
        saved_at,
        loadable,
        problem,
    }
}

/// Reads and decodes one preset file.
fn read(path: &Path) -> Result<Config, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    match decode(&bytes) {
        Ok(Incoming::Config(c)) => Ok(*c),
        Ok(other) => Err(format!("not a configuration dump: {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}

/// Saves a configuration under a name, overwriting any existing preset of that name.
pub fn save(name: &str, config: &Config) -> Result<(), String> {
    let path = path_for(name)?;
    let dir = directory();
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {dir:?}: {e}"))?;
    let bytes = encode::config_dump(config);
    std::fs::write(&path, &bytes).map_err(|e| format!("could not write {path:?}: {e}"))
}

/// Loads a preset by name.
pub fn load(name: &str) -> Result<Config, String> {
    read(&path_for(name)?)
}

/// Deletes a preset by name.
pub fn delete(name: &str) -> Result<(), String> {
    let path = path_for(name)?;
    std::fs::remove_file(&path).map_err(|e| format!("could not delete {path:?}: {e}"))
}
