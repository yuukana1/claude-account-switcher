use super::{Dialog, SwitcherApp, APP_VERSION};
use crate::update::{self, Update};
use chrono::Local;
use eframe::egui;
use semver::Version;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Update),
    Downloading {
        size: usize,
        received: Arc<AtomicUsize>,
    },
    Ready(Update, Vec<u8>),
    Restarting,
    Error(String),
}
enum Reply {
    Checked(Option<Update>),
    Downloaded(Update, Vec<u8>),
}
pub(super) struct Updates {
    phase: Phase,
    // Keep a verified download if the next day's check still advertises the
    // exact same asset (version, digest, URL and size).
    cached: Option<(Update, Vec<u8>)>,
    job: Option<Receiver<Result<Reply, String>>>,
    next_tick: Instant,
}
impl Default for Updates {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            cached: None,
            job: None,
            next_tick: Instant::now() + Duration::from_secs(3),
        }
    }
}
impl Updates {
    fn spawn(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce() -> Result<Reply, String> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(task());
            ctx.request_repaint();
        });
    }
}

impl SwitcherApp {
    pub(super) fn newer_version(&self) -> Option<&str> {
        let v = self.settings.available_version()?;
        (Version::parse(v).ok()? > Version::parse(APP_VERSION).ok()?).then_some(v)
    }

    fn start_check(&mut self, ctx: &egui::Context, day: &str, automatic: bool) {
        if self.updates.job.is_some() {
            return;
        }
        self.settings.record_update_attempt(day);
        self.save_settings();
        if automatic && self.settings_error.is_some() {
            return;
        }
        if let Phase::Ready(u, bytes) = std::mem::replace(&mut self.updates.phase, Phase::Checking)
        {
            self.updates.cached = Some((u, bytes));
        }
        self.updates
            .spawn(ctx, || update::check().map(Reply::Checked));
    }

    pub(super) fn open_updates(&mut self, ctx: &egui::Context) {
        self.menu = None;
        self.dialog = Dialog::Updates;
        if matches!(
            self.updates.phase,
            Phase::Idle | Phase::Current | Phase::Error(_)
        ) {
            self.start_check(ctx, &Local::now().date_naive().to_string(), false);
        }
    }

    pub(super) fn poll_updates(&mut self, ctx: &egui::Context) {
        let reply = self.updates.job.as_ref().map(Receiver::try_recv);
        if let Some(result) = reply {
            let message = match result {
                Ok(msg) => msg,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("更新任务已中断，请重试。".into()),
            };
            self.updates.job = None;
            self.updates.phase = match message {
                Ok(Reply::Checked(update)) => {
                    let version = update.as_ref().map(|u| u.version.to_string());
                    self.settings.set_available_version(version.as_deref());
                    self.save_settings();
                    self.rebuild_tray();
                    match (update, self.updates.cached.take()) {
                        (Some(u), Some((old, bytes))) if u == old => Phase::Ready(u, bytes),
                        (Some(u), _) => Phase::Available(u),
                        (None, _) => Phase::Current,
                    }
                }
                Ok(Reply::Downloaded(u, bytes)) => Phase::Ready(u, bytes),
                Err(e) => Phase::Error(e),
            };
        }
        if Instant::now() >= self.updates.next_tick {
            self.updates.next_tick = Instant::now() + Duration::from_secs(30);
            let day = Local::now().date_naive().to_string();
            if self.settings.daily_update_due(&day)
                && !matches!(self.updates.phase, Phase::Restarting)
            {
                self.start_check(ctx, &day, true);
            }
        }
    }

