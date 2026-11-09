//! Windows integration: process/window discovery, launching Claude, single instance,
//! DWM chrome and the opt-in startup entry. Kept in one place so the UI never touches
//! unsafe Win32 directly.
//!
//! Two notes on safety:
//! - Claude's command line is read with a read-only kernel query (`NtQueryInformationProcess`)
//!   for the same user's processes, never through a shell.
//! - Launches go through `ShellExecuteW` with validated local paths and fixed verbs; no
//!   user-supplied text is ever composed into a shell command line.

use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, RECT,
};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    GWL_EXSTYLE, GWL_STYLE, SPI_GETCLIENTAREAANIMATION, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE, SW_HIDE, SW_SHOW,
    SW_SHOWNOACTIVATE, SW_SHOWNORMAL, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX,
    WS_SYSMENU,
};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

use crate::data::{paths, Install, InstallKind, PROFILE_PREFIX};

const WINDOW_TITLE: &str = "Claude 账号切换器";
const STARTUP_VALUE: &str = "Claude Account Switcher";
const STARTUP_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPX_PACKAGES: &str =
    "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub exe: String,
    pub cmdline: String,
}

pub fn claude_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case("Claude.exe") {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

pub fn query_claude_processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    for pid in claude_pids() {
        if let Some(cmdline) = command_line_of(pid) {
            out.push(ProcessInfo {
                pid,
                exe: String::new(),
                cmdline,
            });
        }
    }
    out
}

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationProcess(
        process_handle: HANDLE,
        process_information_class: u32,
        process_information: *mut c_void,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

/// Read-only command line of a same-user process. Used to tell which
/// `--user-data-dir` a running Claude window belongs to.
fn command_line_of(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
        if handle.is_null() {
            return None;
        }
        let mut needed: u32 = 0;
        let status = NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
        if status >= 0 || needed == 0 || needed > 64 * 1024 {
            CloseHandle(handle);
            return None;
        }
        let mut buffer = vec![0u8; needed as usize + 8];
        let status = NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as u32,
            &mut needed,
        );
        if status < 0 {
            CloseHandle(handle);
            return None;
        }
        let text = &*(buffer.as_ptr() as *const UnicodeString);
        let chars = text.length as usize / 2;
        if chars == 0 {
            CloseHandle(handle);
            return None;
        }
        let mut wide = vec![0u16; chars];
        let mut read = 0usize;
        let ok = ReadProcessMemory(
            handle,
            text.buffer as *const c_void,
            wide.as_mut_ptr() as *mut c_void,
            text.length as usize,
            &mut read,
        );
        CloseHandle(handle);
        if ok == 0 || read < text.length as usize {
            // Some builds return the string inline after the UNICODE_STRING header.
            let offset = std::mem::size_of::<UnicodeString>();
            let length = text.length as usize;
            if buffer.len() < offset + length {
                return None;
            }
            wide = buffer[offset..offset + length]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
        }
        let value = String::from_utf16_lossy(&wide);
        Some(value.trim_end_matches('\0').to_owned())
    }
}

/// Store package registration is per-user; the registry points at the versioned
/// WindowsApps folder without needing to enumerate a protected directory.
pub fn query_appx_install() -> Option<Install> {
    let root = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(APPX_PACKAGES, KEY_READ)
        .ok()?;
    let mut best: Option<Install> = None;
    for name in root.enum_keys().flatten() {
        if !name.starts_with("Claude_") {
            continue;
        }
        let Ok(key) = root.open_subkey_with_flags(&name, KEY_READ) else {
            continue;
        };
        let folder: String = key.get_value("PackageRootFolder").unwrap_or_default();
        if folder.trim().is_empty() {
            continue;
        }
        let exe = PathBuf::from(folder).join("app").join("Claude.exe");
        if !exe.is_file() {
            continue;
        }
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() < 5 {
            continue;
        }
        let version = parts[1].to_owned();
        let pfn = format!("{}_{}", parts[0], parts[parts.len() - 1]);
        let candidate = Install {
            kind: InstallKind::Msix,
            exe,
            version: version.clone(),
            aumid: Some(format!("{pfn}!Claude")),
            pfn: Some(pfn),
            default_profile: PathBuf::new(),
        };
        let better = best
            .as_ref()
            .map(|current| version_key(&version) > version_key(&current.version))
            .unwrap_or(true);
        if better {
            best = Some(candidate);
        }
    }
    best.map(|mut install| {
        install.refresh_identity();
        install
    })
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

// ------------------------------------------------------------- windows ----

pub fn focus_process_window(pid: u32) -> bool {
    struct Ctx {
        pid: u32,
        hwnd: HWND,
    }
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == ctx.pid && IsWindowVisible(hwnd) != 0 && GetWindowTextLengthW(hwnd) > 0 {
            ctx.hwnd = hwnd;
            return 0;
        }
        1
    }

    let mut ctx = Ctx {
        pid,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(callback), &mut ctx as *mut Ctx as LPARAM);
        if ctx.hwnd.is_null() {
            return false;
        }
        if IsIconic(ctx.hwnd) != 0 {
            ShowWindow(ctx.hwnd, SW_RESTORE);
        }
        SetForegroundWindow(ctx.hwnd) != 0
    }
}

