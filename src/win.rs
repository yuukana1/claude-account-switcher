//! Windows integration: process/window discovery, launching Claude, single instance,
//! DWM chrome and the opt-in startup entry. Kept in one place so the UI never touches
//! unsafe Win32 directly.
//!
//! Two notes on safety:
//! - Claude's command line is read with a read-only kernel query (`NtQueryInformationProcess`)
//!   for the same user's processes, never through a shell.
//! - Launches use package activation or `ShellExecuteExW` with validated paths and
//!   fixed verbs. User input is never evaluated as shell code.

use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, RECT,
};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::{CommandLineToArgvW, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    GWL_EXSTYLE, GWL_STYLE, SPI_GETCLIENTAREAANIMATION, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
    SW_SHOWNORMAL, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX, WS_SYSMENU,
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
            out.push(ProcessInfo { pid, cmdline });
        }
    }
    out
}

#[repr(C)]
#[derive(Clone, Copy)]
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
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
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
        CloseHandle(handle);
        decode_command_line(buffer.get(..needed as usize)?)
    }
}

fn decode_command_line(buffer: &[u8]) -> Option<String> {
    let header_size = std::mem::size_of::<UnicodeString>();
    if buffer.len() < header_size {
        return None;
    }
    // Vec<u8> does not guarantee UNICODE_STRING alignment. Copy the header without
    // creating an unaligned reference, then validate the kernel's inline pointer.
    let text = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<UnicodeString>()) };
    let length = usize::from(text.length);
    let offset = (text.buffer as usize).checked_sub(buffer.as_ptr() as usize)?;
    if length == 0 || length % 2 != 0 || text.length > text.maximum_length || offset < header_size {
        return None;
    }
    let bytes = buffer.get(offset..offset.checked_add(length)?)?;
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Some(
        String::from_utf16_lossy(&wide)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

fn command_line_args(command: &str) -> Option<Vec<String>> {
    if command.is_empty() || command.contains('\0') {
        return None;
    }
    let command = to_wide(OsStr::new(command));
    let mut count = 0;
    // Use Windows' quoting rules, including paths with spaces and the two forms
    // `--user-data-dir=path` and `--user-data-dir path`.
    let args = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
    if args.is_null() {
        return None;
    }
    let result = unsafe {
        let result = std::slice::from_raw_parts(args, count as usize)
            .iter()
            .map(|arg| {
                let mut length = 0;
                while *arg.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(*arg, length))
            })
            .collect();
        LocalFree(args.cast());
        result
    };
    Some(result)
}

pub fn process_profile_key(command: &str, install: &Install) -> Option<String> {
    let args = command_line_args(command)?;
    let executable = crate::data::normalize_key(Path::new(args.first()?));
    let desktop = executable == crate::data::normalize_key(&install.exe)
        || (executable.ends_with("\\claude.exe")
            && (executable.contains("\\windowsapps\\claude_")
                || executable.contains("\\anthropicclaude\\")
                || executable.contains("\\programs\\claude\\")));
    if !desktop
        || args
            .iter()
            .skip(1)
            .any(|arg| arg == "--type" || arg.starts_with("--type="))
    {
        return None;
    }
    let mut profile = None;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        let value = if arg == "--user-data-dir" {
            Some(args.next()?.as_str())
        } else {
            arg.strip_prefix("--user-data-dir=")
        };
        if let Some(value) = value {
            if value.is_empty() || !Path::new(value).is_absolute() {
                return None;
            }
            profile = Some(crate::data::normalize_key(Path::new(value)));
        }
    }
    Some(profile.unwrap_or_else(|| crate::data::normalize_key(&install.default_profile)))
}

struct ComApartment(bool);

