//! Runs real Windows images in a disposable directory. The handoff children
//! wait on this test process and never start the tray UI or access profiles.
#[path = "../src/update/install.rs"]
#[allow(dead_code)]
mod install;
#[path = "../src/test_support.rs"]
mod test_support;

use std::{fs, io, process::Child};
use test_support::TestDir;

struct Waiting(Child);
impl Drop for Waiting {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn running_portable_is_replaced_and_both_processes_survive_the_handoff() {
    let dir = TestDir::new();
    let exe = dir.path().join("切换器 with spaces.exe");
    let bytes = fs::read(env!("CARGO_BIN_EXE_claude-account-switcher")).unwrap();
    let mut old = bytes.clone();
    old.extend_from_slice(b"old fixture");
    fs::write(&exe, &old).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let mut before = Waiting(install::launch_waiting(&exe, version).unwrap());
    let mut after = None;
    install::replace(&exe, &bytes, |p| {
        after = Some(Waiting(install::launch_waiting(p, version)?));
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read(&exe).unwrap(), bytes);
    let backup = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .find(|e| e.path().is_dir())
        .unwrap()
        .path()
        .join("previous.exe");
    assert_eq!(fs::read(backup).unwrap(), old);
    assert!(after.as_mut().unwrap().0.try_wait().unwrap().is_none());
    assert!(before.0.try_wait().unwrap().is_none());
}

#[test]
fn bad_executable_or_wrong_version_restores_running_original() {
    let dir = TestDir::new();
    let exe = dir.path().join("original.exe");
    let bytes = fs::read(env!("CARGO_BIN_EXE_claude-account-switcher")).unwrap();
    fs::write(&exe, &bytes).unwrap();
    let mut running = Waiting(install::launch_waiting(&exe, env!("CARGO_PKG_VERSION")).unwrap());
    for candidate in [&b"bad EXE"[..], &bytes] {
        assert!(
            install::replace(&exe, candidate, |p| install::launch_waiting(p, "wrong")
                .map(drop))
            .is_err()
        );
        assert!(running.0.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&exe).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn locked_destination_is_left_intact() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = TestDir::new();
    let exe = dir.path().join("locked.exe");
    fs::write(&exe, b"original").unwrap();
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&exe)
        .unwrap();
    let fail_if_called = |_: &std::path::Path| -> io::Result<()> { panic!("must not launch") };
    assert!(install::replace(&exe, b"new", fail_if_called).is_err());
    assert_eq!(fs::read(exe).unwrap(), b"original");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
