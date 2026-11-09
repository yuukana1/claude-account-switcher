//! Read-only view of everything Claude and the old switcher left on this machine:
//! install location, profiles, Claude Code conversation index files, and usage history.
//!
//! Nothing in this module deletes or rewrites profile data. The only writes are the
//! settings file (through `Settings`) and an explicit conversation copy in `copy_session`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Timelike};
use serde_json::Value;

use crate::settings::Settings;

pub const DEFAULT_ID: &str = "Default";
pub const PROFILE_PREFIX: &str = "ClaudeProfile-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Msix,
    Classic,
}

#[derive(Clone, Debug)]
pub struct Install {
    pub kind: InstallKind,
    pub exe: PathBuf,
    pub version: String,
    pub aumid: Option<String>,
    pub pfn: Option<String>,
    pub default_profile: PathBuf,
}

impl Install {
    pub fn available(&self) -> bool {
        self.exe.is_file()
    }

    pub fn refresh_identity(&mut self) {
        if self.pfn.is_none() {
            self.pfn = self
                .aumid
                .as_deref()
                .and_then(|a| a.split_once('!').map(|(p, _)| p.to_owned()))
                .or_else(|| pfn_from_path(&self.exe));
        }
        if self.aumid.is_none() {
            if let Some(pfn) = &self.pfn {
                self.aumid = Some(format!("{pfn}!Claude"));
            }
        }
        if self.default_profile.as_os_str().is_empty() {
            self.default_profile = match self.kind {
                InstallKind::Msix => {
                    let pfn = self.pfn.clone().unwrap_or_default();
                    paths()
                        .local
                        .join("Packages")
                        .join(pfn)
                        .join("LocalCache")
                        .join("Roaming")
                        .join("Claude")
                }
                InstallKind::Classic => paths().roaming.join("Claude"),
            };
        }
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub profiles: PathBuf,
    pub switcher: PathBuf,
    pub settings: PathBuf,
}

pub fn paths() -> Paths {
    let local = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir());
    let roaming = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| local.clone());
    let profiles = local.join("ClaudeProfiles");
    let switcher = profiles.join(".switcher");
    let settings = profiles.join("settings.json");
    Paths {
        local,
        roaming,
        profiles,
        switcher,
        settings,
    }
}

// --------------------------------------------------------------- install ----

pub fn discover_install(cached: Option<Install>) -> Install {
    let mut stale: Option<Install> = None;
    if let Some(mut install) = cached {
        install.refresh_identity();
        if install.available() {
            return install;
        }
        stale = Some(install);
    }

    // Ask the per-user package repository first: `C:\Program Files\WindowsApps` cannot be
    // listed by a normal process, so after a Store update moved Claude to a new versioned
    // folder the directory scan below sees nothing and the old cached path is all we have.
    if let Some(install) = crate::win::query_appx_install() {
        return install;
    }
    if let Some(install) = find_msix() {
        return install;
    }
    if let Some(install) = find_classic() {
        return install;
    }

    // Keep the identity of a cached Store install even if an update removed the versioned
    // path; the shell AppUserModelId can still start the default account.
    if let Some(install) = stale {
        if install.kind == InstallKind::Msix && install.aumid.is_some() {
            return install;
        }
    }

    // Last resort: we still know where the Store build keeps its data, which lets the
    // account list, quota and conversation views work even before Claude is launchable.
    let pfn = claude_data_dirs(None)
        .first()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let pfn = pfn.or_else(|| Some("Claude_pzs8sxrjxfjjc".into()));
    let mut install = Install {
        kind: InstallKind::Msix,
        exe: PathBuf::new(),
        version: "unknown".into(),
        aumid: pfn.as_ref().map(|p| format!("{p}!Claude")),
        pfn,
        default_profile: PathBuf::new(),
    };
    install.refresh_identity();
    install
}

fn find_msix() -> Option<Install> {
    let root = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Program Files"))
        .join("WindowsApps");
    let entries = fs::read_dir(&root).ok()?;
    let mut best: Option<(SystemTime, Install)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("Claude_") || !name.contains("__") {
            continue;
        }
        let exe = entry.path().join("app").join("Claude.exe");
        if !exe.is_file() {
            continue;
        }
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() < 5 {
            continue;
        }
        let version = parts[1].to_owned();
        let pfn = format!("{}_{}", parts[0], parts[parts.len() - 1]);
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        let mut install = Install {
            kind: InstallKind::Msix,
            exe,
            version,
            aumid: Some(format!("{pfn}!Claude")),
            pfn: Some(pfn),
            default_profile: PathBuf::new(),
        };
        install.refresh_identity();
        if best.as_ref().is_none_or(|(m, _)| modified > *m) {
            best = Some((modified, install));
        }
    }
    best.map(|(_, install)| install)
}