impl ComApartment {
    fn enter() -> windows::core::Result<Self> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        result.ok()?;
        Ok(Self(true))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

// Use the current user's deployment registration, not leftover WindowsApps
// directories. Store updates can leave an old executable present but disabled.
fn registered_claude() -> windows::core::Result<Option<windows::ApplicationModel::Package>> {
    let manager = windows::Management::Deployment::PackageManager::new()?;
    let packages = manager.FindPackagesByUserSecurityId(&windows::core::HSTRING::new())?;
    let mut best = None;
    let mut best_version = (0, 0, 0, 0);
    for package in packages {
        let id = package.Id()?;
        if id.Name()? != "Claude" {
            continue;
        }
        let version = id.Version()?;
        let key = (
            version.Major,
            version.Minor,
            version.Build,
            version.Revision,
        );
        if best.is_none() || key > best_version {
            best_version = key;
            best = Some(package);
        }
    }
    Ok(best)
}

fn query_registered_install() -> windows::core::Result<Option<Install>> {
    let _apartment = ComApartment::enter()?;
    let Some(package) = registered_claude()? else {
        return Ok(None);
    };
    let id = package.Id()?;
    let version = id.Version()?;
    let pfn = id.FamilyName()?.to_string();
    let mut install = Install {
        kind: InstallKind::Msix,
        exe: PathBuf::from(package.InstalledLocation()?.Path()?.to_string())
            .join("app")
            .join("Claude.exe"),
        version: format!(
            "{}.{}.{}.{}",
            version.Major, version.Minor, version.Build, version.Revision
        ),
        aumid: Some(format!("{pfn}!Claude")),
        pfn: Some(pfn),
        default_profile: PathBuf::new(),
    };
    install.refresh_identity();
    Ok(Some(install))
}

/// Current registration takes precedence over readable but retired package paths.
pub fn query_appx_install() -> Option<Install> {
    if let Ok(Some(install)) = query_registered_install() {
        return Some(install);
    }
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
    let preference: i32 = DWMWCP_ROUND;
    let dark_flag: i32 = if dark { 1 } else { 0 };
    unsafe {
        // A decorations-less egui window is created as a bare popup: no taskbar entry and
        // no minimize support. Add them so the taskbar button can minimize/restore and
        // the window behaves like a normal app.
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let style = style as u32 | WS_MINIMIZEBOX | WS_SYSMENU;
        SetWindowLongW(hwnd, GWL_STYLE, style as i32);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let ex = ex as u32;
        let ex = if ex & WS_EX_TOOLWINDOW != 0 {
            ex & !WS_EX_APPWINDOW
        } else {
            ex | WS_EX_APPWINDOW
        };
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex as i32);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
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
    restore_main_window(true)
}

pub fn show_main_window_without_focus() -> bool {
    restore_main_window(false)
}

pub fn start_in_tray() -> bool {
    if !close_to_tray() {
        return false;
    }
    // Initial viewport creation is off-screen to avoid an on-screen flash.
    // Its first manual reveal must restore a normal screen position.
    if let Ok(mut pos) = CLOSED_POS.lock() {
        *pos = Some((120, 60));
    }
    true
}

fn restore_main_window(activate: bool) -> bool {
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
        ShowWindow(hwnd, if activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
        if activate {
            SetForegroundWindow(hwnd);
        }
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
        ok != 0 && (conversion & (IME_CMODE_NATIVE | IME_CMODE_NATIVESYMBOL)) != 0
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
            check_package_launchable(install)?;
            return activate_default(aumid);
        }
    }
    if !install.available() {
        return Err(format!(
            "找不到 Claude 桌面版。\n\n已记录的位置：{}",
            install.exe.display()
        ));
    }
    launch_executable(&install.exe, None)
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
    check_package_launchable(install)?;
    std::fs::create_dir_all(path)
        .map_err(|e| format!("无法创建账号目录 '{}'：{e}", path.display()))?;
    let mut parameters = OsString::from("--user-data-dir=\"");
    parameters.push(path.as_os_str());
    parameters.push("\"");
    launch_executable(&install.exe, Some(&parameters))
}

fn launch_executable(exe: &Path, parameters: Option<&OsStr>) -> Result<(), String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
        SHELLEXECUTEINFOW,
    };
    let _apartment = ComApartment::enter().map_err(|error| error.to_string())?;
    let file = to_wide(exe.as_os_str());
    let args = parameters.map(to_wide);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // Preserve the explicit normal show state while returning the real error
    // instead of letting the shell display a second generic error dialog.
    let result = unsafe { ShellExecuteExW(&mut info) };
    if !info.hProcess.is_invalid() {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(info.hProcess) };
    }
    result.map_err(|error| format!("启动 Claude 失败：{error}\n\n程序位置：{}", exe.display()))
}