    pub(super) fn update_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let ctx = ui.ctx().clone();
        ui.label(format!("当前版本   v{APP_VERSION}   ·   Windows"));
        ui.separator();
        match &self.updates.phase {
            Phase::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("正在检查 GitHub 上的最新正式版…");
                });
            }
            Phase::Downloading { size, received } => {
                let n = received.load(Ordering::Relaxed);
                ui.label("正在下载更新，完成后将校验安装包。");
                ui.add(egui::ProgressBar::new(n as f32 / *size as f32).show_percentage());
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Phase::Available(u) | Phase::Ready(u, _) => {
                ui.strong(format!("新版本 v{}", u.version));
                ui.hyperlink_to(
                    "查看发行说明",
                    format!(
                        "https://github.com/yuukana1/claude-account-switcher/releases/tag/v{}",
                        u.version
                    ),
                );
                ui.label(if matches!(self.updates.phase, Phase::Ready(..)) {
                    "安装包已校验。重启只会关闭切换器，Claude 窗口将保持打开。".into()
                } else {
                    format!("Windows x64 便携版 · {:.1} MiB", u.size as f64 / 1048576.0)
                });
            }
            Phase::Current => {
                ui.label("已是最新正式版。");
            }
            Phase::Error(e) => {
                ui.colored_label(self.pal.danger, format!("更新未完成：{e}"));
            }
            Phase::Restarting => {
                ui.label("正在切换到新版…");
            }
            Phase::Idle => {
                ui.label("手动检查是否有新版本。");
            }
        }
        ui.separator();
        let mut enabled = self.settings.daily_updates();
        if ui.checkbox(&mut enabled, "每天自动检查一次").changed() {
            self.settings.set_daily_updates(enabled);
            self.save_settings();
            self.updates.next_tick = Instant::now();
        }
        if let Some(day) = self.settings.update_check_date() {
            ui.weak(format!("最近检查：{day}"));
        }
        let mut close = false;
        ui.horizontal(|ui| {
            match &self.updates.phase {
                Phase::Available(u) => {
                    if ui.button("下载更新").clicked() {
                        let u = u.clone();
                        let received = Arc::new(AtomicUsize::new(0));
                        self.updates.phase = Phase::Downloading {
                            size: u.size,
                            received: received.clone(),
                        };
                        self.updates.spawn(&ctx, move || {
                            let bytes =
                                update::download(&u, |n| received.store(n, Ordering::Relaxed))?;
                            Ok(Reply::Downloaded(u, bytes))
                        });
                    }
                }
                Phase::Ready(..) => {
                    if ui.button("替换并重启").clicked() {
                        // All download work is finished. Keep the final swap on the UI
                        // thread so tray quit cannot interrupt the two filesystem moves.
                        if let Phase::Ready(u, bytes) =
                            std::mem::replace(&mut self.updates.phase, Phase::Restarting)
                        {
                            match self.settings.save().and_then(|_| update::apply(&u, &bytes)) {
                                Ok(()) => self.quit(&ctx),
                                Err(e) => self.updates.phase = Phase::Error(e.to_string()),
                            }
                        }
                    }
                }
                Phase::Current | Phase::Error(_) | Phase::Idle
                    if ui.button("重新检查").clicked() =>
                {
                    self.start_check(&ctx, &Local::now().date_naive().to_string(), false);
                }
                _ => {}
            }
            if matches!(self.updates.phase, Phase::Available(_) | Phase::Ready(..))
                && ui.button("重新检查").clicked()
            {
                self.start_check(&ctx, &Local::now().date_naive().to_string(), false);
            }
            close = ui.button("关闭").clicked();
        });
        close
    }
}

#[cfg(test)]
mod tests {
    use super::super::interaction_tests::Fixture;
    use super::*;
    #[test]
    fn background_discovery_preserves_dialog_and_exposes_update_without_downloading() {
        let mut f = Fixture::new();
        let name = "Claude-Account-Switcher-9.0.0-windows-x64.exe";
        let json = serde_json::json!({"tag_name":"v9.0.0","draft":false,"prerelease":false,"assets":[{"name":name,"size":1024,"digest":format!("sha256:{}", "a".repeat(64)),"browser_download_url":format!("https://github.com/yuukana1/claude-account-switcher/releases/download/v9.0.0/{name}"),"state":"uploaded"}]});
        let release =
            update::parse_release(&serde_json::to_vec(&json).unwrap(), &Version::new(3, 2, 0))
                .unwrap();
        let (tx, rx) = mpsc::channel();
        f.app.updates.job = Some(rx);
        f.app.updates.next_tick = Instant::now() + Duration::from_secs(3600);
        f.app.dialog = Dialog::Rename {
            id: "Beta".into(),
            buf: "未保存".into(),
            focus: true,
            error: None,
        };
        tx.send(Ok(Reply::Checked(release))).unwrap();
        f.app.poll_updates(&f.ctx);
        assert!(matches!(&f.app.dialog,Dialog::Rename{buf,..} if buf=="未保存"));
        assert_eq!(f.app.newer_version(), Some("9.0.0"));
        f.app.dialog = Dialog::None;
        f.click_label("更多");
        f.click_label("检查更新…");
        assert!(matches!(f.app.dialog, Dialog::Updates));
        f.click_label("每天自动检查一次");
        assert!(!f.app.settings.daily_updates());
        f.click_label("关闭");
        assert!(matches!(f.app.dialog, Dialog::None));
        assert!(matches!(f.app.updates.phase, Phase::Available(_)));
        assert!(f.app.updates.job.is_none());
    }
}
