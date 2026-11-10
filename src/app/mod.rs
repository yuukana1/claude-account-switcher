//! Application state and behaviour. Rendering lives in `draw`, modal menus and dialogs
//! in `dialogs`, and the notification-area icon in `tray`.

mod dialogs;
mod draw;
mod tray;

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::Local;
use eframe::egui;
use serde_json::{json, Value};

use crate::anim::Blend;
use crate::data::{self, paths, Account, Install, Session, Usage, DEFAULT_ID};
use crate::identity::{self, read_avatar_image, Identity};
use crate::settings::Settings;
use crate::theme;
use crate::win::{self, ProcessInfo};

pub const WINDOW_W: f32 = 1000.0;
pub const WINDOW_H: f32 = 624.0;
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    pub text: String,
    pub tone: Tone,
    pub at: Instant,
}

pub struct Transfer {
    pub source_id: String,
    pub target_id: String,
    pub sessions: Vec<Session>,
    pub checked: Vec<bool>,
    pub hover: Option<usize>,
}

pub struct PendingCopy {
    pub target_id: String,
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug)]
pub enum DialogAction {
    None,
    Launch(String),
    PerformCopy,
}

pub enum Dialog {
    None,
    Rename {
        id: String,
        buf: String,
        focus: bool,
        error: Option<String>,
    },
    Delete {
        id: String,
    },
    /// Confirmation before creating and launching a brand-new profile.
    AddAccount {
        name: String,
    },
    Message {
        title: String,
        body: String,
        tone: Tone,
        primary: String,
        secondary: Option<String>,
        action: DialogAction,
    },
    Transfer(Box<Transfer>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    Row,
    More,
}

pub struct OpenMenu {
    pub kind: MenuKind,
    pub account_id: String,
    pub pos: egui::Pos2,
    pub at: Instant,
}

#[derive(Clone)]
pub enum UiAction {
    Launch(String),
    Select(String),
    AddAccount,
    Rename(String),
    Delete(String),
    OpenFolder(String),
    Transfer(String),
    SetAvatar(String),
    ClearAvatar(String),
    ToggleStartup,
    ToggleTheme,
    OpenSettings,
    About,
}

pub struct SwitcherApp {
    pub settings: Settings,
    pub install: Install,
    pub accounts: Vec<Account>,
    pub selected_id: String,
    pub usage: Option<Usage>,
    pub bar_fh: Blend,
    pub bar_sd: Blend,
    bar_owner: String,
    pub pal: theme::Palette,
    pub logo_large: egui::TextureHandle,
    pub logo_small: egui::TextureHandle,
    pub mark: egui::TextureHandle,
    pub identity: HashMap<String, Option<Identity>>,
    pub recent_sessions: Vec<Session>,
    pub avatar_textures: HashMap<String, egui::TextureHandle>,

    pub dialog: Dialog,
    pub toast: Option<Toast>,
    pub menu: Option<OpenMenu>,
    pub pending_copy: Option<PendingCopy>,

    pub tray: Option<tray_icon::TrayIcon>,
    tray_signature: String,
    next_tray_try: Instant,

    /// Whether a composition is open right now, and whether it was open on the previous
    /// frame. A Chinese IME clears its buffer in the same frame the confirming Enter is
    /// delivered, so the previous frame's sample is the only reliable evidence that this
    /// Enter belonged to the input method.
    pub ime_composing: bool,
    pub ime_composing_prev: bool,
    /// When the IME last committed text. Windows re-delivers that Enter right after, so a
    /// recent commit also means the keystroke belonged to the input method.
    pub ime_commit_at: Option<Instant>,

    pub proc_map: HashMap<String, u32>,
    known_pids: Vec<u32>,
    tx: Sender<Vec<ProcessInfo>>,
    rx: Receiver<Vec<ProcessInfo>>,
    querying: bool,
    pending_query_at: Option<Instant>,
    next_poll: Instant,
    next_taskbar: Instant,
    taskbar_icons: crate::taskbar_icon::IconCache,
    next_full_query: Instant,

