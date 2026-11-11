//! Same-volume, no-overwrite moves. The running image is kept as the rollback
//! copy. The new process proves it loaded before the old UI releases its mutex.
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

fn move_new(source: &Path, target: &Path) -> io::Result<()> {
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if unsafe {
        MoveFileExW(
            wide(source).as_ptr(),
            wide(target).as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn staging_dir(exe: &Path) -> io::Result<PathBuf> {
    let parent = exe
        .parent()
        .ok_or_else(|| io::Error::other("找不到程序目录"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = parent.join(format!(".switcher-backup-{}-{stamp}", std::process::id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

pub fn replace(
    exe: &Path,
    bytes: &[u8],
    launch: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if !fs::symlink_metadata(exe)?.is_file() {
        return Err(io::Error::other("程序路径不是普通文件"));
    }
    let dir = staging_dir(exe)?;
    let staged = dir.join("incoming.tmp");
    let backup = dir.join("previous.exe");
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staged)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        move_new(exe, &backup)?;
        let recover = |error: io::Error| match move_new(&backup, exe) {
            Ok(()) => io::Error::other(format!("已恢复旧版：{error}")),
            Err(rollback) => io::Error::other(format!(
                "更新失败：{error}；无法恢复：{rollback}。旧版保存在 {}",
                backup.display()
            )),
        };
        move_new(&staged, exe).map_err(recover)?;
        if let Err(error) = launch(exe) {
            // Never delete the last known good executable, even if an antivirus
            // or another process prevents moving the failed replacement away.
            move_new(exe, &staged).map_err(|e| {
                io::Error::other(format!(
                    "新版无法启动：{error}；无法移开：{e}。旧版保存在 {}",
                    backup.display()
                ))
            })?;
            return Err(recover(error));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    let _ = fs::remove_dir(&dir); // succeeds only if no retained backup
    result
}

pub fn launch_waiting(target: &Path, version: &str) -> io::Result<Child> {
    let mut child = Command::new(target)
        .args(["--await-update", &std::process::id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout.take(128)).read_line(&mut line);
        let _ = tx.send(line);
    });
    if rx
        .recv_timeout(Duration::from_secs(12))
        .is_ok_and(|line| line.trim_end() == format!("READY {version}"))
    {
        Ok(child)
    } else {
        let _ = child.kill();
        let _ = child.wait();
        Err(io::Error::other("新版未完成启动握手"))
    }
}

pub fn await_parent(pid: u32) -> io::Result<()> {
    if pid == std::process::id() {
        return Err(io::Error::other("无效的父进程"));
    }
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut out = io::stdout().lock();
        writeln!(out, "READY {}", env!("CARGO_PKG_VERSION"))?;
        out.flush()?;
        if unsafe { WaitForSingleObject(handle, 60_000) } != WAIT_OBJECT_0 {
            return Err(io::Error::other("等待旧版退出超时"));
        }
        Ok(())
    })();
    unsafe {
        CloseHandle(handle);
    }
    result
}