fn activate_default(aumid: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER};
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, AO_NONE,
    };
    let activate = || -> windows::core::Result<()> {
        let _apartment = ComApartment::enter()?;
        let manager: IApplicationActivationManager =
            unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)? };
        let target = to_wide(OsStr::new(aumid));
        unsafe { manager.ActivateApplication(PCWSTR(target.as_ptr()), PCWSTR::null(), AO_NONE)? };
        Ok(())
    };
    activate().map_err(|error| {
        format!("Windows 未能启动 Claude：{error}\n\n请确认 Claude 已完成更新并能从开始菜单打开。")
    })
}

pub fn check_package_launchable(install: &Install) -> Result<(), String> {
    if install.kind != InstallKind::Msix {
        return Ok(());
    }
    let inspect = || -> windows::core::Result<bool> {
        let _apartment = ComApartment::enter()?;
        let Some(package) = registered_claude()? else {
            return Ok(true);
        };
        let status = package.Status()?;
        status.VerifyIsOK()
    };
    match inspect() {
        Ok(false) => Err("Windows 报告 Claude 的安装状态异常，更新可能尚未完成。请先完成 Claude 更新，再重试。\n\n如果 Windows 提示服务“已标记为删除”（0x80070430），请保存工作后重启 Windows，然后完成更新。无需删除或重新添加账号。".into()),
        // A failed status query must not block a healthy classic/Store launch.
        _ => Ok(()),
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
pub fn single_instance(reveal: bool) -> bool {
    let name = to_wide(OsStr::new("Local\\ClaudeAccountSwitcher.Rust.3"));
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if reveal {
                reveal_main_window();
            }
            CloseHandle(handle);
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
    startup_matches(&value, exe)
}

fn startup_matches(value: &str, exe: &Path) -> bool {
    // Accept the previous quoted/unquoted command so enabled installations
    // migrate to background startup without changing the user's enabled state.
    if value.trim().is_empty() {
        return false;
    }
    if value.trim().eq_ignore_ascii_case(&exe.to_string_lossy()) {
        return true;
    }
    let Some(args) = command_line_args(value.trim()) else {
        return false;
    };
    let valid_args = args.len() == 1 || (args.len() == 2 && args[1] == "--background");
    valid_args && crate::data::normalize_key(Path::new(&args[0])) == crate::data::normalize_key(exe)
}

pub fn set_startup(enable: bool, exe: &Path) -> io::Result<()> {
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(STARTUP_KEY)
        .map_err(|e| io::Error::other(e.to_string()))?
        .0;
    if enable {
        let value = format!("\"{}\" --background", exe.display());
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

    let filter: Vec<u16> = "图片\0*.png;*.jpg;*.jpeg\0所有文件\0*.*\0\0"
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

#[cfg(test)]
mod tests {
    #[test]
    fn startup_recognizes_migration_and_rejects_unrelated_commands() {
        let exe = Path::new(r"C:\Portable Apps\Claude Account Switcher.exe");
        for command in [
            r"C:\Portable Apps\Claude Account Switcher.exe",
            r#""C:\Portable Apps\Claude Account Switcher.exe""#,
            r#""C:\Portable Apps\Claude Account Switcher.exe" --background"#,
            r#"  "c:\portable apps\CLAUDE ACCOUNT SWITCHER.EXE" --background  "#,
        ] {
            assert!(startup_matches(command, exe), "{command}");
        }
        for command in [
            "",
            " ",
            r#""C:\Other.exe" --background"#,
            r#""C:\Portable Apps\Claude Account Switcher.exe" --dump"#,
            r#""C:\Portable Apps\Claude Account Switcher.exe" --background --dump"#,
        ] {
            assert!(!startup_matches(command, exe), "{command}");
        }
    }
    use super::*;

    fn install() -> Install {
        Install {
            kind: InstallKind::Classic,
            exe: PathBuf::from(r"C:\Custom Claude\Claude.exe"),
            version: "test".into(),
            aumid: None,
            pfn: None,
            default_profile: PathBuf::from(r"C:\Users\Tester\AppData\Roaming\Claude"),
        }
    }

    #[test]
    fn reads_current_process_command_line_from_native_query() {
        let command = command_line_of(std::process::id()).expect("query own command line");
        let executable = std::env::current_exe().unwrap();
        assert!(command.contains(executable.file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn command_line_header_and_inline_pointer_are_bounds_checked() {
        let header_size = std::mem::size_of::<UnicodeString>();
        for length in 0..header_size {
            assert!(decode_command_line(&vec![0; length]).is_none());
        }
        let wide: Vec<_> = "Claude 中文".encode_utf16().collect();
        // Deliberately misalign the record to exercise the unaligned header read.
        let mut bytes = vec![0u8; 1 + header_size + wide.len() * 2];
        let buffer = &mut bytes[1..];
        for (dest, source) in buffer[header_size..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&wide)
        {
            dest.copy_from_slice(&source.to_le_bytes());
        }
        let mut header = UnicodeString {
            length: (wide.len() * 2) as u16,
            maximum_length: (wide.len() * 2) as u16,
            buffer: unsafe { buffer.as_mut_ptr().add(header_size).cast() },
        };
        unsafe { std::ptr::write_unaligned(buffer.as_mut_ptr().cast(), header) };
        assert_eq!(decode_command_line(buffer).as_deref(), Some("Claude 中文"));
        header.buffer = std::ptr::null_mut();
        unsafe { std::ptr::write_unaligned(buffer.as_mut_ptr().cast(), header) };
        assert!(decode_command_line(buffer).is_none());
        header.buffer = unsafe { buffer.as_mut_ptr().add(buffer.len()).cast() };
        unsafe { std::ptr::write_unaligned(buffer.as_mut_ptr().cast(), header) };
        assert!(decode_command_line(buffer).is_none());
        header.length = 3;
        header.buffer = unsafe { buffer.as_mut_ptr().add(header_size).cast() };
        unsafe { std::ptr::write_unaligned(buffer.as_mut_ptr().cast(), header) };
        assert!(decode_command_line(buffer).is_none());
    }

    #[test]
    fn profile_arguments_follow_windows_quoting_rules() {
        let install = install();
        let expected = r"c:\users\tester\profile with spaces";
        for arguments in [
            r#"--user-data-dir="C:\Users\Tester\Profile With Spaces""#,
            r#""--user-data-dir=C:\Users\Tester\Profile With Spaces""#,
            r#"--user-data-dir "C:\Users\Tester\Profile With Spaces""#,
        ] {
            let command = format!("\"{}\" {arguments}", install.exe.display());
            assert_eq!(
                process_profile_key(&command, &install).as_deref(),
                Some(expected),
                "{command}"
            );
        }
    }

    #[test]
    fn child_processes_and_unrelated_cli_executables_are_not_profiles() {
        let install = install();
        for command in [
            r#""C:\Custom Claude\Claude.exe" --type=renderer"#,
            r#""C:\Custom Claude\Claude.exe" --type utility"#,
            r#""C:\bin\claude.exe" --note="C:\Programs\Claude\""#,
            r#""C:\Custom Claude\Claude.exe" --user-data-dir"#,
            r#""C:\Custom Claude\Claude.exe" --user-data-dir=relative"#,
        ] {
            assert!(
                process_profile_key(command, &install).is_none(),
                "{command}"
            );
        }
        let command = r#""C:\Custom Claude\Claude.exe" --note="--type=renderer""#;
        assert_eq!(
            process_profile_key(command, &install),
            Some(crate::data::normalize_key(&install.default_profile))
        );
    }

    #[test]
    fn aumid_rejects_shell_syntax_and_missing_components() {
        assert!(valid_aumid("Claude_pzs8sxrjxfjjc!Claude"));
        for value in [
            "",
            "!Claude",
            "Claude_123!",
            "Claude!app!other",
            "Claude!app & other",
            "Claude!app\0other",
        ] {
            assert!(!valid_aumid(value), "{value:?}");
        }
    }
}
#[test]
fn failed_executable_launch_returns_native_error() {
    let dir = crate::test_support::TestDir::new();
    let missing = dir.path().join("missing.exe");
    let error = launch_executable(&missing, None).unwrap_err();
    assert!(error.contains("启动 Claude 失败"));
    assert!(error.contains(&missing.display().to_string()));
    assert!(!missing.exists());
}

#[test]
fn unregistered_default_app_does_not_report_a_successful_launch() {
    let error = activate_default("CodexSwitcherMissing_0000000000000!Missing").unwrap_err();
    assert!(error.contains("Windows 未能启动 Claude"));
}