fn find_window_by_title(title: &str) -> HWND {
    // `FindWindowW(null, title)` inexplicably returns NULL for winit's window on this
    // system, which used to make the tray icon unable to restore the window. Enumerate
    // top-level windows instead. The class filter keeps unrelated windows out; the lookup
    // must stay process-agnostic because a second launch uses it to reveal the first
    // instance.
    struct Ctx<'a> {
        title: &'a str,
        hwnd: HWND,
    }
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut class = [0u16; 64];
        let class_len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if class_len <= 0
            || String::from_utf16_lossy(&class[..class_len as usize]) != "Window Class"
        {
            return 1;
        }
        let mut text = [0u16; 128];
        let len = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        if len > 0 && String::from_utf16_lossy(&text[..len as usize]) == ctx.title {
            ctx.hwnd = hwnd;
            return 0;
        }
        1
    }

    let mut ctx = Ctx {
        title,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(callback), &mut ctx as *mut Ctx as LPARAM);
    }
    ctx.hwnd
}

fn to_wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn apply_window_style(dark: bool) {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    if hwnd.is_null() {
        return;
    }
    let preference: i32 = DWMWCP_ROUND as i32;
    let dark_flag: i32 = if dark { 1 } else { 0 };
    unsafe {
        // A decorations-less egui window is created as a bare popup: no taskbar entry and
        // no minimize support. Add them so the taskbar button can minimize/restore and
        // the window behaves like a normal app.
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let style = style as u32 | WS_MINIMIZEBOX | WS_SYSMENU;
        SetWindowLongW(hwnd, GWL_STYLE, style as i32);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let ex = ex as u32 | WS_EX_APPWINDOW;
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex as i32);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );

        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        // DWMWA_USE_IMMERSIVE_DARK_MODE
        DwmSetWindowAttribute(
            hwnd,
            20u32,
            &dark_flag as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// True while the window has been "closed" to the notification area. Detected from the
/// window style (`WS_EX_TOOLWINDOW`) rather than process state, so a second launch can
/// also see it and reopen the window.
static CLOSED_POS: Mutex<Option<(i32, i32)>> = Mutex::new(None);

pub fn is_tray_closed() -> bool {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    if hwnd.is_null() {
        return false;
    }
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 };
    ex & WS_EX_TOOLWINDOW != 0
}

/// Closes the UI to the notification area. The window is hidden for one instant so the
/// shell drops the taskbar button (`WS_EX_TOOLWINDOW`), then shown again far off-screen:
/// a truly hidden window makes eframe 0.33 busy-loop at 100 % of a core, while a visible
/// off-screen one keeps painting normally and costs almost nothing.
pub fn close_to_tray() -> bool {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) != 0 && IsIconic(hwnd) == 0 {
            if let Ok(mut pos) = CLOSED_POS.lock() {
                *pos = Some((rect.left, rect.top));
            }
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongW(
            hwnd,
            GWL_EXSTYLE,
            ((ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW) as i32,
        );
        ShowWindow(hwnd, SW_HIDE);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            -32000,
            -32000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    true
}

/// Undoes `close_to_tray`: restores the previous position, the taskbar button and focus.
pub fn reopen_from_tray() -> bool {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    if hwnd.is_null() {
        return false;
    }
    let pos = CLOSED_POS
        .lock()
        .ok()
        .and_then(|mut pos| pos.take())
        .unwrap_or((200, 120));
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongW(
            hwnd,
            GWL_EXSTYLE,
            ((ex | WS_EX_APPWINDOW) & !WS_EX_TOOLWINDOW) as i32,
        );
        // The shell only re-creates the taskbar button after a visibility change.
        ShowWindow(hwnd, SW_HIDE);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            pos.0,
            pos.1,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    true
}

/// Brings the switcher window back from the notification area. Called from the tray
/// callback on the UI thread, which may run when the window is hidden and eframe is not
/// painting — so this acts on the HWND directly instead of waiting for a viewport command.
pub fn reveal_main_window() -> bool {
    if is_tray_closed() {
        return reopen_from_tray();
    }
    let hwnd = find_window_by_title(WINDOW_TITLE);
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    true
}

pub fn is_main_window_visible() -> bool {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) != 0 }
}

