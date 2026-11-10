//! Give each secondary Claude profile its own taskbar group. Claude assigns all
//! windows the same AppUserModelID, so a different user-data-dir alone is not enough.

use std::path::Path;

use sha2::{Digest, Sha256};
use windows::core::{Result, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::{HWND, PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{ITaskbarList, SHStrDupW, TaskbarList};
use windows_sys::Win32::Foundation::LPARAM;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowThreadProcessId,
    IsWindowVisible, SendMessageTimeoutW, GWL_EXSTYLE, GW_OWNER, SMTO_ABORTIFHUNG, WM_GETICON,
    WM_SETICON, WS_EX_TOOLWINDOW,
};

use crate::data::{normalize_key, Account, Install};

const APP_MODEL: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
const ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_MODEL,
    pid: 5,
};
const RELAUNCH_COMMAND: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_MODEL,
    pid: 2,
};
const RELAUNCH_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_MODEL,
    pid: 3,
};
const RELAUNCH_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_MODEL,
    pid: 4,
};

struct Apartment(bool);

impl Apartment {
    fn enter() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            // The GUI runtime already initialized this thread in another apartment.
            return Ok(Self(false));
        }
        result.ok()?;
        Ok(Self(true))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub fn profile_group_id(path: &Path) -> String {
    // Persist across renames and restarts, distinguish punctuation/Unicode, and stay
    // below Windows' 128-character limit even for very long profile paths.
    let hash = Sha256::digest(normalize_key(path).as_bytes());
    format!("ClaudeAccountSwitcher.Profile.{hash:x}")
}

fn profile_windows(pid: u32) -> Vec<HWND> {
    struct Query {
        pid: u32,
        windows: Vec<HWND>,
    }
    unsafe extern "system" fn visit(
        hwnd: windows_sys::Win32::Foundation::HWND,
        param: LPARAM,
    ) -> i32 {
        let query = &mut *(param as *mut Query);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != query.pid
            || !GetWindow(hwnd, GW_OWNER).is_null()
            || GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0
        {
            return 1;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "Chrome_WidgetWin_1" {
            query.windows.push(HWND(hwnd));
        }
        1
    }
    let mut query = Query {
        pid,
        windows: Vec::new(),
    };
    unsafe { EnumWindows(Some(visit), &mut query as *mut Query as LPARAM) };
    query.windows
}

fn string_property(store: &IPropertyStore, key: &PROPERTYKEY) -> Result<String> {
    let value = unsafe { store.GetValue(key)? };
    if value.is_empty() {
        return Ok(String::new());
    }
    Ok(BSTR::try_from(&value)?.to_string())
}

fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, text: &str) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // AppUserModel properties require VT_LPWSTR, not VT_BSTR. PROPVARIANT owns the
    // CoTaskMem-allocated copy and frees it on every success/error path.
    let copy = unsafe { SHStrDupW(PCWSTR(wide.as_ptr()))? };
    let value = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                Anonymous: PROPVARIANT_0_0_0 { pwszVal: copy },
                ..Default::default()
            }),
        },
    };
    unsafe { store.SetValue(key, &value) }
}

fn relaunch_command(install: &Install, account: &Account) -> String {
    format!(
        "\"{}\" --user-data-dir=\"{}\"",
        install.exe.display(),
        account.path.display()
    )
}

pub fn sync_profiles(
    install: &Install,
    accounts: &[Account],
    icons: &mut crate::taskbar_icon::IconCache,
) -> std::result::Result<(), String> {
    sync_profiles_inner(install, accounts, icons).map_err(|error| error.to_string())
}