    next_meta: Instant,
    next_identity: Instant,
    next_watch: Instant,
    started: Instant,
    pub time: f32,
    /// Repaint at full rate until this instant, e.g. while a hover fade or bar is moving.
    pub animate_until: Instant,
    pub motion: bool,
    pub startup: bool,
    pub quit: bool,
    styled: bool,
    pub tray_hint_pending: bool,
    settings_error: Option<String>,
}

impl SwitcherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);

        let paths = paths();
        let mut settings = Settings::load(paths.settings.clone());
        let mode = match settings.theme_pref().as_str() {
            "light" => theme::Mode::Light,
            "dark" => theme::Mode::Dark,
            _ => {
                if win::system_uses_light_theme() {
                    theme::Mode::Light
                } else {
                    theme::Mode::Dark
                }
            }
        };
        let pal = theme::Palette::from_mode(mode);
        theme::install_style(&cc.egui_ctx, &pal);

        let install = data::discover_install(settings.install_cache());
        if install.available() {
            settings.set_install_cache(&install);
        }
        let mut accounts = data::discover_accounts(&install, &mut settings);
        for account in &mut accounts {
            data::refresh_metadata(&install, account);
        }
        let settings_error = settings.save().err().map(|error| error.to_string());

        let selected_id = settings
            .last_account()
            .filter(|id| accounts.iter().any(|account| &account.id == id))
            .unwrap_or_else(|| {
                accounts
                    .first()
                    .map(|a| a.id.clone())
                    .unwrap_or_else(|| DEFAULT_ID.to_owned())
            });
        let startup = win::startup_enabled(&win::current_exe());
        if startup {
            // Keep the Run entry pointing at the current portable exe location.
            let _ = win::set_startup(true, &win::current_exe());
        }

        let (tx, rx) = channel();
        let now = Instant::now();

        let texture = |size: u32, name: &str, rgba: Vec<u8>| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba);
            cc.egui_ctx
                .load_texture(name.to_owned(), image, egui::TextureOptions::LINEAR)
        };
        let logo_large = texture(128, "logo-large", theme::icon_rgba(128));
        let logo_small = texture(48, "logo-small", theme::icon_rgba(48));
        let mark = texture(128, "starburst-mark", theme::starburst_rgba(128));

        let mut app = Self {
            settings,
            install,
            accounts,
            selected_id,
            usage: None,
            bar_fh: Blend::new(0.0, 0.55),
            bar_sd: Blend::new(0.0, 0.55),
            bar_owner: String::new(),
            pal,
            logo_large,
            logo_small,
            mark,
            identity: HashMap::new(),
            recent_sessions: Vec::new(),
            avatar_textures: HashMap::new(),
            dialog: Dialog::None,
            toast: None,
            menu: None,
            pending_copy: None,
            tray: None,
            tray_signature: String::new(),
            next_tray_try: now,
            ime_composing: false,
            ime_composing_prev: false,
            ime_commit_at: None,
            proc_map: HashMap::new(),
            known_pids: Vec::new(),
            tx,
            rx,
            querying: false,
            pending_query_at: None,
            next_poll: now,
            next_taskbar: now,
            taskbar_icons: crate::taskbar_icon::IconCache::new(
                paths.switcher.join("taskbar-icons"),
            ),
            next_full_query: now,
            next_meta: now,
            next_identity: now + Duration::from_secs(30),
            next_watch: now + Duration::from_secs(60),
            started: now,
            time: 0.0,
            animate_until: now,
            motion: win::animations_enabled(),
            startup,
            quit: false,
            styled: false,
            tray_hint_pending: false,
            settings_error,
        };

        app.refresh_usage(false);
        app.load_identities_in(Some(&cc.egui_ctx));
        app.spawn_query(&cc.egui_ctx);

        // Establish a baseline without announcing old history, exactly like the old
        // switcher: the first run only records what is already there.
        let notes = app.run_quota_watch();
        if !notes.is_empty() {
            app.toast(notes.join("；"), Tone::Info);
        }

        // Tray handlers both wake the hidden window and queue the payload for the UI
        // (the library's own channels go silent once a handler is installed).
        tray::install_tray_handlers(&cc.egui_ctx);

        app.rebuild_tray();
        let startup_dir = paths
            .roaming
            .join("Microsoft/Windows/Start Menu/Programs/Startup");
        match crate::legacy::migrate_disabled_startup(
            &startup_dir,
            &paths.switcher.join("startup-backups"),
        ) {
            Ok(Some(_)) => app.toast(
                "已将旧版禁用的启动快捷方式移至备份目录，避免 Windows 弹出文件打开方式。",
                Tone::Info,
            ),
            Ok(None) => {}
            Err(error) => app.toast(format!("无法备份旧启动项：{error}"), Tone::Warning),
        }
        app
    }

    // ------------------------------------------------------------ selection ----

    fn save_settings(&mut self) {
        self.settings_error = self.settings.save().err().map(|error| error.to_string());
    }

    pub fn selected(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == self.selected_id)
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn select(&mut self, id: String, ctx: &egui::Context) {
        if self.selected_id == id {
            return;
        }
        self.selected_id = id.clone();
        self.settings.set_last_account(&id);
        self.save_settings();
        self.refresh_usage(true);
        ctx.request_repaint();
    }

    pub fn running_count(&self) -> usize {
        self.accounts
            .iter()
            .filter(|a| a.running_pid.is_some())
            .count()
    }

    pub fn usage_name(&self) -> String {
        self.selected().map(|a| a.name.clone()).unwrap_or_default()
    }

    // ---------------------------------------------------------------- data ----

    pub fn refresh_usage(&mut self, animate: bool) {
        let Some(account) = self.selected().cloned() else {
            self.usage = None;
            self.recent_sessions.clear();
            return;
        };
        self.usage = data::read_usage(&self.install, &account);
        self.recent_sessions = data::list_sessions(&self.install, &account);
        let (fh, sd) = self
            .usage
            .as_ref()
            .map(|u| {
                (
                    u.fh.unwrap_or(0) as f32 / 100.0,
                    u.sd.unwrap_or(0) as f32 / 100.0,
                )
            })
            .unwrap_or((0.0, 0.0));
        let switched = self.bar_owner != self.selected_id;
        // The periodic meta refresh runs every few seconds. Kicking a 900 ms burst of
        // full-rate repaints on every tick would burn a core while the user is just
        // looking at the window, so only animate when the numbers really moved.
        let moved =
            (fh - self.bar_fh.target).abs() > 0.004 || (sd - self.bar_sd.target).abs() > 0.004;
        if switched {
            // Coming from another account, grow the meters from zero instead of sliding
            // from a stranger's numbers.
            self.bar_fh.snap(0.0);
            self.bar_sd.snap(0.0);
            self.bar_owner = self.selected_id.clone();
        }
        if animate && (switched || moved) {
            self.animate_for(Duration::from_millis(900));
            self.bar_fh.to(fh);
            self.bar_sd.to(sd);
        } else {
            self.bar_fh.snap(fh);
            self.bar_sd.snap(sd);
        }
    }

    pub fn reload_accounts(&mut self, ctx: &egui::Context) {
        let mut accounts = data::discover_accounts(&self.install, &mut self.settings);
        for account in &mut accounts {
            data::refresh_metadata(&self.install, account);
            account.running_pid = self.proc_map.get(&data::running_key(account)).copied();
        }
        let keep = accounts.iter().any(|a| a.id == self.selected_id);
        if !keep {
            self.selected_id = accounts
                .first()
                .map(|a| a.id.clone())
                .unwrap_or_else(|| DEFAULT_ID.to_owned());
        }
        self.accounts = accounts;
        self.identity
            .retain(|id, _| self.accounts.iter().any(|account| &account.id == id));
        self.avatar_textures
            .retain(|id, _| self.accounts.iter().any(|account| &account.id == id));
        self.load_identities_in(Some(ctx));
        self.save_settings();
        self.refresh_usage(false);
        self.rebuild_tray();
        ctx.request_repaint();
    }

    fn refresh_meta(&mut self) {
        for account in &mut self.accounts {
            data::refresh_metadata(&self.install, account);
            account.running_pid = self.proc_map.get(&data::running_key(account)).copied();
        }
        self.refresh_usage(true);
    }

    /// Reads each account's e-mail and any locally chosen avatar. Both are cached; the
    /// IndexedDB reads are refreshed periodically so a new login replaces stale identity.
    pub fn load_identities(&mut self) {
        self.load_identities_in(None);
    }

    fn load_identities_in(&mut self, ctx: Option<&egui::Context>) {
        for account in &self.accounts {
            if self.identity.get(&account.id).is_some_and(Option::is_some) {
                continue;
            }
            let roots = data::profile_data_roots(&self.install, account);
            let found = identity::identity_from_profile(&roots);
            self.identity.insert(account.id.clone(), found);
        }
        let Some(ctx) = ctx else {
            return;
        };
        for account in &self.accounts {
            if self.avatar_textures.contains_key(&account.id) {
                continue;
            }
            let Some(path) =
                identity::custom_avatar_path(&account.id, self.legacy_avatar_unique(&account.id))
            else {
                continue;
            };
            let name = format!("avatar-{}", account.id);
            if let Some(texture) = load_avatar_texture(ctx, &path, name) {
                self.avatar_textures.insert(account.id.clone(), texture);
            }
        }
    }

    /// The e-mail shown under an account name.
    pub fn account_email(&self, id: &str) -> Option<&str> {
        self.identity
            .get(id)
            .and_then(|found| found.as_ref())
            .and_then(|identity| identity.email.as_deref())
    }

    /// A readable display name: a user-chosen avatar label, then the e-mail's local
    /// part. The account's switcher label stays the primary line in the list.
    pub fn account_display_name(&self, account: &Account) -> String {
        if let Some(email) = self.account_email(&account.id) {
            return identity::display_name_from_email(email);
        }
        account.name.clone()
    }

    /// Primary line for an account: the switcher label, unless that label is still the
    /// generated folder name and the e-mail gives something more human.
    pub fn row_heading(&self, account: &Account) -> String {
        if account.name == account.id && self.account_email(&account.id).is_some() {
            return self.account_display_name(account);
        }
        account.name.clone()
    }

    /// Opens the system image picker for an account avatar.
    pub fn pick_avatar(&mut self, account_id: &str, ctx: &egui::Context) {
        if let Some(path) = win::pick_image_file() {
            self.set_custom_avatar(account_id, &path, ctx);
        }
    }

    fn legacy_avatar_unique(&self, id: &str) -> bool {
        let legacy = identity::legacy_avatar_stem(id).to_lowercase();
        self.accounts
            .iter()
            .filter(|account| identity::legacy_avatar_stem(&account.id).to_lowercase() == legacy)
            .count()
            == 1
    }

    pub fn set_custom_avatar(
        &mut self,
        account_id: &str,
        source: &std::path::Path,
        ctx: &egui::Context,
    ) {
        let Some(dir) = identity::avatar_dir() else {
            return;
        };
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.show_message("头像保存失败", error.to_string(), Tone::Error);
            return;
        }
        let target = dir.join(format!("{}.png", identity::avatar_stem(account_id)));
        // Re-encode through the image crate so oversized phone photos stay small and any
        // format the dialog accepted ends up as a PNG.
        let Ok(image) = read_avatar_image(source) else {
            self.show_message("头像不可用", "请选择可读取的 PNG 或 JPEG 图片（边长不超过 16384 像素，解码内存不超过 256 MiB）。", Tone::Error);
            return;
        };
        let thumb = image.resize_to_fill(256, 256, image::imageops::FilterType::Lanczos3);
        let mut encoded = std::io::Cursor::new(Vec::new());
        if thumb
            .write_to(&mut encoded, image::ImageFormat::Png)
            .is_err()
            || crate::storage::write_atomic(&target, encoded.get_ref(), true).is_err()
        {
            self.show_message("头像保存失败", "无法写入头像文件。", Tone::Error);
            return;
        }
        for path in identity::avatar_paths(account_id, self.legacy_avatar_unique(account_id)) {
            if path != target {
                let _ = std::fs::remove_file(path);
            }
        }
        self.avatar_textures.remove(account_id);
        let name = format!(
            "avatar-{account_id}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        if let Some(texture) = load_avatar_texture(ctx, &target, name) {
            self.avatar_textures.insert(account_id.to_owned(), texture);
        }
        ctx.request_repaint();
        self.toast("已更新账号头像。", Tone::Success);
    }

    pub fn clear_custom_avatar(&mut self, account_id: &str, ctx: &egui::Context) {
        for path in identity::avatar_paths(account_id, self.legacy_avatar_unique(account_id)) {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    self.show_message("头像删除失败", error.to_string(), Tone::Error);
                    return;
                }
            }
        }
        self.avatar_textures.remove(account_id);
        ctx.request_repaint();
        self.toast("已恢复默认头像。", Tone::Info);
    }

    // ------------------------------------------------------------- process ----

    fn spawn_query(&mut self, ctx: &egui::Context) {
        if self.querying {
            return;
        }
        self.querying = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let list = win::query_claude_processes();
            let _ = tx.send(list);
            ctx.request_repaint();
        });
    }

    fn apply_processes(&mut self, list: Vec<ProcessInfo>) {
        let mut map: HashMap<String, u32> = HashMap::new();
        for process in list {
            if let Some(key) = win::process_profile_key(&process.cmdline, &self.install) {
                map.entry(key).or_insert(process.pid);
            }
        }
        self.proc_map = map;
        let mut changed = false;
        for account in &mut self.accounts {
            let pid = self.proc_map.get(&data::running_key(account)).copied();
            if account.running_pid != pid {
                account.running_pid = pid;
                changed = true;
            }
        }
        if changed {
            self.rebuild_tray();
        }
    }

    fn poll_process(&mut self, ctx: &egui::Context) {
        if let Ok(list) = self.rx.try_recv() {
            self.querying = false;
            self.apply_processes(list);
        }
        let now = Instant::now();
        if now >= self.next_taskbar {
            self.next_taskbar = now + Duration::from_secs(2);
            if let Err(error) = crate::taskbar::sync_profiles(
                &self.install,
                &self.accounts,
                &mut self.taskbar_icons,
            ) {
                // Keep account operations usable if a window exits during enumeration.
                // Retry on the next poll; this does not affect Claude profile data.
                eprintln!("Taskbar identity update: {error}");
            }
        }
        if now >= self.next_poll {
            self.next_poll = now + Duration::from_millis(2000);
            let pids = win::claude_pids();
            if pids != self.known_pids || now >= self.next_full_query {
                self.known_pids = pids;
                self.next_full_query = now + Duration::from_secs(12);
                self.spawn_query(ctx);
            }
        }
        if let Some(at) = self.pending_query_at {
            if now >= at {
                self.pending_query_at = None;
                self.spawn_query(ctx);
            }
        }
    }

    // --------------------------------------------------------- quota watch ----

    fn run_quota_watch(&mut self) -> Vec<String> {
        let mut watch = self.settings.quota_watch();
        let now = Local::now();
        let now_ms = now.timestamp_millis();
        let mut changed = false;
        let mut notes: Vec<String> = Vec::new();

        for account in self.accounts.clone() {
            let Some(q) = data::read_usage(&self.install, &account) else {
                continue;
            };
            let Some(prev) = watch.get(&account.id).and_then(Value::as_object).cloned() else {
                watch.insert(
                    account.id.clone(),
                    json!({
                        "T": q.t, "Fh": q.fh, "Sd": q.sd, "Org": q.org,
                        "FhNoticeAt": Value::Null, "SdNoticeAt": Value::Null,
                        "FhNoticeFor": Value::Null, "SdNoticeFor": Value::Null
                    }),
                );
                changed = true;
                continue;
            };

            let prev_t = prev.get("T").and_then(Value::as_i64).unwrap_or(0);
            let prev_fh = prev
                .get("Fh")
                .and_then(Value::as_i64)
                .filter(|n| (0..=100).contains(n));
            let prev_sd = prev
                .get("Sd")
                .and_then(Value::as_i64)
                .filter(|n| (0..=100).contains(n));
            let prev_org = prev.get("Org").and_then(Value::as_str).unwrap_or_default();
            let mut fh_notice_at = prev.get("FhNoticeAt").and_then(Value::as_i64);
            let mut sd_notice_at = prev.get("SdNoticeAt").and_then(Value::as_i64);
            let mut fh_notice_for = prev.get("FhNoticeFor").and_then(Value::as_i64);
            let mut sd_notice_for = prev.get("SdNoticeFor").and_then(Value::as_i64);

            let mut fh_notified = false;
            let mut sd_notified = false;
            let mut fh_key: Option<i64> = None;
            let mut sd_key: Option<i64> = None;
            let mut tick_changed = false;

            if q.t > prev_t {
                tick_changed = true;
                if !prev_org.is_empty() && !q.org.is_empty() && prev_org != q.org {
                    watch.insert(
                        account.id.clone(),
                        json!({
                            "T": q.t, "Fh": q.fh, "Sd": q.sd, "Org": q.org,
                            "FhNoticeAt": fh_notice_at, "SdNoticeAt": sd_notice_at,
                            "FhNoticeFor": fh_notice_for, "SdNoticeFor": sd_notice_for
                        }),
                    );
                    changed = true;
                    continue;
                }

                let fh_drop = match (q.fh, prev_fh) {
                    (Some(cur), Some(old)) => cur <= 40 && cur <= old - 15,
                    _ => false,
                };
                let sd_drop = match (q.sd, prev_sd) {
                    (Some(cur), Some(old)) => cur <= 5 && cur <= old - 5,
                    _ => false,
                };
                let recent_fh = fh_notice_at
                    .is_some_and(|t| (0..45 * 60 * 1000).contains(&now_ms.saturating_sub(t)));
                let recent_sd = sd_notice_at
                    .is_some_and(|t| (0..2 * 60 * 60 * 1000).contains(&now_ms.saturating_sub(t)));

                if fh_drop && !recent_fh {
                    notes.push(format!(
                        "{}：5 小时额度已重置（现 {}%）",
                        account.name,
                        q.fh.unwrap_or(0)
                    ));
                    fh_notified = true;
                }
                if sd_drop && !recent_sd {
                    notes.push(format!(
                        "{}：本周额度已重置（现 {}%）",
                        account.name,
                        q.sd.unwrap_or(0)
                    ));
                    sd_notified = true;
                }
            }

            let recent_fh = fh_notice_at
                .is_some_and(|t| (0..45 * 60 * 1000).contains(&now_ms.saturating_sub(t)));
            let recent_sd = sd_notice_at
                .is_some_and(|t| (0..2 * 60 * 60 * 1000).contains(&now_ms.saturating_sub(t)));

            if !fh_notified && q.fh_active {
                if let Some(reset) = q.fh_reset_at {
                    if now >= reset && q.updated <= reset {
                        let key = reset.timestamp_millis();
                        let same = fh_notice_for.is_some_and(|v| v.abs_diff(key) < 30 * 60 * 1000);
                        let fresh = (0..6 * 60 * 60 * 1000).contains(&now_ms.saturating_sub(key));
                        if !recent_fh && !same && fresh {
                            notes.push(format!(
                                "{}：5 小时额度已重置（按上次记录推算）",
                                account.name
                            ));
                            fh_notified = true;
                            fh_key = Some(key);
                        }
                    }
                }
            }

            if !sd_notified {
                if let Some((last, _)) = q
                    .sd_anchor
                    .and_then(|anchor| data::weekly_window(anchor, now))
                {
                    if q.updated <= last && (now - last).num_hours() <= 24 {
                        let key = last.timestamp_millis();
                        let same =
                            sd_notice_for.is_some_and(|v| v.abs_diff(key) < 12 * 60 * 60 * 1000);
                        if !recent_sd && !same {
                            notes.push(format!(
                                "{}：本周额度已重置（按上次记录推算）",
                                account.name
                            ));
                            sd_notified = true;
                            sd_key = Some(key);
                        }
                    }
                }
            }

            if tick_changed || fh_notified || sd_notified {
                if fh_notified {
                    fh_notice_at = Some(now_ms);
                }
                if sd_notified {
                    sd_notice_at = Some(now_ms);
                }
                if let Some(key) = fh_key {
                    fh_notice_for = Some(key);
                }
                if let Some(key) = sd_key {
                    sd_notice_for = Some(key);
                }
                watch.insert(
                    account.id.clone(),
                    json!({
                        "T": q.t, "Fh": q.fh, "Sd": q.sd, "Org": q.org,
                        "FhNoticeAt": fh_notice_at, "SdNoticeAt": sd_notice_at,
                        "FhNoticeFor": fh_notice_for, "SdNoticeFor": sd_notice_for
                    }),
                );
                changed = true;
            }
        }

        if changed {
            self.settings.set_quota_watch(watch);
            self.save_settings();
        }
        notes
    }

    fn maybe_quota_watch(&mut self) {
        let now = Instant::now();
        if now < self.next_watch {
            return;
        }
        self.next_watch = now + Duration::from_secs(60);
        let notes = self.run_quota_watch();
        if !notes.is_empty() {
            self.toast(notes.join("；"), Tone::Info);
        }
    }

    // ---------------------------------------------------------------- toast ----

    pub fn toast(&mut self, text: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast {
            text: text.into(),
            tone,
            at: Instant::now(),
        });
        self.animate_for(Duration::from_millis(600));
    }

    /// Keeps the frame loop at full rate for a short while, so fades and bars finish
    /// smoothly without running at 60 fps for the whole life of the window.
    pub fn animate_for(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        if until > self.animate_until {
            self.animate_until = until;
        }
    }

    fn expire_toast(&mut self) {
        if let Some(toast) = &self.toast {
            if toast.at.elapsed() > Duration::from_millis(4600) {
                self.toast = None;
            }
        }
    }

    pub fn show_message(&mut self, title: impl Into<String>, body: impl Into<String>, tone: Tone) {
        self.dialog = Dialog::Message {
            title: title.into(),
            body: body.into(),
            tone,
            primary: "好".into(),
            secondary: None,
            action: DialogAction::None,
        };
    }

    // --------------------------------------------------------------- actions ----

    pub fn launch(&mut self, id: &str, ctx: &egui::Context) {
        self.ensure_install(ctx);
        let Some(account) = self.account(id).cloned() else {
            return;
        };
        self.select(account.id.clone(), ctx);

        if let Some(pid) = account.running_pid {
            if win::focus_process_window(pid) {
                self.toast(format!("已切换到 '{}'。", account.name), Tone::Success);
                return;
            }
        }

        let result = if account.is_default {
            win::launch_default(&self.install)
        } else {
            win::launch_profile(&self.install, &account.path)
        };
        match result {
            Ok(()) => {
                self.toast(format!("正在打开 '{}'…", account.name), Tone::Info);
                self.pending_query_at = Some(Instant::now() + Duration::from_millis(1300));
            }
            Err(error) => self.show_message("启动失败", error, Tone::Error),
        }
    }

    /// Claude's Store updates replace the versioned MSIX folder. Re-resolve the install
    /// before launching instead of failing on the path cached at start-up.
    fn ensure_install(&mut self, ctx: &egui::Context) {
        if self.install.available() {
            return;
        }
        let fresh = data::discover_install(None);
        if fresh.available() {
            self.install = fresh;
            self.settings.set_install_cache(&self.install);
            self.save_settings();
            self.reload_accounts(ctx);
        }
    }

    /// Asks before creating anything: this opens a login window in a new profile.
    pub fn request_add_account(&mut self) {
        if !self.install.available() && self.install.aumid.is_none() {
            self.show_message(
                "找不到 Claude 桌面版",
                format!(
                    "请先安装 Claude，或确认它仍然存在：\n{}",
                    self.install.exe.display()
                ),
                Tone::Error,
            );
            return;
        }
        let name = data::next_account_name(&self.accounts);
        self.dialog = Dialog::AddAccount { name };
    }

    pub fn add_account(&mut self, ctx: &egui::Context, name: &str) {
        self.ensure_install(ctx);
        if let Err(error) = data::validate_label(name, &self.accounts, None) {
            self.show_message("添加账号失败", error, Tone::Error);
            return;
        }
        let path = paths()
            .local
            .join(format!("{}{}", data::PROFILE_PREFIX, name));
        match win::launch_profile(&self.install, &path) {
            Ok(()) => {
                self.reload_accounts(ctx);
                self.select(name.to_owned(), ctx);
                self.toast(format!("已创建 '{name}'，正在打开登录窗口…"), Tone::Success);
                self.pending_query_at = Some(Instant::now() + Duration::from_millis(1300));
            }
            Err(error) => self.show_message("添加账号失败", error, Tone::Error),
        }
    }

    pub fn rename_account(
        &mut self,
        id: &str,
        name: &str,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        data::validate_label(name, &self.accounts, Some(id))?;
        let name = name.trim().to_owned();
        self.settings.set_label(id, Some(&name));
        self.settings
            .save()
            .map_err(|error| format!("无法保存名称：{error}"))?;
        self.reload_accounts(ctx);
        self.selected_id = id.to_owned();
        self.toast(format!("已重命名为 '{name}'。"), Tone::Success);
        Ok(())
    }

    pub fn delete_account(&mut self, id: &str, ctx: &egui::Context) {
        // Confirmation may have been open across multiple polls or a new launch.
        // Refresh now, and refuse deletion if process ownership cannot be checked.
        let processes = win::query_claude_processes();
        let unreadable = win::claude_pids()
            .iter()
            .any(|pid| !processes.iter().any(|process| process.pid == *pid));
        self.apply_processes(processes);
        if unreadable {
            self.show_message(
                "无法确认账号状态",
                "无法读取部分 Claude 进程的状态。请关闭 Claude 后重试删除。",
                Tone::Warning,
            );
            return;
        }
        let Some(account) = self.account(id).cloned() else {
            return;
        };
        if account.is_default {
            return;
        }
        if account.running_pid.is_some() {
            self.show_message(
                "账号正在运行",
                format!("请先关闭 '{}' 的窗口，再删除它。", account.name),
                Tone::Warning,
            );
            return;
        }
        match data::delete_account_dir(&account) {
            Ok(()) => {
                self.settings.set_label(&account.id, None);
                self.settings.forget_color(&account.id);
                self.save_settings();
                self.reload_accounts(ctx);
                self.toast(format!("已删除 '{}'。", account.name), Tone::Success);
            }
            Err(error) => self.show_message("删除失败", error.to_string(), Tone::Error),
        }
    }

    pub fn open_folder(&mut self, id: &str) {
        let Some(account) = self.account(id).cloned() else {
            return;
        };
        let path = account
            .data_path
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| Some(account.path.clone()).filter(|p| p.is_dir()));
        match path {
            Some(path) => {
                if let Err(error) = win::open_folder(&path) {
                    self.show_message("打开文件夹失败", error.to_string(), Tone::Error);
                }
            }
            None => self.toast("该账号还没有数据目录，首次启动时会自动创建。", Tone::Info),
        }
    }

    pub fn open_transfer(&mut self, id: &str) {
        let Some(source) = self.account(id).cloned() else {
            return;
        };
        let sessions = data::list_sessions(&self.install, &source);
        if sessions.is_empty() {
            self.show_message(
                "没有可转移内容",
                format!("'{}' 没有可转移的 Claude Code 对话。", source.name),
                Tone::Info,
            );
            return;
        }
        let targets: Vec<&Account> = self.accounts.iter().filter(|a| a.id != source.id).collect();
        if targets.is_empty() {
            self.show_message(
                "没有其他账号",
                "请先添加第二个账号，再向其转移对话。",
                Tone::Info,
            );
            return;
        }
        let checked = vec![false; sessions.len()];
        self.dialog = Dialog::Transfer(Box::new(Transfer {
            source_id: source.id,
            target_id: targets[0].id.clone(),
            sessions,
            checked,
            hover: None,
        }));
    }

    pub fn perform_copy(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_copy.take() else {
            return;
        };
        let Some(target) = self.account(&pending.target_id).cloned() else {
            return;
        };
        let Some(store) = data::session_store(&self.install, &target) else {
            self.show_message(
                "目标账号尚未就绪",
                format!(
                    "'{}' 还没用过 Claude Code，对话没有可存放的位置。\n\n请先打开它、登录并进入 Code 页一次，然后再试。",
                    target.name
                ),
                Tone::Warning,
            );
            return;
        };
        let mut copied = 0usize;
        let mut skipped = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for session in &pending.sessions {
            match data::copy_session(session, &store) {
                Ok(true) => copied += 1,
                Ok(false) => skipped += 1,
                Err(error) => failures.push(error.to_string()),
            }
        }
        self.refresh_meta();
        self.reload_accounts(ctx);

        if !failures.is_empty() {
            self.show_message(
                "转移失败",
                format!(
                    "已复制 {copied} 个，失败 {} 个。\n\n{}",
                    failures.len(),
                    failures.join("\n")
                ),
                Tone::Error,
            );
            return;
        }
        let mut body = format!("已复制 {copied} 个对话到 '{}'。", target.name);
        if skipped > 0 {
            body.push_str(&format!(" 另有 {skipped} 个已存在。"));
        }
        let running = target.running_pid.is_some();
        self.dialog = Dialog::Message {
            title: "对话已复制".into(),
            body,
            tone: Tone::Success,
            primary: if running {
                "完成".into()
            } else {
                "打开账号".into()
            },
            secondary: if running { None } else { Some("稍后".into()) },
            action: if running {
                DialogAction::None
            } else {
                DialogAction::Launch(target.id.clone())
            },
        };
    }

    pub fn toggle_startup(&mut self, ctx: &egui::Context) {
        let enable = !self.startup;
        match win::set_startup(enable, &win::current_exe()) {
            Ok(()) => {
                self.startup = enable;
                self.toast(
                    if enable {
                        "已开启开机自动启动。"
                    } else {
                        "已关闭开机自动启动。"
                    },
                    Tone::Success,
                );
                self.rebuild_tray();
                ctx.request_repaint();
            }
            Err(error) => self.show_message("设置启动项失败", error.to_string(), Tone::Error),
        }
    }

    pub fn open_settings_file(&mut self) {
        if let Err(error) = win::open_file(self.settings.path()) {
            self.show_message("打开设置失败", error.to_string(), Tone::Error);
        }
    }

    pub fn show_about(&mut self) {
        let body = format!(
            "Claude 账号切换器 {APP_VERSION}\n\n\
             Claude 桌面版 {}\n\
             账号数据：{}\n\
             设置文件：{}\n\n\
             账号目录与登录状态沿用旧切换器，无需重新登录。",
            self.install.version,
            paths().profiles.display(),
            self.settings.path().display()
        );
        self.dialog = Dialog::Message {
            title: "关于".into(),
            body,
            tone: Tone::Info,
            primary: "好".into(),
            secondary: None,
            action: DialogAction::None,
        };
    }

    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        let mode = if self.pal.is_dark() {
            theme::Mode::Light
        } else {
            theme::Mode::Dark
        };
        self.pal = theme::Palette::from_mode(mode);
        theme::install_style(ctx, &self.pal);
        self.settings.set_theme(match mode {
            theme::Mode::Light => "light",
            theme::Mode::Dark => "dark",
        });
        self.save_settings();
        win::apply_window_style(self.pal.is_dark());
        ctx.request_repaint();
    }

    pub fn show_window(&mut self, _ctx: &egui::Context) {
        // Act on the HWND directly: while hidden there may be no frame running to apply a
        // viewport command, and a tray click must work regardless.
        crate::win::reveal_main_window();
        if self.tray_hint_pending {
            self.tray_hint_pending = false;
            self.toast("切换器仍在后台运行；点击通知区图标可重新打开。", Tone::Info);
        }
    }

    pub fn quit(&mut self, ctx: &egui::Context) {
        self.quit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.quit {
            return;
        }
        if self.has_tray() {
            // Any close affordance (window ×, taskbar thumbnail ×, Alt+F4) only closes
            // the window. Closing hides it to the notification area; quitting is reserved
            // for the tray icon's 退出 item.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::win::close_to_tray();
            if !self.settings.tray_hint_shown() {
                self.settings.set_tray_hint_shown();
                self.save_settings();
                self.tray_hint_pending = true;
            }
        } else {
            // No tray icon means no way to reopen a closed UI.
            self.quit = true;
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if !matches!(self.dialog, Dialog::None) {
            return;
        }
        let new = take_add_account_shortcut(ctx);
        let (up, down, enter, rename, delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::F2),
                i.key_pressed(egui::Key::Delete),
            )
        });
        if up || down {
            if let Some(index) = self.accounts.iter().position(|a| a.id == self.selected_id) {
                let next = if up {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(self.accounts.len().saturating_sub(1))
                };
                if let Some(account) = self.accounts.get(next) {
                    self.select(account.id.clone(), ctx);
                }
            }
        }
        if enter {
            self.launch(&self.selected_id.clone(), ctx);
        }
        if rename {
            if let Some(account) = self.selected() {
                self.dialog = Dialog::Rename {
                    id: account.id.clone(),
                    buf: account.name.clone(),
                    focus: false,
                    error: None,
                };
            }
        }
        if delete {
            if let Some(account) = self.selected() {
                if !account.is_default {
                    self.dialog = Dialog::Delete {
                        id: account.id.clone(),
                    };
                }
            }
        }
        if new {
            self.request_add_account();
        }
    }

    /// True when the IME was composing very recently, meaning the next Enter keypress is
    /// most likely the IME's own confirmation rather than a dialog shortcut.
    pub fn ime_recently_active(&self) -> bool {
        // The composition is open now, or was open one frame ago (the confirming Enter
        // clears the buffer before egui sees the key), or the IME just committed text.
        self.ime_composing
            || self.ime_composing_prev
            || self
                .ime_commit_at
                .is_some_and(|at| at.elapsed() < Duration::from_millis(200))
    }

    pub fn apply_action(&mut self, action: UiAction, ctx: &egui::Context) {
        self.menu = None;
        match action {
            UiAction::Launch(id) => self.launch(&id, ctx),
            UiAction::Select(id) => self.select(id, ctx),
            UiAction::AddAccount => self.request_add_account(),
            UiAction::Rename(id) => {
                if let Some(account) = self.account(&id).cloned() {
                    self.select(id, ctx);
                    self.dialog = Dialog::Rename {
                        id: account.id,
                        buf: account.name,
                        focus: false,
                        error: None,
                    };
                }
            }
            UiAction::Delete(id) => {
                if self.account(&id).is_some_and(|a| !a.is_default) {
                    self.select(id.clone(), ctx);
                    self.dialog = Dialog::Delete { id };
                }
            }
            UiAction::OpenFolder(id) => self.open_folder(&id),
            UiAction::Transfer(id) => {
                self.select(id.clone(), ctx);
                self.open_transfer(&id);
            }
            UiAction::SetAvatar(id) => {
                self.pick_avatar(&id, ctx);
            }
            UiAction::ClearAvatar(id) => self.clear_custom_avatar(&id, ctx),
            UiAction::ToggleStartup => self.toggle_startup(ctx),
            UiAction::ToggleTheme => self.toggle_theme(ctx),
            UiAction::OpenSettings => self.open_settings_file(),
            UiAction::About => self.show_about(),
        }
    }

    fn apply_dialog_action(&mut self, action: DialogAction, ctx: &egui::Context) {
        match action {
            DialogAction::None => {}
            DialogAction::Launch(id) => {
                self.dialog = Dialog::None;
                self.launch(&id, ctx);
            }
            DialogAction::PerformCopy => {
                self.dialog = Dialog::None;
                self.perform_copy(ctx);
            }
        }
    }
}

