//! Account identity: the e-mail, display name and avatar the Claude client already
//! knows about. Read from the profile's own IndexedDB blob (the only place the desktop
//! app stores the signed-in account), never by touching the network.
//!
//! Nothing here writes to Claude's files. Optional user overrides live in settings.json.

use std::fs;
use std::path::{Path, PathBuf};

/// A slice of text found inside an IndexedDB blob. The blobs are binary encoder
/// streams, so a strict JSON parse is not possible; the fields are located by name and
/// validated by shape instead (an address with an @, a UUID, a short human name).
#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub email: Option<String>,
    pub name: Option<String>,
    pub user_id: Option<String>,
    pub org: Option<String>,
    pub org_name: Option<String>,
}

pub fn read_identity(payload: &[u8]) -> Identity {
    // The blobs mix UTF-8, binary length prefixes and arbitrary account data. Mapping
    // every byte to an ASCII character keeps all string indices on char boundaries and
    // turns the binary separators into line breaks for the scanner below.
    let text: String = payload
        .iter()
        .map(|byte| {
            if (0x20..=0x7E).contains(byte) {
                *byte as char
            } else {
                '\n'
            }
        })
        .collect();

    let mut identity = Identity {
        email: find_after(&text, "email_address", is_email),
        name: None,
        user_id: find_after(&text, "aggregated_id", is_user_id),
        org: find_after(&text, "org_uuid", is_uuid),
        org_name: find_after(&text, "organi", is_org_name),
    };
    identity.email = identity.email.map(|e| e.trim().to_lowercase());
    identity
}

/// A short, human-readable name derived from an e-mail address.
pub fn display_name_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let cleaned: String = local
        .chars()
        .map(|c| if c == '.' || c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let mut out = String::new();
    for word in cleaned.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        email.to_owned()
    } else {
        out
    }
}

fn find_after(text: &str, key: &str, accept: fn(&str) -> bool) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = text[from..].find(key) {
        let start = from + pos + key.len();
        // Skip the encoder's type/size token between the key and the value.
        let window = &text[start..text.len().min(start + 220)];
        for candidate in candidates(window) {
            let clean = candidate
                .trim()
                .trim_matches(['"', '\'', '\u{0}', ' ', ',', '{', '}']);
            if accept(clean) {
                return Some(clean.to_owned());
            }
        }
        from = start;
    }
    None
}

/// Reads the printable runs that follow the key, preferring the first plausible value.
fn candidates(window: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in window.chars() {
        let printable = ch.is_ascii_graphic() || ch == ' ' || (ch as u32) > 0x7F;
        if printable {
            current.push(ch);
        } else {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current.clear();
        }
        if out.len() >= 6 {
            break;
        }
    }
    if !current.trim().is_empty() && out.len() < 6 {
        out.push(current.trim().to_string());
    }
    out
}

fn is_email(value: &str) -> bool {
    let value = value.trim().trim_matches(['"', '\'', '\u{0}']);
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.len() > 3
        && value.len() < 120
        && !value.contains(' ')
        && !value.contains('\\')
        && !value.contains('/')
        && value
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '"')
}

fn is_uuid(value: &str) -> bool {
    let value = value.trim_matches(['"', '\'', ' ']);
    value.len() == 36
        && value.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn is_user_id(value: &str) -> bool {
    let value = value.trim_matches(['"', '\'', ' ']);
    value.starts_with("user_") && value.len() > 10 && value.len() < 60
}

fn is_human_name(value: &str) -> bool {
    let value = value.trim().trim_matches(['"', '\'', '\u{0}']);
    let count = value.chars().count();
    count >= 2
        && count <= 40
        && !value.contains('@')
        && !value.contains('\\')
        && !value.contains('/')
        && !value.contains("http")
        && !value.starts_with('{')
        && !value.starts_with('"')
        && !value.contains("name\"")
        && !value.contains('_')
        && value.chars().any(|c| c.is_alphabetic())
}

fn is_org_name(value: &str) -> bool {
    value.contains("Organization") && value.len() < 90 && !value.contains('"')
}

/// The account record under a profile. IndexedDB keeps several blobs per origin, so the
/// candidates are tried newest-first until one of them carries an account record.
pub fn identity_from_profile(roots: &[PathBuf]) -> Option<Identity> {
    for root in roots {
        let idb = root.join("IndexedDB");
        let mut blobs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        collect_blobs(&idb, 0, &mut blobs);
        blobs.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in blobs {
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            if !bytes.windows(8).any(|w| w == b"email_ad") {
                continue;
            }
            let identity = read_identity(&bytes);
            if identity.email.is_some() || identity.name.is_some() {
                return Some(identity);
            }
        }
    }
    None
}

fn collect_blobs(dir: &Path, depth: usize, out: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_blobs(&path, depth + 1, out);
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        // Account records are large; tiny index files and lock files are skipped.
        if meta.len() < 20_000 || meta.len() > 64 * 1024 * 1024 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".lock") || name == "LOCK" {
            continue;
        }
        let stamp = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        out.push((stamp, path));
    }
}

/// Avatar the user picked, stored next to the switcher's own files.
pub fn custom_avatar_path(account_id: &str) -> Option<PathBuf> {
    let base = avatar_dir()?;
    for ext in ["png", "jpg", "jpeg"] {
        let path = base.join(format!("{}.{ext}", safe_name(account_id)));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn avatar_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("ClaudeProfiles")
            .join(".switcher")
            .join("avatars"),
    )
}

fn safe_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}