fn find_classic() -> Option<Install> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for hive in [winreg::enums::HKEY_CURRENT_USER, winreg::enums::HKEY_LOCAL_MACHINE] {
        for view in [
            winreg::enums::KEY_WOW64_64KEY,
            winreg::enums::KEY_WOW64_32KEY,
        ] {
            let key = winreg::RegKey::predef(hive).open_subkey_with_flags(
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                winreg::enums::KEY_READ | view,
            );
            if let Ok(key) = key {
                for name in key.enum_keys().flatten() {
                    if let Ok(sub) = key.open_subkey(&name) {
                        let display: String = sub.get_value("DisplayName").unwrap_or_default();
                        if display.to_lowercase().contains("claude") {
                            if let Ok(location) = sub.get_value::<String, _>("InstallLocation") {
                                if !location.trim().is_empty() {
                                    dirs.push(PathBuf::from(location));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let local = paths().local;
    dirs.push(local.join("AnthropicClaude"));
    dirs.push(local.join("Programs").join("Claude"));
    if let Some(pf) = env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(pf).join("Claude"));
    }
    if let Some(pf) = env::var_os("ProgramFiles(x86)") {
        dirs.push(PathBuf::from(pf).join("Claude"));
    }

    for dir in dirs {
        let direct = dir.join("Claude.exe");
        if direct.is_file() {
            let mut install = Install {
                kind: InstallKind::Classic,
                exe: direct,
                version: "unknown".into(),
                aumid: None,
                pfn: None,
                default_profile: paths().roaming.join("Claude"),
            };
            install.refresh_identity();
            return Some(install);
        }
        if let Some(exe) = find_exe_shallow(&dir, 2) {
            let mut install = Install {
                kind: InstallKind::Classic,
                exe,
                version: "unknown".into(),
                aumid: None,
                pfn: None,
                default_profile: paths().roaming.join("Claude"),
            };
            install.refresh_identity();
            return Some(install);
        }
    }
    None
}

fn find_exe_shallow(dir: &Path, depth: u32) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Claude.exe")) {
            return Some(path);
        }
        if path.is_dir() {
            dirs.push(path);
        }
    }
    for sub in dirs {
        if let Some(found) = find_exe_shallow(&sub, depth - 1) {
            return Some(found);
        }
    }
    None
}

fn pfn_from_path(exe: &Path) -> Option<String> {
    for part in exe.components() {
        let text = part.as_os_str().to_string_lossy();
        if text.starts_with("Claude_") && text.contains("__") {
            let pieces: Vec<&str> = text.split('_').collect();
            if pieces.len() >= 5 {
                return Some(format!("{}_{}", pieces[0], pieces[pieces.len() - 1]));
            }
        }
    }
    None
}

pub fn claude_data_dirs(pfn: Option<&str>) -> Vec<PathBuf> {
    let base = paths().local.join("Claude-Data");
    let Ok(entries) = fs::read_dir(&base) else {
        return Vec::new();
    };
    let dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    if let Some(pfn) = pfn {
        let ours: Vec<PathBuf> = dirs
            .iter()
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().eq_ignore_ascii_case(pfn))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !ours.is_empty() {
            return ours;
        }
    }
    if dirs.len() == 1 {
        dirs
    } else {
        Vec::new()
    }
}

// --------------------------------------------------------------- accounts ----

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub is_default: bool,
    pub color: usize,
    pub running_pid: Option<u32>,
    pub last_used: Option<SystemTime>,
    pub session_count: usize,
    pub data_path: Option<PathBuf>,
}

pub fn discover_accounts(install: &Install, settings: &mut Settings) -> Vec<Account> {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    ids.insert(DEFAULT_ID.to_owned());

    if let Ok(entries) = fs::read_dir(&paths().local) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_prefix(PROFILE_PREFIX) {
                if !id.is_empty() && entry.path().is_dir() {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&paths().profiles) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().is_dir() {
                ids.insert(name);
            }
        }
    }

    let alive: Vec<String> = ids.iter().cloned().collect();
    let mut result = Vec::new();
    for id in ids {
        let path = resolve_profile_path(install, &id);
        let name = settings.label_for(&id).unwrap_or_else(|| id.clone());
        let color = settings.ensure_color(&id, &alive);
        result.push(Account {
            id,
            name,
            path,
            is_default: false,
            color,
            running_pid: None,
            last_used: None,
            session_count: 0,
            data_path: None,
        });
    }
    for account in &mut result {
        account.is_default = account.id == DEFAULT_ID;
    }
    result
}