fn sync_profiles_inner(
    install: &Install,
    accounts: &[Account],
    icons: &mut crate::taskbar_icon::IconCache,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let _apartment = Apartment::enter()?;
    let mut taskbar: Option<ITaskbarList> = None;
    for account in accounts
        .iter()
        .filter(|a| !a.is_default && a.running_pid.is_some())
    {
        let legacy = crate::identity::legacy_avatar_stem(&account.id).to_lowercase();
        let allow_legacy = accounts
            .iter()
            .filter(|a| crate::identity::legacy_avatar_stem(&a.id).to_lowercase() == legacy)
            .count()
            == 1;
        let avatar = crate::identity::custom_avatar_path(&account.id, allow_legacy);
        let icon = icons.prepare(account, avatar.as_deref())?;
        // Explorer caches group artwork by identity. Include the artwork revision
        // so changing the avatar refreshes the taskbar instead of leaving a stale icon.
        let identity = format!("{}.{}", profile_group_id(&account.path), &icon.digest[..16]);
        let handle = icons.handle(&icon)?;
        for hwnd in profile_windows(account.running_pid.unwrap()) {
            let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
            let previous = string_property(&store, &ID)?;
            let name = format!("Claude - {}", account.name);
            let command = relaunch_command(install, account);
            for (key, text) in [
                (&RELAUNCH_NAME, name.as_str()),
                (&RELAUNCH_COMMAND, command.as_str()),
                (&RELAUNCH_ICON, &format!("{},0", icon.path.display())),
                (&ID, identity.as_str()),
            ] {
                if string_property(&store, key)? != text {
                    set_string(&store, key, text)?;
                }
            }
            unsafe { store.Commit()? };
            update_window_icon(hwnd, handle)?;
            if previous != identity && unsafe { IsWindowVisible(hwnd.0) } != 0 {
                if taskbar.is_none() {
                    let list: ITaskbarList =
                        unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
                    unsafe { list.HrInit()? };
                    taskbar = Some(list);
                }
                // Re-register existing visible windows once. Never add a hidden
                // window: that would leave a phantom taskbar button.
                let list = taskbar.as_ref().unwrap();
                unsafe {
                    let removed = list.DeleteTab(hwnd);
                    let added = list.AddTab(hwnd);
                    if removed.is_err() || added.is_err() {
                        // Restore the previous identity so the next poll also retries
                        // shell registration, rather than treating this as applied.
                        let _ = set_string(&store, &ID, &previous);
                        let _ = store.Commit();
                    }
                    removed?;
                    added?;
                }
            }
        }
    }
    Ok(())
}