impl eframe::App for SwitcherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = crate::anim::dt(ctx);
        if self.motion {
            self.time += dt;
        }

        // Any pointer/keyboard activity means a hover fade or press effect is running.
        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.animate_for(Duration::from_millis(420));
        }
        self.handle_close(ctx);
        // Record whether the IME owned the keyboard last frame. A Chinese IME keeps a
        // composition open while pinyin is typed; the Enter that converts it arrives in
        // the same frame the composition buffer is cleared, so the previous frame is the
        // only place that still shows "the IME was in charge here".
        self.ime_composing_prev = self.ime_composing;
        self.ime_composing = win::ime_is_composing();
        // A commit means the IME just consumed a keystroke (Enter or Space) to turn the
        // pinyin into text. Windows re-delivers that same Enter as a normal key event,
        // which would otherwise confirm the dialog the user is still typing in.
        let ime_committed = ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty())
            })
        });
        if ime_committed {
            self.ime_commit_at = Some(Instant::now());
        }
        self.handle_keyboard(ctx);
        self.poll_process(ctx);
        self.poll_tray(ctx);

        let now = Instant::now();
        // Hidden or minimized, there is nothing to look at. Keep the cheap probes (tray,
        // Claude processes) alive but stop re-reading every account's files at the
        // on-screen cadence.
        let window_active = !win::is_tray_closed()
            && win::is_main_window_visible()
            && !win::is_main_window_minimized();
        if now >= self.next_meta {
            self.next_meta = now
                + if window_active {
                    Duration::from_secs(4)
                } else {
                    Duration::from_secs(30)
                };
            self.refresh_meta();
        }
        if now >= self.next_identity {
            self.next_identity = now + Duration::from_secs(30);
            self.identity.clear();
            self.load_identities_in(Some(ctx));
        }
        self.maybe_quota_watch();
        self.expire_toast();

        if let Some(error) = self.settings_error.take() {
            self.toast(format!("设置未保存：{error}"), Tone::Error);
        }

        self.bar_fh.update(dt);
        self.bar_sd.update(dt);

        self.draw(ctx);

        if !self.styled && self.started.elapsed() > Duration::from_millis(150) {
            win::apply_window_style(self.pal.is_dark());
            self.styled = true;
        }

        // Full rate only while something is actually moving. Idle or hidden, poll slowly;
        // tray events still request an immediate repaint.
        let now = Instant::now();
        if self.motion && window_active && now < self.animate_until {
            ctx.request_repaint();
        } else if window_active {
            ctx.request_repaint_after(Duration::from_millis(500));
        } else {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = self.pal.bg;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }
}