pub fn resolve_profile_path(install: &Install, id: &str) -> PathBuf {
    let local = paths().local;
    if id == DEFAULT_ID {
        return install.default_profile.clone();
    }
    let current = local.join(format!("{PROFILE_PREFIX}{id}"));
    let legacy = paths().profiles.join(id);
    if legacy.is_dir() && !current.join("config.json").is_file() {
        legacy
    } else {
        current
    }
}

pub fn refresh_metadata(install: &Install, account: &mut Account) {
    let roots = profile_data_roots(install, account);
    let mut newest: Option<SystemTime> = None;
    for root in &roots {
        for name in ["Preferences", "Local State", "config.json", "window-state.json"] {
            if let Ok(meta) = fs::metadata(root.join(name)) {
                if let Ok(stamp) = meta.modified() {
                    newest = Some(newest.map_or(stamp, |n| n.max(stamp)));
                }
            }
        }
        if let Ok(meta) = fs::metadata(root.join("Network").join("Cookies")) {
            if let Ok(stamp) = meta.modified() {
                newest = Some(newest.map_or(stamp, |n| n.max(stamp)));
            }
        }
    }
    account.last_used = newest;
    account.session_count = count_sessions(install, account);
    account.data_path = roots.into_iter().find(|p| p.is_dir());
}

pub fn profile_data_roots(install: &Install, account: &Account) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        let key = normalize_key(&path);
        if !roots.iter().any(|p| normalize_key(p) == key) {
            roots.push(path);
        }
    };

    if account.is_default {
        if install.kind == InstallKind::Msix {
            for dir in claude_data_dirs(install.pfn.as_deref()) {
                push(dir);
            }
        }
        if !account.path.as_os_str().is_empty() {
            push(account.path.clone());
        }
        if !install.default_profile.as_os_str().is_empty() {
            push(install.default_profile.clone());
        }
        return roots;
    }

    push(account.path.clone());
    if install.kind == InstallKind::Msix {
        let local = paths().local;
        if let Ok(rel) = account.path.strip_prefix(&local) {
            let pfn = install.pfn.clone().unwrap_or_default();
            let redirected = local
                .join("Packages")
                .join(pfn)
                .join("LocalCache")
                .join("Local")
                .join(rel);
            if redirected.is_dir() {
                push(redirected);
            }
        }
    }
    roots
}

pub fn normalize_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

pub fn session_roots(install: &Install, account: &Account) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for data in profile_data_roots(install, account) {
        let root = data.join("claude-code-sessions");
        if root.is_dir() {
            roots.push(root);
        }
    }
    roots
}

pub fn count_sessions(install: &Install, account: &Account) -> usize {
    let mut names: HashSet<OsString> = HashSet::new();
    for root in session_roots(install, account) {
        for account_dir in dirs(&root) {
            for org_dir in dirs(&account_dir) {
                if let Ok(entries) = fs::read_dir(&org_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let text = name.to_string_lossy();
                        if text.starts_with("local_") && text.ends_with(".json") {
                            names.insert(name);
                        }
                    }
                }
            }
        }
    }
    names.len()
}