pub fn is_main_window_minimized() -> bool {
    let hwnd = find_window_by_title(WINDOW_TITLE);
    !hwnd.is_null() && unsafe { IsIconic(hwnd) != 0 }
}

/// Windows' "app mode" preference (Settings → Personalisation → Colours).
pub fn system_uses_light_theme() -> bool {
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .ok();
    let Some(key) = key else { return true };
    let value: u32 = key.get_value("AppsUseLightTheme").unwrap_or(1);
    value != 0
}

/// True while a Windows IME (e.g. Microsoft Pinyin) is composing text in the foreground
/// window. egui only forwards IME events it receives, and a Chinese IME swallows Enter
/// inside its own composition window before delivering a key event — but it also does not
/// emit `Ime::Commit` when Enter merely converts the pinyin to raw latin text. Asking the
/// IME directly is the only reliable way to know whether Enter belongs to the input method.
pub fn ime_is_composing() -> bool {
    use windows_sys::Win32::UI::Input::Ime::{
        ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, GCS_COMPSTR,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        let imc = ImmGetContext(hwnd);
        if imc.is_null() {
            return false;
        }
        let length = ImmGetCompositionStringW(imc, GCS_COMPSTR, std::ptr::null_mut(), 0);
        ImmReleaseContext(hwnd, imc);
        length > 0
    }
}

/// Whether the active IME is in a mode where Enter belongs to the input method.
///
/// The composition buffer is empty by the time the confirming Enter reaches the app, so
/// querying it is too late. The conversion mode is stable across that keystroke: Chinese,
/// Japanese and Korean modes set `IME_CMODE_NATIVE`, and in those modes Enter converts
/// the pinyin/kana rather than confirming the surrounding dialog.
pub fn ime_owns_enter() -> bool {
    use windows_sys::Win32::UI::Input::Ime::{
        ImmGetContext, ImmGetConversionStatus, ImmReleaseContext, IME_CMODE_NATIVE,
        IME_CMODE_NATIVESYMBOL,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        let imc = ImmGetContext(hwnd);
        if imc.is_null() {
            return false;
        }
        let mut conversion: u32 = 0;
        let mut sentence: u32 = 0;
        let ok = ImmGetConversionStatus(imc, &mut conversion, &mut sentence);
        ImmReleaseContext(hwnd, imc);
        ok != 0
            && (conversion & (IME_CMODE_NATIVE | IME_CMODE_NATIVESYMBOL)) != 0
    }
}

pub fn animations_enabled() -> bool {
    let mut enabled: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut enabled as *mut i32 as *mut c_void,
            0,
        )
    };
    ok == 0 || enabled != 0
}

// -------------------------------------------------------------- launch ----

fn valid_aumid(aumid: &str) -> bool {
    // PackageFamilyName!AppId. Strict allow-list; the value normally comes from the
    // install cache, but settings.json is user-editable.
    let mut parts = aumid.split('!');
    let family = parts.next().unwrap_or_default();
    let app = parts.next().unwrap_or_default();
    parts.next().is_none()
        && !family.is_empty()
        && !app.is_empty()
        && aumid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '!'))
}

fn valid_profile_path(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    if name.starts_with('.') || name.contains(['\\', '/', '"']) {
        return false;
    }
    let local = paths().local;
    let profiles = paths().profiles;
    (crate::data::normalize_key(parent) == crate::data::normalize_key(&local)
        && name.starts_with(PROFILE_PREFIX))
        || (crate::data::normalize_key(parent) == crate::data::normalize_key(&profiles))
}

fn shell_execute(file: &OsStr, params: Option<&OsStr>, verb: Option<&str>, show: i32) -> bool {
    let file = to_wide(file);
    let params = params.map(to_wide);
    let verb = verb.map(|v| to_wide(OsStr::new(v)));
    unsafe {
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
            file.as_ptr(),
            params.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            std::ptr::null(),
            show,
        );
        result as isize > 32
    }
}

pub fn launch_default(install: &Install) -> Result<(), String> {
    if install.kind == InstallKind::Msix {
        if let Some(aumid) = install.aumid.as_deref().filter(|a| valid_aumid(a)) {
            // Activate through the shell so the window keeps Store package identity.
            let target = format!("shell:AppsFolder\\{aumid}");
            if shell_execute(OsStr::new(&target), None, None, SW_SHOWNORMAL) {
                return Ok(());
            }
        }
    }
    if !install.available() {
        return Err(format!(
            "找不到 Claude 桌面版。\n\n已记录的位置：{}",
            install.exe.display()
        ));
    }
    if shell_execute(install.exe.as_os_str(), None, Some("open"), SW_SHOWNORMAL) {
        Ok(())
    } else {
        Err("启动 Claude 失败。".into())
    }
}