pub fn tone_color(tone: Tone, pal: &theme::Palette) -> egui::Color32 {
    match tone {
        Tone::Info => pal.weekly,
        Tone::Success => pal.running,
        Tone::Warning => {
            if pal.is_dark() {
                egui::Color32::from_rgb(214, 168, 96)
            } else {
                egui::Color32::from_rgb(154, 108, 30)
            }
        }
        Tone::Error => pal.danger,
    }
}

/// Decodes a user-chosen avatar into an egui texture, circular-cropped at draw time.
fn load_avatar_texture(
    ctx: &egui::Context,
    path: &std::path::Path,
    name: String,
) -> Option<egui::TextureHandle> {
    let image = read_avatar_image(path).ok()?;
    let image = image.resize_to_fill(256, 256, image::imageops::FilterType::Lanczos3);
    let rgba = image.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn take_add_account_shortcut(ctx: &egui::Context) -> bool {
    // Read the key event's modifiers. The frame's final modifier state can already
    // be cleared when a quick Ctrl+N press and release arrive in the same frame.
    ctx.input_mut(|input| {
        input.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL,
            egui::Key::N,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn new_account_shortcut_survives_modifier_release_in_same_frame() {
        let ctx = egui::Context::default();
        let events = [true, false].map(|pressed| egui::Event::Key {
            key: egui::Key::N,
            physical_key: Some(egui::Key::N),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        });
        let input = egui::RawInput {
            modifiers: egui::Modifiers::NONE,
            events: events.into(),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            assert!(take_add_account_shortcut(ctx));
            assert!(!take_add_account_shortcut(ctx));
        });
    }

    #[test]
    fn avatar_decoder_accepts_normal_png_and_rejects_oversized_dimensions() {
        let dir = TestDir::new();
        let path = dir.path().join("avatar.png");
        image::RgbaImage::new(32, 32).save(&path).unwrap();
        assert!(read_avatar_image(&path).is_ok());
        image::RgbaImage::new(16_385, 1).save(&path).unwrap();
        assert!(read_avatar_image(&path).is_err());
    }
}
