//! Keep a disabled legacy shortcut out of Windows' Startup folder. An unknown
//! extension there can still be opened by the shell, producing an Open With dialog.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DISABLED_LINK: &str = "Claude Profile Switcher.lnk.disabled";
// Shell Link header size followed by CLSID_ShellLink, in on-disk byte order.
const LINK_HEADER: &[u8] = &[
    0x4c, 0, 0, 0, 1, 0x14, 2, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46,
];

pub fn migrate_disabled_startup(startup: &Path, backup: &Path) -> io::Result<Option<PathBuf>> {
    let source = startup.join(DISABLED_LINK);
    let metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // Only handle the known legacy shortcut. Leave directories, links and unrelated
    // files alone, even when they have the same name.
    use std::os::windows::fs::MetadataExt;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Ok(None);
    }
    use std::io::Read;
    let mut header = [0; LINK_HEADER.len()];
    let mut file = fs::File::open(&source)?;
    match file.read_exact(&mut header) {
        Ok(()) if header == LINK_HEADER => {}
        Ok(()) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    drop(file);
    fs::create_dir_all(backup)?;
    for index in 0u32.. {
        let name = if index == 0 {
            DISABLED_LINK.to_owned()
        } else {
            format!("Claude Profile Switcher.{index}.lnk.disabled")
        };
        let destination = backup.join(name);
        // Explicitly omit REPLACE_EXISTING. std::fs::rename can replace a backup;
        // no cross-volume copy/delete fallback may remove the original on failure.
        match move_without_replacing(&source, &destination) {
            Ok(()) => return Ok(Some(destination)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("旧启动项备份名称已用尽"))
}

fn move_without_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn moves_only_disabled_legacy_link_and_preserves_existing_backups() {
        let dir = TestDir::new();
        let startup = dir.path().join("Startup");
        let backup = dir.path().join("backup");
        fs::create_dir_all(&startup).unwrap();
        fs::create_dir_all(&backup).unwrap();
        let mut bytes = LINK_HEADER.to_vec();
        bytes.extend_from_slice(b"legacy shortcut contents");
        fs::write(startup.join(DISABLED_LINK), &bytes).unwrap();
        fs::write(startup.join("Claude Profile Switcher.lnk"), b"enabled").unwrap();
        fs::write(startup.join("other.lnk.disabled"), &bytes).unwrap();
        fs::write(backup.join(DISABLED_LINK), b"previous backup").unwrap();
        let saved = migrate_disabled_startup(&startup, &backup)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(saved).unwrap(), bytes);
        assert_eq!(
            fs::read(backup.join(DISABLED_LINK)).unwrap(),
            b"previous backup"
        );
        assert!(!startup.join(DISABLED_LINK).exists());
        assert!(startup.join("Claude Profile Switcher.lnk").is_file());
        assert!(startup.join("other.lnk.disabled").is_file());
        assert!(migrate_disabled_startup(&startup, &backup)
            .unwrap()
            .is_none());
    }

    #[test]
    fn unrelated_or_missing_file_is_left_alone() {
        let dir = TestDir::new();
        let backup = dir.path().join("backup");
        assert!(migrate_disabled_startup(dir.path(), &backup)
            .unwrap()
            .is_none());
        fs::write(dir.path().join(DISABLED_LINK), b"not a shortcut").unwrap();
        assert!(migrate_disabled_startup(dir.path(), &backup)
            .unwrap()
            .is_none());
        assert!(dir.path().join(DISABLED_LINK).is_file());
        assert!(!backup.exists());
    }

    #[test]
    fn failed_backup_preserves_source() {
        let dir = TestDir::new();
        fs::write(dir.path().join(DISABLED_LINK), LINK_HEADER).unwrap();
        let backup = dir.path().join("not-a-directory");
        fs::write(&backup, b"keep").unwrap();
        assert!(migrate_disabled_startup(dir.path(), &backup).is_err());
        assert_eq!(
            fs::read(dir.path().join(DISABLED_LINK)).unwrap(),
            LINK_HEADER
        );
        assert_eq!(fs::read(backup).unwrap(), b"keep");
    }
}
