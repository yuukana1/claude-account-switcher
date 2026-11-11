//! settings.json lives next to the old switcher's state, so labels, avatar colours and
//! quota history migrate without any import step. Unknown keys are preserved verbatim.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::data::Install;

pub const DEFAULT_ID: &str = "Default";

pub struct Settings {
    value: Value,
    path: PathBuf,
    dirty: bool,
    load_error: Option<String>,
}

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        // The old PowerShell switcher writes this file as UTF-8 with a BOM and reads it
        // with `Get-Content`; keeping the BOM is what stops PowerShell 5.1 from decoding
        // the Chinese account names as ANSI. A retry covers a concurrent rewrite.
        let mut value = Value::Object(Map::new());
        let mut load_error = None;
        for attempt in 0..3 {
            let parsed = fs::read_to_string(&path).and_then(|text| {
                let parsed: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if !parsed.is_object() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "settings must be a JSON object",
                    ));
                }
                Ok(parsed)
            });
            match parsed {
                Ok(parsed) => {
                    value = parsed;
                    load_error = None;
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    load_error = None;
                    break;
                }
                Err(error) => load_error = Some(error.to_string()),
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        Self {
            value,
            path,
            dirty: false,
            load_error,
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        if let Some(error) = &self.load_error {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "无法读取设置 {}：{error}。原文件已保留，请修复文件后重新启动。",
                    self.path.display()
                ),
            ));
        }
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.value).map_err(io::Error::other)?;
        crate::storage::write_atomic(&self.path, format!("\u{feff}{text}").as_bytes(), true)?;
        self.dirty = false;
        Ok(())
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

    pub fn pinned(&self, id: &str) -> bool {
        self.object("PinnedAccounts")
            .and_then(|pins| pins.get(id))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        if self.pinned(id) == pinned {
            return;
        }
        let pins = self.object_mut("PinnedAccounts");
        if pinned {
            pins.insert(id.to_owned(), Value::Bool(true));
        } else {
            pins.remove(id);
        }
        self.dirty = true;
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

    pub fn daily_updates(&self) -> bool {
        self.value
            .get("DailyUpdateChecks")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn daily_update_due(&self, today: &str) -> bool {
        self.daily_updates() && self.update_check_date() != Some(today)
    }

    pub fn set_daily_updates(&mut self, enabled: bool) {
        self.value["DailyUpdateChecks"] = Value::Bool(enabled);
        self.dirty = true;
    }

    pub fn update_check_date(&self) -> Option<&str> {
        self.value.get("UpdateCheckDate").and_then(Value::as_str)
    }

    pub fn record_update_attempt(&mut self, day: &str) {
        if self.update_check_date() != Some(day) {
            self.value["UpdateCheckDate"] = Value::String(day.into());
            self.dirty = true;
        }
    }

    pub fn available_version(&self) -> Option<&str> {
        self.value.get("AvailableUpdate").and_then(Value::as_str)
    }

    pub fn set_available_version(&mut self, version: Option<&str>) {
        if self.available_version() != version {
            self.value["AvailableUpdate"] =
                version.map_or(Value::Null, |v| Value::String(v.into()));
            self.dirty = true;
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    #[test]
    fn save_preserves_unknown_keys_and_bom_with_unicode_labels() {
        let dir = TestDir::new();
        let path = dir.path().join("settings.json");
        let original = json!({"FutureFeature": {"enabled": true, "values": [1, "原值"]}});
        fs::write(&path, format!("\u{feff}{original}")).unwrap();
        let mut settings = Settings::load(path.clone());
        settings.set_label("Secondary", Some("  工作账号  "));
        settings.set_theme("dark");
        settings.save().unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        let reloaded = Settings::load(path);
        assert_eq!(reloaded.label_for("Secondary").as_deref(), Some("工作账号"));
        assert_eq!(reloaded.theme_pref(), "dark");
        assert_eq!(reloaded.value["FutureFeature"], original["FutureFeature"]);
    }

    #[test]
    fn malformed_existing_settings_are_never_replaced_with_defaults() {
        for original in ["{truncated", "[]", "null", "\u{feff}invalid"] {
            let dir = TestDir::new();
            let path = dir.path().join("settings.json");
            fs::write(&path, original).unwrap();
            let mut settings = Settings::load(path.clone());
            settings.set_theme("dark");
            let error = settings.save().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn failed_save_keeps_changes_dirty_for_retry_after_unlocking() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = TestDir::new();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\"Theme\":\"light\"}").unwrap();
        let mut settings = Settings::load(path.clone());
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        settings.set_theme("dark");
        assert!(settings.save().is_err());
        assert!(settings.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"Theme\":\"light\"}");
        drop(lock);
        settings.save().unwrap();
        assert!(!settings.dirty);
        assert_eq!(Settings::load(path).theme_pref(), "dark");
    }

    #[test]
    fn missing_settings_are_created_only_after_a_change() {
        let dir = TestDir::new();
        let path = dir.path().join("nested").join("settings.json");
        let mut settings = Settings::load(path.clone());
        settings.save().unwrap();
        assert!(!path.exists());
        settings.set_last_account("Secondary");
        settings.save().unwrap();
        assert_eq!(
            Settings::load(path).last_account().as_deref(),
            Some("Secondary")
        );
    }

    #[test]
    fn daily_update_attempt_survives_restart_and_respects_opt_out() {
        let dir = TestDir::new();
        let path = dir.path().join("settings.json");
        let mut s = Settings::load(path.clone());
        assert!(s.daily_update_due("2026-11-11"));
        s.record_update_attempt("2026-11-11");
        s.save().unwrap();
        let mut s = Settings::load(path.clone());
        assert!(!s.daily_update_due("2026-11-11"));
        assert!(s.daily_update_due("2026-11-12"));
        s.set_daily_updates(false);
        s.save().unwrap();
        assert!(!Settings::load(path).daily_update_due("2026-11-12"));
    }
}
