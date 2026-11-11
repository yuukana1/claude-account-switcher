mod http;
pub mod install;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io;

const REPO: &str = "yuukana1/claude-account-switcher";
const MAX_EXE: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: usize,
    digest: Option<String>,
    browser_download_url: String,
    state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    pub size: usize,
    path: String,
    sha256: String,
}

pub(crate) fn parse_release(bytes: &[u8], current: &Version) -> Result<Option<Update>, String> {
    let r: Release = serde_json::from_slice(bytes).map_err(|e| format!("发行信息无法读取：{e}"))?;
    let version = r
        .tag_name
        .strip_prefix('v')
        .and_then(|s| Version::parse(s).ok())
        .ok_or("发行标签不是有效版本号")?;
    if r.draft || r.prerelease || !version.pre.is_empty() || !version.build.is_empty() {
        return Err("不是稳定发行版".into());
    }
    if &version <= current {
        return Ok(None);
    }
    if !cfg!(target_arch = "x86_64") {
        return Err("此架构没有匹配的更新包".into());
    }
    let name = format!("Claude-Account-Switcher-{version}-windows-x64.exe");
    let path = format!("/{REPO}/releases/download/v{version}/{name}");
    let mut candidates = r.assets.into_iter().filter(|a| a.name == name);
    let asset = candidates.next().ok_or("发行版缺少 Windows x64 便携版")?;
    if candidates.next().is_some()
        || asset.state != "uploaded"
        || asset.browser_download_url != format!("https://github.com{path}")
        || !(1..=MAX_EXE).contains(&asset.size)
    {
        return Err("发行附件的地址、状态或大小不正确".into());
    }
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("发行附件缺少有效的 SHA-256 校验值")?
        .to_ascii_lowercase();
    Ok(Some(Update {
        version,
        size: asset.size,
        path,
        sha256,
    }))
}

fn manifest() -> io::Result<Vec<u8>> {
    http::get(
        "github.com",
        &format!("/{REPO}/releases/latest/download/update.json"),
        64 * 1024,
        |_| {},
    )
}

pub fn check() -> Result<Option<Update>, String> {
    let bytes = manifest().or_else(|manifest_error| {
        http::get(
            "api.github.com",
            &format!("/repos/{REPO}/releases/latest"),
            1024 * 1024,
            |_| {},
        )
        .map_err(|api_error| {
            format!(
                "无法获取发行信息。更新清单：{manifest_error}；备用接口：{api_error}。请稍后重试。"
            )
        })
    })?;
    parse_release(
        &bytes,
        &Version::parse(env!("CARGO_PKG_VERSION")).expect("package version"),
    )
}

pub fn download(update: &Update, progress: impl FnMut(usize)) -> Result<Vec<u8>, String> {
    let bytes =
        http::get("github.com", &update.path, update.size, progress).map_err(|e| e.to_string())?;
    verify(update, &bytes)?;
    Ok(bytes)
}

fn verify(update: &Update, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != update.size || format!("{:x}", Sha256::digest(bytes)) != update.sha256 {
        return Err("安装包不完整或 SHA-256 不匹配，未替换程序".into());
    }
    Ok(())
}

pub fn apply(update: &Update, bytes: &[u8]) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    install::replace(&exe, bytes, |target| {
        install::launch_waiting(target, &update.version.to_string()).map(drop)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn metadata() -> Value {
        let name = "Claude-Account-Switcher-3.10.0-windows-x64.exe";
        json!({"tag_name":"v3.10.0","draft":false,"prerelease":false,"assets":[{
            "name":name,"state":"uploaded","size":4,"digest":format!("sha256:{:x}",Sha256::digest(b"data")),
            "browser_download_url":format!("https://github.com/{REPO}/releases/download/v3.10.0/{name}")}]})
    }
    fn parse(m: Value, v: &str) -> Result<Option<Update>, String> {
        parse_release(
            &serde_json::to_vec(&m).unwrap(),
            &Version::parse(v).unwrap(),
        )
    }
    #[test]
    fn accepts_only_newer_stable_matching_assets() {
        assert!(parse(metadata(), "3.9.9").unwrap().is_some());
        for current in ["3.10.0", "4.0.0"] {
            assert!(parse(metadata(), current).unwrap().is_none());
        }
        for (field, value) in [
            ("draft", json!(true)),
            ("prerelease", json!(true)),
            ("tag_name", json!("v3.10.0-beta.1")),
            ("tag_name", json!("../bad")),
        ] {
            let mut m = metadata();
            m[field] = value;
            assert!(parse(m, "3.0.0").is_err());
        }
        for (field, value) in [
            (
                "browser_download_url",
                json!("https://example.org/asset.exe"),
            ),
            ("digest", Value::Null),
            ("name", json!("linux")),
            ("size", json!(MAX_EXE + 1)),
            ("state", json!("new")),
        ] {
            let mut m = metadata();
            m["assets"][0][field] = value;
            assert!(parse(m, "3.0.0").is_err());
        }
    }
    #[test]
    fn download_requires_both_length_and_digest() {
        let u = parse(metadata(), "1.0.0").unwrap().unwrap();
        assert!(verify(&u, b"data").is_ok());
        for bad in [&b"dat"[..], b"dAta", b"data!"] {
            assert!(verify(&u, bad).is_err());
        }
    }
    #[test]
    #[ignore = "live GitHub check for release validation; no installation"]
    fn live_release_download_verifies() {
        let bytes = manifest().unwrap();
        let release = parse_release(&bytes, &Version::new(0, 0, 0))
            .unwrap()
            .unwrap();
        let exe = download(&release, |_| {}).unwrap();
        assert!(exe.starts_with(b"MZ"));
        println!(
            "verified v{}: {} bytes sha256:{}",
            release.version,
            exe.len(),
            release.sha256
        );
    }
}