pub fn launch_profile(install: &Install, path: &Path) -> Result<(), String> {
    if !valid_profile_path(path) {
        return Err(format!("账号目录不在允许的位置：{}", path.display()));
    }
    if !install.available() {
        return Err(format!(
            "找不到 Claude 桌面版。\n\n已记录的位置：{}",
            install.exe.display()
        ));
    }
    std::fs::create_dir_all(path)
        .map_err(|e| format!("无法创建账号目录 '{}'：{e}", path.display()))?;
    // A single quoted parameter, built from a validated local path. No shell syntax is
    // evaluated; ShellExecuteW receives the executable and its parameters separately.
    let mut parameters = OsString::from("--user-data-dir=\"");
    parameters.push(path.as_os_str());
    parameters.push("\"");
    if shell_execute(install.exe.as_os_str(), Some(&parameters), Some("open"), SW_SHOWNORMAL) {
        Ok(())
    } else {
        Err("启动 Claude 失败。".into())
    }
}

pub fn open_folder(path: &Path) -> io::Result<()> {
    if shell_execute(path.as_os_str(), None, Some("open"), SW_SHOWNORMAL) {
        Ok(())
    } else {
        Err(io::Error::other("无法打开资源管理器"))
    }
}

pub fn open_file(path: &Path) -> io::Result<()> {
    if shell_execute(path.as_os_str(), None, Some("open"), SW_SHOWNORMAL) {
        Ok(())
    } else {
        Err(io::Error::other("无法打开文件"))
    }
}

// ------------------------------------------------------- single instance ----

/// Returns true when this process is the first instance. A second launch brings the
/// existing window forward and returns false.
pub fn single_instance() -> bool {
    let name = to_wide(OsStr::new("Local\\ClaudeAccountSwitcher.Rust.3"));
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            reveal_main_window();
            return false;
        }
        // The handle intentionally lives for the whole process; closing it would release
        // the single-instance lock while the window is still open. Dropping a HANDLE value
        // does not close it, so it is simply left alone.
        let _ = handle;
        true
    }
}

// --------------------------------------------------------------- startup ----

pub fn startup_enabled(exe: &Path) -> bool {
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(STARTUP_KEY)
        .ok();
    let Some(key) = key else { return false };
    let value: String = key.get_value(STARTUP_VALUE).unwrap_or_default();
    let value = value.trim().trim_matches('"');
    value.eq_ignore_ascii_case(&exe.to_string_lossy())
}

pub fn set_startup(enable: bool, exe: &Path) -> io::Result<()> {
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(STARTUP_KEY)
        .map_err(|e| io::Error::other(e.to_string()))?
        .0;
    if enable {
        let value = format!("\"{}\"", exe.display());
        key.set_value(STARTUP_VALUE, &value)
            .map_err(|e| io::Error::other(e.to_string()))?;
    } else {
        match key.delete_value(STARTUP_VALUE) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io::Error::other(e.to_string())),
        }
    }
    Ok(())
}

/// Native "open image" picker. Returns a path the user chose, or None on cancel.
/// comdlg32 is used directly so the portable exe needs no extra runtime.
pub fn pick_image_file() -> Option<PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OPENFILENAMEW};

    let filter: Vec<u16> = "图片\0*.png;*.jpg;*.jpeg;*.webp\0所有文件\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "选择账号头像\0".encode_utf16().collect();
    let mut buffer = vec![0u16; 1024];

    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: std::ptr::null_mut(),
        hInstance: std::ptr::null_mut(),
        lpstrFilter: filter.as_ptr(),
        lpstrCustomFilter: std::ptr::null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 1,
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        lpstrFileTitle: std::ptr::null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: std::ptr::null(),
        lpstrTitle: title.as_ptr(),
        Flags: 0x0000_0008 | 0x0000_1000 | 0x0000_0800, // FILEMUSTEXIST | PATHMUSTEXIST | EXPLORER
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: std::ptr::null(),
        lCustData: 0,
        lpfnHook: None,
        lpTemplateName: std::ptr::null(),
        pvReserved: std::ptr::null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    };

    let ok = unsafe { GetOpenFileNameW(&mut dialog) };
    if ok == 0 {
        return None;
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len])))
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Claude Account Switcher.exe"))
}