fn dirs(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

// -------------------------------------------------------------- sessions ----

#[derive(Clone, Debug)]
pub struct Session {
    pub file: PathBuf,
    pub account: String,
    pub org: String,
    pub title: String,
    pub folder: String,
    pub updated: SystemTime,
    pub archived: bool,
}

pub fn list_sessions(install: &Install, account: &Account) -> Vec<Session> {
    let mut found: HashMap<OsString, Session> = HashMap::new();
    for root in session_roots(install, account) {
        for account_dir in dirs(&root) {
            let account_id = account_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            for org_dir in dirs(&account_dir) {
                let org_id = org_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let archived = archived_ids(&org_dir);
                let Ok(entries) = fs::read_dir(&org_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name();
                    let text = name.to_string_lossy();
                    if !path.is_file() || !text.starts_with("local_") || !text.ends_with(".json") {
                        continue;
                    }
                    let data = fs::read_to_string(&path)
                        .ok()
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
                    let title = data
                        .as_ref()
                        .and_then(|v| first_string(v, &["title", "name", "summary", "customTitle", "displayName"]))
                        .unwrap_or_else(|| fallback_title(&text));
                    let folder = data
                        .as_ref()
                        .and_then(|v| first_string(v, &["cwd", "workingDirectory", "projectPath", "folder", "path"]))
                        .map(|p| folder_label(&p))
                        .unwrap_or_default();
                    let stem = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let archived = archived.contains(&stem)
                        || data
                            .as_ref()
                            .and_then(|v| v.get("isArchived"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                    let updated = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(UNIX_EPOCH);
                    let session = Session {
                        file: path,
                        account: account_id.clone(),
                        org: org_id.clone(),
                        title,
                        folder,
                        updated,
                        archived,
                    };
                    match found.get(&name) {
                        Some(existing) if existing.updated >= session.updated => {}
                        _ => {
                            found.insert(name, session);
                        }
                    }
                }
            }
        }
    }
    let mut list: Vec<Session> = found.into_values().collect();
    list.sort_by(|a, b| b.updated.cmp(&a.updated));
    list
}

fn archived_ids(org_dir: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let path = org_dir.join("archived-sessions.idx");
    let Ok(text) = fs::read_to_string(path) else {
        return set;
    };
    let text = text.trim_start_matches('\u{feff}');
    if let Ok(data) = serde_json::from_str::<Value>(text) {
        if let Some(list) = data.get("archived").and_then(Value::as_array) {
            for item in list {
                if let Some(id) = item.as_str() {
                    set.insert(id.to_owned());
                }
            }
        }
    }
    set
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_owned());
            }
        }
    }
    None
}

fn fallback_title(file_name: &str) -> String {
    let stem = file_name.strip_suffix(".json").unwrap_or(file_name);
    let stem = stem.strip_prefix("local_").unwrap_or(stem);
    let short: String = stem.chars().take(8).collect();
    format!("对话 {short}")
}

fn folder_label(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if path.contains("\\scratch-workspaces\\") {
        return "(no folder)".into();
    }
    path.split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .next_back()
        .unwrap_or("")
        .to_owned()
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    pub path: PathBuf,
    pub account: String,
    pub org: String,
}

pub fn session_store(install: &Install, account: &Account) -> Option<SessionStore> {
    for root in session_roots(install, account) {
        let mut best: Option<(SystemTime, PathBuf)> = None;
        for account_dir in dirs(&root) {
            for org_dir in dirs(&account_dir) {
                let stamp = fs::metadata(&org_dir)
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH);
                if best.as_ref().is_none_or(|(t, _)| stamp > *t) {
                    best = Some((stamp, org_dir));
                }
            }
        }
        if let Some((_, org_dir)) = best {
            let account_id = org_dir
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let org_id = org_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Some(SessionStore {
                path: org_dir,
                account: account_id,
                org: org_id,
            });
        }
    }
    None
}

pub fn copy_session(session: &Session, store: &SessionStore) -> io::Result<bool> {
    let name = session
        .file
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session has no file name"))?;
    let dest = store.path.join(name);
    if dest.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(&session.file)?;
    let text = text
        .replace(
            &format!("\"{}\"", session.account),
            &format!("\"{}\"", store.account),
        )
        .replace(
            &format!("\"{}\"", session.org),
            &format!("\"{}\"", store.org),
        );
    fs::write(&dest, text)?;
    Ok(true)
}

// ----------------------------------------------------------------- usage ----

#[derive(Clone, Debug)]
pub struct Usage {
    pub fh: Option<i64>,
    pub sd: Option<i64>,
    pub t: i64,
    pub updated: DateTime<Local>,
    pub org: String,
    pub fh_active: bool,
    /// A reset was observed and no message has used the new window yet.
    pub fh_reset_pending: bool,
    pub fh_reset_at: Option<DateTime<Local>>,
    pub sd_anchor: Option<DateTime<Local>>,
    pub chart: Vec<(i64, Option<i64>, Option<i64>)>,
}

