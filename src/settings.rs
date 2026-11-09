//! settings.json lives next to the old switcher's state, so labels, avatar colours and
//! quota history migrate without any import step. Unknown keys are preserved verbatim.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::data::Install;

pub const DEFAULT_ID: &str = "Default";

pub struct Settings {
    pub value: Value,
    pub path: PathBuf,
    dirty: bool,
}

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        // The old PowerShell switcher writes this file as UTF-8 with a BOM and reads it
        // with `Get-Content`; keeping the BOM is what stops PowerShell 5.1 from decoding
        // the Chinese account names as ANSI. A retry covers a concurrent rewrite.
        let mut value = Value::Object(Map::new());
        for attempt in 0..3 {
            let parsed = fs::read_to_string(&path)
                .ok()
                .map(|s| s.trim_start_matches('\u{feff}').to_owned())
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            if let Some(parsed) = parsed {
                if parsed.is_object() {
                    value = parsed;
                    break;
                }
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        let value = if value.is_object() {
            value
        } else {
            Value::Object(Map::new())
        };
        Self {
            value,
            path,
            dirty: false,
        }
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.value) {
            let text = format!("\u{feff}{text}");
            let tmp = self.path.with_extension("json.tmp");
            let result = fs::write(&tmp, text.as_bytes())
                .and_then(|_| fs::rename(&tmp, &self.path));
            if result.is_err() {
                // Fallback for a locked target; still write the BOM so PowerShell reads it.
                let _ = fs::write(&self.path, text.as_bytes());
                let _ = fs::remove_file(&tmp);
            }
            self.dirty = false;
        }
    }

    fn object(&self, key: &str) -> Option<&Map<String, Value>> {
        self.value.get(key).and_then(Value::as_object)
    }

    fn object_mut(&mut self, key: &str) -> &mut Map<String, Value> {
        if !self.value.is_object() {
            self.value = Value::Object(Map::new());
        }
        let root = self.value.as_object_mut().expect("object");
        if !root.get(key).is_some_and(Value::is_object) {
            root.insert(key.to_owned(), Value::Object(Map::new()));
        }
        root.get_mut(key)
            .and_then(Value::as_object_mut)
            .expect("object")
    }

    // ---- labels ----

    pub fn labels(&self) -> BTreeMap<String, String> {
        self.object("Labels")
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn label_for(&self, id: &str) -> Option<String> {
        self.labels().get(id).cloned()
    }

    pub fn set_label(&mut self, id: &str, label: Option<&str>) {
        let label = label.map(str::trim).filter(|s| !s.is_empty() && *s != id);
        let map = self.object_mut("Labels");
        match label {
            Some(text) => {
                if map.get(id).and_then(Value::as_str) != Some(text) {
                    map.insert(id.to_owned(), Value::String(text.to_owned()));
                    self.dirty = true;
                }
            }
            None => {
                if map.remove(id).is_some() {
                    self.dirty = true;
                }
            }
        }
    }

    // ---- avatar colours ----

    pub fn color_index(&self, id: &str) -> Option<usize> {
        self.object("Colors")
            .and_then(|m| m.get(id))
            .and_then(Value::as_i64)
            .map(|n| n.max(0) as usize)
    }

    /// Assign a palette slot the first time an account is seen, spreading accounts over
    /// the palette instead of letting a hash collide. Matches the old switcher's rule.
    pub fn ensure_color(&mut self, id: &str, alive: &[String]) -> usize {
        if let Some(index) = self.color_index(id) {
            return index;
        }
        let mut used = [0usize; 8];
        if let Some(map) = self.object("Colors") {
            for (key, value) in map {
                if let Some(slot) = value.as_i64() {
                    if alive.iter().any(|a| a == key) {
                        used[(slot.max(0) as usize) % 8] += 1;
                    }
                }
            }
        }
        let mut pick = 0usize;
        for i in 1..8 {
            if used[i] < used[pick] {
                pick = i;
            }
        }
        self.object_mut("Colors")
            .insert(id.to_owned(), Value::Number(pick.into()));
        self.dirty = true;
        pick
    }

    pub fn forget_color(&mut self, id: &str) {
        if self.object_mut("Colors").remove(id).is_some() {
            self.dirty = true;
        }
    }

    // ---- install cache ----

    pub fn install_cache(&self) -> Option<Install> {
        let map = self.object("InstallCache")?;
        let exe = map.get("Exe")?.as_str()?.to_owned();
        let kind = match map.get("Kind").and_then(Value::as_str) {
            Some("Msix") => crate::data::InstallKind::Msix,
            _ => crate::data::InstallKind::Classic,
        };
        let version = map
            .get("Version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let aumid = map
            .get("AppUserModelId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let default_profile = map
            .get("DefaultProfilePath")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let mut install = Install {
            kind,
            exe: PathBuf::from(exe),
            version,
            aumid,
            pfn: None,
            default_profile: default_profile?,
        };
        install.refresh_identity();
        Some(install)
    }

    pub fn set_install_cache(&mut self, install: &Install) {
        let mut map = Map::new();
        map.insert(
            "Kind".into(),
            Value::String(match install.kind {
                crate::data::InstallKind::Msix => "Msix".into(),
                crate::data::InstallKind::Classic => "Classic".into(),
            }),
        );
        map.insert(
            "Exe".into(),
            Value::String(install.exe.to_string_lossy().into_owned()),
        );
        map.insert("Version".into(), Value::String(install.version.clone()));
        map.insert(
            "AppUserModelId".into(),
            install
                .aumid
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "DefaultProfilePath".into(),
            Value::String(install.default_profile.to_string_lossy().into_owned()),
        );
        let root = self.value.as_object_mut().expect("object");
        root.insert("InstallCache".into(), Value::Object(map));
        self.dirty = true;
    }

    // ---- window / behaviour ----

    pub fn tray_on_close(&self) -> bool {
        self.value
            .get("TrayOnClose")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn set_tray_on_close(&mut self, value: bool) {
        let root = self.value.as_object_mut().expect("object");
        if root.get("TrayOnClose").and_then(Value::as_bool) != Some(value) {
            root.insert("TrayOnClose".into(), Value::Bool(value));
            self.dirty = true;
        }
    }

    pub fn tray_hint_shown(&self) -> bool {
        self.value
            .get("TrayHintShown")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn last_account(&self) -> Option<String> {
        self.value
            .get("LastAccount")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub fn set_last_account(&mut self, id: &str) {
        let root = self.value.as_object_mut().expect("object");
        if root.get("LastAccount").and_then(Value::as_str) != Some(id) {
            root.insert("LastAccount".into(), Value::String(id.to_owned()));
            self.dirty = true;
        }
    }

    pub fn theme_pref(&self) -> String {
        self.value
            .get("Theme")
            .and_then(Value::as_str)
            .unwrap_or("system")
            .to_owned()
    }

    pub fn set_theme(&mut self, theme: &str) {
        let root = self.value.as_object_mut().expect("object");
        if root.get("Theme").and_then(Value::as_str) != Some(theme) {
            root.insert("Theme".into(), Value::String(theme.to_owned()));
            self.dirty = true;
        }
    }

    pub fn set_tray_hint_shown(&mut self) {
        let root = self.value.as_object_mut().expect("object");
        if root.get("TrayHintShown").and_then(Value::as_bool) != Some(true) {
            root.insert("TrayHintShown".into(), Value::Bool(true));
            self.dirty = true;
        }
    }

    // ---- quota watch ----

    pub fn quota_watch(&self) -> Map<String, Value> {
        self.object("QuotaWatch").cloned().unwrap_or_default()
    }

    pub fn set_quota_watch(&mut self, watch: Map<String, Value>) {
        let root = self.value.as_object_mut().expect("object");
        root.insert("QuotaWatch".into(), Value::Object(watch));
        self.dirty = true;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