fn update_window_icon(hwnd: HWND, icon: usize) -> std::io::Result<()> {
    for size in [0usize, 1] {
        let mut current = 0;
        let read = unsafe {
            SendMessageTimeoutW(
                hwnd.0,
                WM_GETICON,
                size,
                0,
                SMTO_ABORTIFHUNG,
                100,
                &mut current,
            )
        };
        if read == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if current != icon {
            let written = unsafe {
                SendMessageTimeoutW(
                    hwnd.0,
                    WM_SETICON,
                    size,
                    icon as isize,
                    SMTO_ABORTIFHUNG,
                    100,
                    std::ptr::null_mut(),
                )
            };
            if written == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// Read-only diagnostics; also useful when the shell resets a window's properties.
#[derive(Debug)]
pub struct WindowIdentity {
    pub identity: String,
    pub icon_resource: String,
    pub has_window_icon: bool,
}

pub fn window_identities(pid: u32) -> Result<Vec<WindowIdentity>> {
    let _apartment = Apartment::enter()?;
    profile_windows(pid)
        .into_iter()
        .map(|hwnd| {
            let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
            let mut icon = 0;
            let read = unsafe {
                SendMessageTimeoutW(hwnd.0, WM_GETICON, 1, 0, SMTO_ABORTIFHUNG, 100, &mut icon)
            };
            Ok(WindowIdentity {
                identity: string_property(&store, &ID)?,
                icon_resource: string_property(&store, &RELAUNCH_ICON)?,
                has_window_icon: read != 0 && icon != 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_stable_case_insensitive_bounded_and_distinct() {
        assert_eq!(
            profile_group_id(Path::new(r"C:\Users\测试\Profile A")),
            profile_group_id(Path::new("c:/users/测试/profile a/"))
        );
        let ids: std::collections::HashSet<_> = ["a b", "a_b", "a-b", "测试", "測試"]
            .map(|name| profile_group_id(&Path::new(r"C:\Profiles").join(name)))
            .into_iter()
            .collect();
        assert_eq!(ids.len(), 5);
        let long = profile_group_id(&Path::new(r"C:\Profiles").join("a".repeat(500)));
        assert!(long.len() < 128);
        assert!(long.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
    }

    #[test]
    fn relaunch_preserves_unicode_and_spaces_in_windows_arguments() {
        let install = Install {
            kind: crate::data::InstallKind::Classic,
            exe: Path::new(r"C:\Claude App\Claude.exe").into(),
            version: String::new(),
            aumid: None,
            pfn: None,
            default_profile: Path::new(r"C:\Default").into(),
        };
        let account = Account {
            id: "test".into(),
            name: "test".into(),
            path: Path::new(r"C:\Users\测试\ClaudeProfile-副号 1").into(),
            is_default: false,
            color: 0,
            running_pid: None,
            last_used: None,
            session_count: 0,
            data_path: None,
        };
        assert_eq!(
            crate::win::process_profile_key(&relaunch_command(&install, &account), &install),
            Some(normalize_key(&account.path))
        );
    }

    #[test]
    fn hidden_window_gets_profile_identity_and_default_window_is_untouched() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW,
        };

        let _apartment = Apartment::enter().unwrap();
        let class: Vec<u16> = "Chrome_WidgetWin_1".encode_utf16().chain(Some(0)).collect();
        let definition = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        // A real hidden Win32 window owned by this test process; no user Claude
        // windows, profiles or shell startup entries are touched by this test.
        assert_ne!(unsafe { RegisterClassW(&definition) }, 0);
        let raw = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!raw.is_null());
        struct TestWindow(windows_sys::Win32::Foundation::HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }
        let _window = TestWindow(raw);
        let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(HWND(raw)).unwrap() };
        set_string(&store, &ID, "Claude.Test.Default").unwrap();
        unsafe { store.Commit().unwrap() };
        let install = Install {
            kind: crate::data::InstallKind::Classic,
            exe: Path::new(r"C:\Claude App\Claude.exe").into(),
            version: String::new(),
            aumid: None,
            pfn: None,
            default_profile: Path::new(r"C:\Default").into(),
        };
        let mut account = Account {
            id: "secondary".into(),
            name: "副号测试".into(),
            path: Path::new(r"C:\Users\测试\ClaudeProfile-副号 1").into(),
            is_default: true,
            color: 0,
            running_pid: Some(std::process::id()),
            last_used: None,
            session_count: 0,
            data_path: None,
        };
        let dir = crate::test_support::TestDir::new();
        let mut icons = crate::taskbar_icon::IconCache::new(dir.path().join("icons"));
        sync_profiles(&install, &[account.clone()], &mut icons).unwrap();
        assert_eq!(string_property(&store, &ID).unwrap(), "Claude.Test.Default");
        account.is_default = false;
        sync_profiles(&install, &[account.clone()], &mut icons).unwrap();
        let identity = string_property(&store, &ID).unwrap();
        assert!(identity.starts_with(&profile_group_id(&account.path)));
        assert!(identity.len() < 128);
        let icon_path = string_property(&store, &RELAUNCH_ICON).unwrap();
        assert!(Path::new(icon_path.strip_suffix(",0").unwrap()).is_file());
        assert_eq!(
            string_property(&store, &RELAUNCH_COMMAND).unwrap(),
            relaunch_command(&install, &account)
        );
        account.name = "副号重命名".into();
        sync_profiles(&install, &[account], &mut icons).unwrap();
        assert_eq!(string_property(&store, &ID).unwrap(), identity);
        assert_eq!(
            string_property(&store, &RELAUNCH_NAME).unwrap(),
            "Claude - 副号重命名"
        );
        assert_eq!(unsafe { IsWindowVisible(raw) }, 0);
    }
}
