use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui;
use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use super::SwitcherApp;
use crate::theme;

/// tray-icon and muda deliver events either to their channel or to a global handler —
/// setting a handler silences the channel. The handler is what wakes a hidden window, so
/// it forwards the payload here and the UI drains these queues instead of the channels.
static TRAY_QUEUE: OnceLock<Mutex<Vec<TrayIconEvent>>> = OnceLock::new();
static MENU_QUEUE: OnceLock<Mutex<Vec<muda::MenuEvent>>> = OnceLock::new();

fn tray_queue() -> &'static Mutex<Vec<TrayIconEvent>> {
    TRAY_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

fn menu_queue() -> &'static Mutex<Vec<muda::MenuEvent>> {
    MENU_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Wakes the event loop for tray events and keeps the payload for `poll_tray`.
pub(super) fn install_tray_handlers(ctx: &egui::Context) {
    let ctx_tray = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        // A left click must restore the window even if eframe is not painting while the
        // window is hidden, so act on the HWND here rather than waiting for the UI loop.
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        ) {
            crate::win::reveal_main_window();
        }
        if let Ok(mut queue) = tray_queue().lock() {
            queue.push(event);
        }
        ctx_tray.request_repaint();
    }));
    let ctx_menu = ctx.clone();
    tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
        let id = event.id.0.clone();
        // 退出 is the only way to stop the program, and it must never wait for a UI
        // frame: a minimized window may not paint for a while.
        if id == "quit" {
            std::process::exit(0);
        }
        if id == "show" || id == "add" || id == "updates" {
            crate::win::reveal_main_window();
        }
        if let Ok(mut queue) = menu_queue().lock() {
            queue.push(event);
        }
        ctx_menu.request_repaint();
    }));
}

impl SwitcherApp {
    /// True when the notification-area icon exists, so "close" can hide safely.
    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    fn tray_signature(&self) -> String {
        let mut signature = String::new();
        for account in &self.accounts {
            signature.push(if self.settings.pinned(&account.id) {
                '*'
            } else {
                ' '
            });
            signature.push_str(&account.id);
            signature.push('|');
            signature.push_str(&account.name);
            signature.push('|');
            signature.push(if account.running_pid.is_some() {
                '1'
            } else {
                '0'
            });
            signature.push(';');
        }
        signature.push_str(if self.startup { "boot1" } else { "boot0" });
        signature.push_str(self.newer_version().unwrap_or(""));
        signature
    }

    fn tray_menu(&self) -> Menu {
        let menu = Menu::new();
        for account in &self.accounts {
            let text = if account.running_pid.is_some() {
                format!("{}   ·   运行中", account.name)
            } else {
                account.name.clone()
            };
            let text = text.replace('&', "&&");
            let text = if self.settings.pinned(&account.id) {
                format!("★  {text}")
            } else {
                text
            };
            let item = MenuItem::with_id(format!("acct:{}", account.id), text, true, None);
            let _ = menu.append(&item);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("add", "添加账号…", true, None));
        let _ = menu.append(&MenuItem::with_id("show", "打开切换器", true, None));
        let updates = self
            .newer_version()
            .map(|v| format!("有新版本 v{v}…"))
            .unwrap_or_else(|| "检查更新…".into());
        let _ = menu.append(&MenuItem::with_id("updates", updates, true, None));
        let _ = menu.append(&CheckMenuItem::with_id(
            "startup",
            "开机静默启动",
            true,
            self.startup,
            None,
        ));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("quit", "退出", true, None));
        menu
    }

    pub(super) fn rebuild_tray(&mut self) {
        let signature = self.tray_signature();
        if self.tray.is_some() && signature == self.tray_signature {
            return;
        }
        if self.tray.is_none() && Instant::now() < self.next_tray_try {
            return;
        }

        let menu = self.tray_menu();
        let tooltip = format!(
            "Claude 账号切换器 · {} 个账号 · {} 个运行中",
            self.accounts.len(),
            self.running_count()
        );

        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            let _ = tray.set_tooltip(Some(tooltip));
            self.tray_signature = signature;
            return;
        }

        let Ok(icon) = Icon::from_rgba(theme::icon_rgba(32), 32, 32) else {
            self.next_tray_try = Instant::now() + Duration::from_secs(5);
            return;
        };
        match TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip(tooltip)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
        {
            Ok(tray) => {
                self.tray = Some(tray);
                self.tray_signature = signature;
            }
            Err(_) => {
                self.next_tray_try = Instant::now() + Duration::from_secs(5);
            }
        }
    }

    pub(super) fn poll_tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_none() {
            self.rebuild_tray();
        }

        let events: Vec<TrayIconEvent> = tray_queue()
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for event in events {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                self.show_window(ctx);
            }
        }

        let menu_events: Vec<muda::MenuEvent> = menu_queue()
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for event in menu_events {
            let id = event.id.0;
            if let Some(account_id) = id.strip_prefix("acct:") {
                self.launch(account_id, ctx);
            } else if id == "add" {
                self.request_add_account();
            } else if id == "show" {
                self.show_window(ctx);
            } else if id == "updates" {
                self.open_updates(ctx);
            } else if id == "startup" {
                self.toggle_startup(ctx);
            } else if id == "quit" {
                self.quit(ctx);
            }
        }
    }
}