impl Usage {
    pub fn fh_row(&self, now: DateTime<Local>) -> (Option<i64>, String) {
        if !self.fh_active {
            if self.fh_reset_pending {
                return (Some(0), "已重置 · 待使用".into());
            }
            return (self.fh, "下次使用后计时".into());
        }
        if let Some(reset) = self.fh_reset_at {
            if now < reset {
                return (self.fh, format_reset_hint(reset, now));
            }
            if self.updated <= reset {
                return (Some(0), "已重置 · 待使用".into());
            }
        }
        (self.fh, "—".into())
    }

    pub fn sd_row(&self, now: DateTime<Local>) -> (Option<i64>, String) {
        let Some(anchor) = self.sd_anchor else {
            return (self.sd, "—".into());
        };
        let week = chrono::Duration::days(7);
        let mut last = anchor;
        while last + week <= now {
            last += week;
        }
        let next = last + week;
        if self.updated <= last {
            (
                Some(0),
                format!("已重置 · 下次 {}", next.format("%m-%d %H:%M")),
            )
        } else {
            (self.sd, format_reset_hint(next, now))
        }
    }
}

pub fn read_usage(install: &Install, account: &Account) -> Option<Usage> {
    let mut file: Option<PathBuf> = None;
    for root in profile_data_roots(install, account) {
        let candidate = root.join("plan-usage-history.json");
        if candidate.is_file() {
            file = Some(candidate);
            break;
        }
    }
    let path = file?;
    let text = fs::read_to_string(&path).ok()?;
    let data: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let raw = data.get("samples")?.as_array()?;

    let mut samples: Vec<(i64, String, Option<i64>, Option<i64>)> = Vec::new();
    for item in raw {
        let t = item.get("t").and_then(Value::as_i64);
        let u = item.get("u");
        let fh = u.and_then(|u| u.get("fh")).and_then(Value::as_i64);
        let sd = u.and_then(|u| u.get("sd")).and_then(Value::as_i64);
        if t.is_none() || (fh.is_none() && sd.is_none()) {
            continue;
        }
        samples.push((
            t.unwrap(),
            item.get("org")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            fh,
            sd,
        ));
    }
    if samples.is_empty() {
        return None;
    }

    let last = samples.last().unwrap();
    let mut reset_idx: Option<usize> = None;
    for i in (1..samples.len()).rev() {
        let prev = &samples[i - 1];
        let cur = &samples[i];
        if !prev.1.is_empty() && !cur.1.is_empty() && prev.1 != cur.1 {
            continue;
        }
        if let (Some(pfh), Some(cfh)) = (prev.2, cur.2) {
            if cfh <= 40 && cfh <= pfh - 15 {
                reset_idx = Some(i);
                break;
            }
        }
    }

    let mut fh_active = false;
    let mut fh_start: Option<DateTime<Local>> = None;
    if let Some(idx) = reset_idx {
        let boundary = &samples[idx];
        if boundary.2.unwrap_or(0) > 0 {
            fh_start = Some(midpoint(samples[idx - 1].0, boundary.0));
            fh_active = true;
        } else {
            for j in (idx + 1)..samples.len() {
                if samples[j].2.unwrap_or(0) > 0 {
                    fh_start = Some(midpoint(samples[j - 1].0, samples[j].0));
                    fh_active = true;
                    break;
                }
            }
        }
    }
    let fh_reset_at = fh_start.map(|start| {
        round_to_minutes(start + chrono::Duration::hours(5), 5)
    });

    let mut sd_anchor: Option<DateTime<Local>> = None;
    for i in (1..samples.len()).rev() {
        let prev = &samples[i - 1];
        let cur = &samples[i];
        if !prev.1.is_empty() && !cur.1.is_empty() && prev.1 != cur.1 {
            continue;
        }
        if let (Some(psd), Some(csd)) = (prev.3, cur.3) {
            if csd <= 5 && csd <= psd - 5 {
                if let (Some(prev_dt), Some(cur_dt)) = (from_ms(prev.0), from_ms(cur.0)) {
                    let mut guess = midpoint(prev.0, cur.0);
                    let snap = round_to_minutes(guess, 15);
                    if snap > prev_dt && snap <= cur_dt {
                        guess = snap;
                    }
                    sd_anchor = Some(guess);
                    break;
                }
            }
        }
    }

    let chart: Vec<(i64, Option<i64>, Option<i64>)> = samples
        .iter()
        .map(|(t, _, fh, sd)| (*t, *fh, *sd))
        .collect();

    Some(Usage {
        fh: last.2,
        sd: last.3,
        t: last.0,
        updated: from_ms(last.0).unwrap_or_else(Local::now),
        org: last.1.clone(),
        fh_active,
        fh_reset_pending: reset_idx.is_some() && !fh_active,
        fh_reset_at,
        sd_anchor,
        chart,
    })
}

fn from_ms(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

fn midpoint(prev_ms: i64, cur_ms: i64) -> DateTime<Local> {
    let gap = cur_ms - prev_ms;
    let value = if gap > 0 && gap <= 2 * 3600 * 1000 {
        (prev_ms + cur_ms) / 2
    } else {
        cur_ms
    };
    from_ms(value).unwrap_or_else(Local::now)
}

fn round_to_minutes(time: DateTime<Local>, minutes: i64) -> DateTime<Local> {
    let naive = time.naive_local();
    let seconds = naive.num_seconds_from_midnight() as i64;
    let step = minutes * 60;
    let mut floored = (seconds / step) * step;
    if seconds - floored >= step / 2 {
        floored += step;
    }
    let base = naive
        .date()
        .and_hms_opt(0, 0, 0)
        .or_else(|| Some(NaiveDateTime::new(naive.date(), chrono::NaiveTime::MIN)))
        .expect("midnight always exists");
    let rounded = base + chrono::Duration::seconds(floored);
    Local
        .from_local_datetime(&rounded)
        .single()
        .unwrap_or(time)
}

pub fn format_reset_hint(time: DateTime<Local>, now: DateTime<Local>) -> String {
    if time.date_naive() == now.date_naive() {
        format!("约 {} 重置", time.format("%H:%M"))
    } else if time.date_naive() == now.date_naive() + chrono::Duration::days(1) {
        format!("约 明天 {} 重置", time.format("%H:%M"))
    } else {
        format!("约 {} 重置", time.format("%m-%d %H:%M"))
    }
}

/// The soonest quota reset the reader can predict for this account.
#[derive(Clone, Debug)]
pub struct ResetInfo {
    /// "5 小时" or "本周"
    pub label: &'static str,
    pub at: DateTime<Local>,
    pub minutes: i64,
}

impl ResetInfo {
    /// True when the reset is close enough to deserve emphasis in the UI.
    pub fn imminent(&self) -> bool {
        self.minutes <= 30
    }

    /// Human countdown: "12 分钟", "3 小时 20 分", "2 天 5 小时".
    pub fn countdown(&self) -> String {
        let minutes = self.minutes.max(0);
        if minutes < 60 {
            format!("{minutes} 分钟")
        } else if minutes < 24 * 60 {
            format!("{} 小时 {} 分", minutes / 60, minutes % 60)
        } else {
            format!("{} 天 {} 小时", minutes / (24 * 60), (minutes % (24 * 60)) / 60)
        }
    }
}

/// Whichever window resets first, for the "next reset" readout.
pub fn next_reset(usage: &Usage, now: DateTime<Local>) -> Option<ResetInfo> {
    let mut candidates: Vec<ResetInfo> = Vec::new();

    if usage.fh_active {
        if let Some(at) = usage.fh_reset_at {
            if at > now {
                candidates.push(ResetInfo {
                    label: "5 小时",
                    at,
                    minutes: (at - now).num_minutes(),
                });
            }
        }
    }
    if let Some(anchor) = usage.sd_anchor {
        let week = chrono::Duration::days(7);
        let mut last = anchor;
        while last + week <= now {
            last += week;
        }
        let next = last + week;
        candidates.push(ResetInfo {
            label: "本周",
            at: next,
            minutes: (next - now).num_minutes(),
        });
    }
    candidates
        .into_iter()
        .min_by_key(|candidate| candidate.minutes)
}

pub fn format_when(when: DateTime<Local>) -> String {    let now = Local::now();
    let delta = now - when;
    let minutes = delta.num_minutes();
    if minutes < 1 {
        return "刚刚".into();
    }
    if minutes < 60 {
        return format!("{minutes} 分钟前");
    }
    if when.date_naive() == now.date_naive() {
        return format!("今天 {}", when.format("%H:%M"));
    }
    if when.date_naive() == now.date_naive() - chrono::Duration::days(1) {
        return format!("昨天 {}", when.format("%H:%M"));
    }
    if when.year() == now.year() {
        return when.format("%m-%d %H:%M").to_string();
    }
    when.format("%Y-%m-%d").to_string()
}

pub fn system_time_to_local(time: SystemTime) -> DateTime<Local> {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    from_ms(millis).unwrap_or_else(Local::now)
}

pub fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

// A key used to match a launched profile directory against a process command line.
pub fn running_key(account: &Account) -> String {
    normalize_key(&account.path)
}

pub fn next_account_name(existing: &[Account]) -> String {
    for n in 2..1000 {
        let candidate = format!("账号 {n}");
        if !existing
            .iter()
            .any(|a| a.id == candidate || a.name == candidate)
        {
            return candidate;
        }
    }
    "新账号".into()
}

pub fn validate_label(name: &str, accounts: &[Account], except_id: Option<&str>) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空。".into());
    }
    if name.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err("名称不能包含 \\ / : * ? \" < > |".into());
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return Err("名称不能以点开头，或以点/空格结尾。".into());
    }
    if name.chars().count() > 40 {
        return Err("名称过长（最多 40 个字符）。".into());
    }
    let upper = name.to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
        "LPT7", "LPT8", "LPT9"];
    if reserved.contains(&upper.as_str()) {
        return Err(format!("'{name}' 是 Windows 保留名称。"));
    }
    if name == "Add account" {
        return Err("'Add account' 是保留名称，请换一个。".into());
    }
    for account in accounts {
        if Some(account.id.as_str()) == except_id {
            continue;
        }
        if account.id == name || account.name == name {
            return Err(format!("已存在名为 '{name}' 的账号。"));
        }
    }
    Ok(())
}

pub fn delete_account_dir(account: &Account) -> io::Result<()> {
    if account.is_default {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "默认账号不能删除。",
        ));
    }
    let local = paths().local;
    let profiles = paths().profiles;
    let current = local.join(format!("{PROFILE_PREFIX}{}", account.id));
    let legacy = profiles.join(&account.id);
    let path = if current.join("config.json").is_file() || !legacy.is_dir() {
        current
    } else {
        legacy
    };
    let leaf = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path.parent().map(normalize_key).unwrap_or_default();
    let ours = (parent == normalize_key(&local) && leaf == format!("{PROFILE_PREFIX}{}", account.id))
        || (parent == normalize_key(&profiles) && leaf == account.id);
    if !ours || account.id.starts_with('.') || account.id.contains(['\\', '/']) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("拒绝删除 '{}'。", path.display()),
        ));
    }
    if !path.exists() {
        return Ok(());
    }
    fs::remove_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn usage_with(fh_reset: DateTime<Local>, sd_anchor: DateTime<Local>) -> Usage {
        Usage {
            fh: Some(12),
            sd: Some(34),
            t: 0,
            updated: Local::now(),
            org: "org".into(),
            fh_active: true,
            fh_reset_pending: false,
            fh_reset_at: Some(fh_reset),
            sd_anchor: Some(sd_anchor),
            chart: Vec::new(),
        }
    }

    #[test]
    fn reset_info_prefers_the_sooner_window() {
        let now = Local::now();
        let usage = usage_with(now + Duration::minutes(5), now - Duration::days(6));
        let reset = next_reset(&usage, now).expect("reset");
        assert_eq!(reset.label, "5 小时");
        assert!(reset.imminent());
        assert_eq!(reset.countdown(), "5 分钟");
    }

    #[test]
    fn weekly_reset_wins_when_5h_is_far_away() {
        let now = Local::now();
        // Weekly anchor seven days ago means the next weekly reset is moments away.
        let usage = usage_with(
            now + Duration::hours(4),
            now - Duration::days(7) + Duration::minutes(90),
        );
        let reset = next_reset(&usage, now).expect("reset");
        assert_eq!(reset.label, "本周");
        assert_eq!(reset.minutes, 90);
    }

    #[test]
    fn countdown_shapes_read_naturally() {
        let now = Local::now();
        let cases = [
            (12, "12 分钟"),
            (75, "1 小时 15 分"),
            (26 * 60, "1 天 2 小时"),
        ];
        for (minutes, expected) in cases {
            let info = ResetInfo {
                label: "5 小时",
                at: now + Duration::minutes(minutes),
                minutes,
            };
            assert_eq!(info.countdown(), expected);
        }
    }

    #[test]
    fn imminence_threshold_is_half_an_hour() {
        let now = Local::now();
        let mk = |minutes| ResetInfo {
            label: "5 小时",
            at: now + Duration::minutes(minutes),
            minutes,
        };
        assert!(mk(30).imminent());
        assert!(!mk(31).imminent());
    }
}
