//! Main surface, structured like the Claude desktop client: a quiet sidebar for the
//! account list, and a detail canvas that gives the selected account the room to breathe.
//!
//! Everything is token-driven (see theme.rs); light and dark are the same painting code.

use chrono::Local;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Margin, Pos2, Rect, Response,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2,
};

use super::{
    tone_color, MenuKind, OpenMenu, SwitcherApp, Tone, UiAction, APP_VERSION, WINDOW_H, WINDOW_W,
};
use crate::anim::breathe;
use crate::data::{format_when, next_reset, system_time_to_local, Account};
use crate::theme::{self, display_font, semi_font, ui_font, Palette};

const TITLE_H: f32 = 42.0;
const SIDEBAR_W: f32 = 286.0;
const ROW_H: f32 = 58.0;

type IconFn = fn(&egui::Painter, Rect, Color32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

impl SwitcherApp {
    pub(super) fn draw(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.set_clip_rect(rect);
                let painter = ui.painter().clone();
                painter.rect_filled(rect, 0, self.pal.bg);

                self.draw_titlebar(ui, rect);

                let sidebar = Rect::from_min_max(
                    Pos2::new(rect.left(), TITLE_H),
                    Pos2::new(rect.left() + SIDEBAR_W, rect.bottom()),
                );
                let detail = Rect::from_min_max(Pos2::new(sidebar.right(), TITLE_H), rect.max);
                self.draw_sidebar(ui, sidebar);
                self.draw_detail(ui, detail);

                self.draw_toast(ctx);
                self.draw_menu(ctx);
                self.draw_dialog(ctx);
            });
    }

    // -------------------------------------------------------------- title ----

    fn draw_titlebar(&mut self, ui: &mut Ui, rect: Rect) {
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TITLE_H));
        let painter = ui.painter().clone();
        painter.rect_filled(bar, 0, self.pal.bg);
        painter.line_segment(
            [
                Pos2::new(bar.left(), bar.bottom() - 0.5),
                Pos2::new(bar.right(), bar.bottom() - 0.5),
            ],
            Stroke::new(1.0, self.pal.hairline),
        );

        theme::logo_image(
            &painter,
            &self.logo_small,
            Rect::from_center_size(
                Pos2::new(bar.left() + 22.0, bar.center().y),
                Vec2::splat(15.0),
            ),
        );

        let drag = ui.interact(bar, Id::new("titlebar_drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let theme_rect = Rect::from_center_size(
            Pos2::new(bar.right() - 82.0, bar.center().y),
            Vec2::splat(26.0),
        );
        let icon = if self.pal.is_dark() {
            theme::icon_sun as IconFn
        } else {
            theme::icon_moon as IconFn
        };
        if title_button(ui, theme_rect, icon, &self.pal) {
            self.toggle_theme(ui.ctx());
        }

        let close_rect = Rect::from_center_size(
            Pos2::new(bar.right() - 20.0, bar.center().y),
            Vec2::splat(26.0),
        );
        if title_button(ui, close_rect, theme::icon_close, &self.pal) {
            // Closing only closes the window, exactly like the taskbar thumbnail's ×.
            // The app stays alive in the notification area; only the tray's 退出 quits.
            if self.has_tray() {
                self.tray_hint_pending = true;
            } else {
                // No notification icon: a closed window could never be reopened.
                self.quit = true;
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let min_rect = Rect::from_center_size(
            Pos2::new(bar.right() - 50.0, bar.center().y),
            Vec2::splat(26.0),
        );
        if title_button(ui, min_rect, theme::icon_minimize, &self.pal) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    // ------------------------------------------------------------- sidebar ----

    fn draw_sidebar(&mut self, ui: &mut Ui, rect: Rect) {
        let pal = self.pal;
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0, pal.bg);
        painter.line_segment(
            [
                Pos2::new(rect.right() - 0.5, rect.top()),
                Pos2::new(rect.right() - 0.5, rect.bottom()),
            ],
            Stroke::new(1.0, pal.hairline),
        );

        // Brand lockup: serif "Claude", quiet "账号" after it.
        let mark = Rect::from_min_size(
            Pos2::new(rect.left() + 20.0, rect.top() + 20.0),
            Vec2::splat(26.0),
        );
        theme::logo_image(&painter, &self.logo_large, mark);
        painter.text(
            Pos2::new(mark.right() + 9.0, mark.top() + 12.0),
            Align2::LEFT_CENTER,
            "Claude",
            display_font(19.0),
            pal.ink,
        );
        let word = measure(ui.ctx(), "Claude", &display_font(19.0));
        painter.text(
            Pos2::new(mark.right() + 13.0 + word, mark.top() + 12.0),
            Align2::LEFT_CENTER,
            "账号",
            display_font(19.0),
            pal.muted,
        );

        painter.text(
            Pos2::new(rect.left() + 20.0, rect.top() + 56.0),
            Align2::LEFT_CENTER,
            format!("{} 个账号", self.accounts.len()),
            ui_font(10.0),
            pal.faint,
        );

        // A full-width, labelled action: an icon-only "+" hid the entry point.
        let add_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 12.0, rect.top() + 70.0),
            Vec2::new(rect.width() - 24.0, 34.0),
        );
        let add = button_at(
            ui,
            add_rect,
            "添加账号",
            Btn::Secondary,
            Some(theme::icon_plus as IconFn),
            true,
            &pal,
        );
        if add.clicked() {
            self.request_add_account();
        }
        if add.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let _ = add.on_hover_text("在新的独立目录里登录另一个账号（Ctrl+N）");

        let list_top = rect.top() + 114.0;
        let footer_top = rect.bottom() - 62.0;
        let list_rect = Rect::from_min_max(
            Pos2::new(rect.left(), list_top),
            Pos2::new(rect.right(), footer_top),
        );

        let accounts = self.accounts.clone();
        let selected_id = self.selected_id.clone();
        let mut actions: Vec<UiAction> = Vec::new();

        ui.scope_builder(
            UiBuilder::new().max_rect(list_rect.shrink2(Vec2::new(8.0, 0.0))),
            |ui| {
                ui.set_width(list_rect.width() - 16.0);
                ui.set_clip_rect(list_rect);
                egui::ScrollArea::vertical()
                    .id_salt("account_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(list_rect.width() - 20.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for account in &accounts {
                            let (row_rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), ROW_H),
                                Sense::click(),
                            );
                            let response = response.on_hover_cursor(CursorIcon::PointingHand);
                            self.draw_sidebar_row(
                                ui,
                                row_rect,
                                &response,
                                account,
                                account.id == selected_id,
                                &mut actions,
                            );
                        }
                        ui.allocate_space(Vec2::new(0.0, 6.0));
                    });
            },
        );

        let footer = Rect::from_min_max(
            Pos2::new(rect.left() + 12.0, footer_top),
            Pos2::new(rect.right() - 12.0, rect.bottom() - 10.0),
        );
        painter.line_segment(
            [
                Pos2::new(footer.left(), footer.top()),
                Pos2::new(footer.right(), footer.top()),
            ],
            Stroke::new(1.0, pal.hairline),
        );
        let more = make_footer_button(ui, footer, "更多", theme::icon_dots, &pal);
        if more.clicked() {
            let open = self.menu.as_ref().is_some_and(|m| m.kind == MenuKind::More);
            if open {
                self.menu = None;
            } else {
                self.menu = Some(OpenMenu {
                    kind: MenuKind::More,
                    account_id: self.selected_id.clone(),
                    pos: Pos2::new(footer.left(), footer.top() - 8.0 - menu_reserved_height()),
                    at: std::time::Instant::now(),
                });
            }
        }
        if more.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        painter.text(
            Pos2::new(footer.left() + 12.0, footer.center().y),
            Align2::LEFT_CENTER,
            format!("v{APP_VERSION}"),
            ui_font(9.8),
            pal.faint,
        );

        for action in actions {
            self.apply_action(action, ui.ctx());
        }
    }

    fn draw_sidebar_row(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        response: &Response,
        account: &Account,
        selected: bool,
        actions: &mut Vec<UiAction>,
    ) {
        let pal = self.pal;
        let painter = ui.painter().clone();
        let id = response.id;
        let hover = ui
            .ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered(), 0.15);
        let select = ui
            .ctx()
            .animate_bool_with_time(id.with("select"), selected, 0.20);

        if select > 0.01 || hover > 0.01 {
            // Hover is deliberately a whisper so it can never be mistaken for the
            // selection: a barely-there wash, while the selected account is a raised
            // card with a hairline border and a clay edge marker.
            let raised = context_surface(pal);
            let wash = theme::with_alpha(pal.ink, if pal.is_dark() { 0.05 } else { 0.024 } * hover);
            let base = theme::mix(wash, raised, select);
            if select > 0.01 {
                theme::soft_shadow(
                    &painter,
                    rect.shrink2(Vec2::new(2.0, 1.0)),
                    8,
                    3.0,
                    1.0,
                    if pal.is_dark() { 70 } else { 16 },
                );
            }
            painter.rect_filled(
                rect.shrink2(Vec2::new(2.0, 1.0)),
                CornerRadius::same(8),
                base,
            );
            if select > 0.01 {
                painter.rect_stroke(
                    rect.shrink2(Vec2::new(2.0, 1.0)),
                    CornerRadius::same(8),
                    Stroke::new(1.0, pal.border),
                    StrokeKind::Inside,
                );
                // The clay tick is the unmistakable "this one" signal; hover has none.
                let tick = Rect::from_min_size(
                    Pos2::new(rect.left() + 7.0, rect.center().y - 10.0),
                    Vec2::new(2.5, 20.0),
                );
                painter.rect_filled(
                    tick,
                    CornerRadius::same(2),
                    theme::with_alpha(pal.accent, 0.9 * select),
                );
            }
        }

        let avatar = Rect::from_center_size(
            Pos2::new(rect.left() + 12.0 + 16.0, rect.center().y),
            Vec2::splat(32.0),
        );
        self.draw_avatar(ui, avatar, account, 12.5);

        let text_x = avatar.right() + 10.0;
        let right_x = rect.right() - 16.0;
        let title = theme::truncate(
            ui.ctx(),
            &self.row_heading(account),
            &semi_font(12.4),
            (right_x - text_x - 20.0).max(60.0),
        );
        painter.text(
            Pos2::new(text_x, rect.center().y - 8.0),
            Align2::LEFT_CENTER,
            title,
            semi_font(12.4),
            pal.ink,
        );
        let sub = self
            .account_email(&account.id)
            .map(|e| e.to_owned())
            .unwrap_or_else(|| format!("{} 个 Code 对话", account.session_count));
        let sub = theme::truncate(
            ui.ctx(),
            &sub,
            &ui_font(9.8),
            (right_x - text_x - 20.0).max(50.0),
        );
        painter.text(
            Pos2::new(text_x, rect.center().y + 9.0),
            Align2::LEFT_CENTER,
            sub,
            ui_font(9.8),
            pal.faint,
        );

        if account.running_pid.is_some() {
            let pulse = 0.65 + 0.35 * breathe(self.time, 2.8);
            painter.circle_filled(
                Pos2::new(right_x - 3.0, rect.center().y),
                3.0,
                theme::with_alpha(pal.running, pulse),
            );
        }

        if response.clicked() {
            actions.push(UiAction::Select(account.id.clone()));
        }
        if response.double_clicked() {
            actions.push(UiAction::Launch(account.id.clone()));
        }
        if response.secondary_clicked() {
            let same = self
                .menu
                .as_ref()
                .is_some_and(|m| m.kind == MenuKind::Row && m.account_id == account.id);
            if same {
                self.menu = None;
            } else {
                let pos = ui
                    .ctx()
                    .input(|i| i.pointer.interact_pos())
                    .unwrap_or_else(|| response.rect.left_top());
                self.menu = Some(OpenMenu {
                    kind: MenuKind::Row,
                    account_id: account.id.clone(),
                    pos: pos + Vec2::new(4.0, 4.0),
                    at: std::time::Instant::now(),
                });
            }
        }
    }

    fn draw_avatar(&self, ui: &Ui, rect: Rect, account: &Account, letter_size: f32) {
        let pal = self.pal;
        let painter = ui.painter();
        match self.avatar_textures.get(&account.id) {
            Some(texture) => theme::circle_image(painter, texture, rect, pal.surface, 1.0),
            None if account.is_default => {
                theme::circle_image(painter, &self.logo_large, rect, pal.surface, 1.0)
            }
            None => {
                let tint = theme::avatar_tint(account.color, &pal);
                let radius = rect.width() * 0.5;
                painter.circle_filled(rect.center(), radius, tint);
                // A very soft top-left highlight gives the initial a material feel.
                painter.circle_filled(
                    rect.center() + Vec2::new(-radius * 0.30, -radius * 0.34),
                    radius * 0.55,
                    theme::with_alpha(Color32::WHITE, 0.07),
                );
                painter.circle_stroke(
                    rect.center(),
                    rect.width() * 0.5 - 0.5,
                    Stroke::new(1.0, theme::with_alpha(Color32::WHITE, 0.22)),
                );
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    badge_letter(&self.row_heading(account)),
                    semi_font(letter_size),
                    Color32::WHITE,
                );
            }
        }
    }

    // -------------------------------------------------------------- detail ----

    fn draw_detail(&mut self, ui: &mut Ui, rect: Rect) {
        let pal = self.pal;
        let painter = ui.painter().clone();
        theme::detail_ground(&painter, rect, &pal);
        // A very quiet warm wash keeps the canvas from reading as flat grey.
        let glow = if pal.is_dark() {
            theme::with_alpha(Color32::from_rgb(120, 96, 82), 0.05)
        } else {
            theme::with_alpha(pal.accent, 0.035)
        };
        theme::radial_glow(
            &painter,
            Pos2::new(rect.right() - 40.0, rect.top() + 10.0),
            420.0,
            glow,
        );

        let Some(account) = self.selected().cloned() else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "还没有账号",
                ui_font(13.0),
                pal.muted,
            );
            return;
        };

        let content = Rect::from_min_max(
            Pos2::new(rect.left() + 30.0, rect.top() + 24.0),
            Pos2::new(rect.right() - 28.0, rect.bottom() - 16.0),
        );

        // ---- identity block ----
        let avatar =
            Rect::from_min_size(Pos2::new(content.left(), content.top()), Vec2::splat(56.0));
        self.draw_avatar(ui, avatar, &account, 20.0);

        let name = self.row_heading(&account);
        let name_x = avatar.right() + 16.0;
        let name_width = (content.right() - name_x - 96.0).max(120.0);
        let name = theme::truncate(ui.ctx(), &name, &display_font(24.0), name_width);
        painter.text(
            Pos2::new(name_x, avatar.top() + 16.0),
            Align2::LEFT_CENTER,
            name,
            display_font(24.0),
            pal.ink,
        );

        let meta = match self.account_email(&account.id) {
            Some(email) => format!("{} · {} 个 Code 对话", email, account.session_count),
            None => format!("{} 个 Code 对话", account.session_count),
        };
        let meta = theme::truncate(ui.ctx(), &meta, &ui_font(10.6), name_width + 44.0);
        painter.text(
            Pos2::new(name_x, avatar.top() + 40.0),
            Align2::LEFT_CENTER,
            meta,
            ui_font(10.6),
            pal.faint,
        );

        let running = account.running_pid.is_some();
        let pill = Rect::from_min_size(
            Pos2::new(content.right() - 78.0, avatar.top() + 4.0),
            Vec2::new(78.0, 25.0),
        );
        if running {
            painter.rect_filled(pill, CornerRadius::same(13), pal.running_bg);
            let pulse = 0.65 + 0.35 * breathe(self.time, 2.8);
            painter.circle_filled(
                Pos2::new(pill.left() + 13.0, pill.center().y),
                3.0,
                theme::with_alpha(pal.running, pulse),
            );
            painter.text(
                Pos2::new(pill.left() + 23.0, pill.center().y),
                Align2::LEFT_CENTER,
                "运行中",
                ui_font(10.2),
                pal.running,
            );
        } else {
            painter.rect_stroke(
                pill,
                CornerRadius::same(13),
                Stroke::new(1.0, pal.border),
                StrokeKind::Inside,
            );
            painter.circle_filled(
                Pos2::new(pill.left() + 13.0, pill.center().y),
                2.6,
                pal.faint,
            );
            painter.text(
                Pos2::new(pill.left() + 23.0, pill.center().y),
                Align2::LEFT_CENTER,
                "未运行",
                ui_font(10.2),
                pal.faint,
            );
        }
        if let Some(last) = account.last_used {
            painter.text(
                Pos2::new(content.right(), avatar.top() + 42.0),
                Align2::RIGHT_CENTER,
                format_when(system_time_to_local(last)),
                ui_font(9.8),
                pal.faint,
            );
        }

        // ---- usage panel ----
        let actions_top = content.bottom() - 54.0;
        let mut usage_top = avatar.bottom() + 22.0;

        // A reset that is close deserves its own line, not just a pill buried in the
        // quota header. This banner appears only in the last half hour.
        let imminent_reset = self
            .usage
            .as_ref()
            .and_then(|usage| next_reset(usage, Local::now()))
            .filter(crate::data::ResetInfo::imminent);
        if let Some(reset) = &imminent_reset {
            let banner = Rect::from_min_size(
                Pos2::new(content.left(), usage_top),
                Vec2::new(content.width(), 30.0),
            );
            painter.rect_filled(banner, CornerRadius::same(8), pal.accent_soft);
            painter.rect_stroke(
                banner,
                CornerRadius::same(8),
                Stroke::new(1.0, theme::with_alpha(pal.accent, 0.35)),
                StrokeKind::Inside,
            );
            let pulse = 0.55 + 0.45 * breathe(self.time, 1.6);
            painter.circle_filled(
                Pos2::new(banner.left() + 15.0, banner.center().y),
                3.2,
                theme::with_alpha(pal.accent, pulse),
            );
            painter.text(
                Pos2::new(banner.left() + 26.0, banner.center().y),
                Align2::LEFT_CENTER,
                format!(
                    "{} 额度将在 {}后重置 · {}",
                    reset.label,
                    reset.countdown(),
                    reset.at.format("%H:%M")
                ),
                semi_font(11.2),
                pal.accent_dark,
            );
            usage_top += 38.0;
        }

        let usage_rect = Rect::from_min_size(
            Pos2::new(content.left(), usage_top),
            Vec2::new(content.width(), 150.0),
        );
        theme::card(&painter, usage_rect, 12, &pal);
        let inner = usage_rect.shrink2(Vec2::new(18.0, 16.0));

        painter.text(
            inner.left_top(),
            Align2::LEFT_TOP,
            "额度",
            semi_font(11.6),
            pal.ink,
        );

        let usage_snapshot = self.usage.clone();
        if let Some(usage_data) = &usage_snapshot {
            painter.text(
                Pos2::new(inner.left() + 32.0, inner.top() + 1.5),
                Align2::LEFT_TOP,
                format!("更新于 {}", format_when(usage_data.updated)),
                ui_font(9.4),
                pal.faint,
            );

            let refresh = Rect::from_center_size(
                Pos2::new(inner.right() - 8.0, inner.top() + 8.0),
                Vec2::splat(22.0),
            );
            let refresh_resp = ui.interact(refresh, Id::new("quota_refresh"), Sense::click());
            let rh = ui
                .ctx()
                .animate_bool_with_time(refresh_resp.id, refresh_resp.hovered(), 0.15);
            if rh > 0.01 {
                painter.rect_filled(
                    refresh,
                    CornerRadius::same(6),
                    theme::with_alpha(pal.ink, 0.05 * rh),
                );
            }
            theme::icon_refresh(
                &painter,
                refresh.shrink(6.0),
                theme::mix(pal.faint, pal.ink, rh),
            );
            if refresh_resp.clicked() {
                self.refresh_usage(true);
                self.toast("已重新读取额度。", Tone::Info);
            }
            if refresh_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }

            if let Some(reset) = next_reset(usage_data, Local::now()) {
                let imminent = reset.imminent();
                // Always a countdown: "距 5 小时重置 3 小时 20 分" reads better than an
                // absolute clock, and matches what people actually want to know.
                // When it is imminent the banner above already says so; keep the pill
                // quiet in that case to avoid saying it twice.
                let label = if imminent {
                    String::new()
                } else {
                    format!("距{}重置 {}", reset.label, reset.countdown())
                };
                if label.is_empty() {
                    // The banner above carries the message.
                } else {
                    let font = ui_font(10.0);
                    let width = measure(ui.ctx(), &label, &font);
                    let pill = Rect::from_min_size(
                        Pos2::new(inner.right() - 34.0 - width - 20.0, inner.top() - 2.0),
                        Vec2::new(width + 20.0, 20.0),
                    );
                    let (bg, fg) = if imminent {
                        (theme::with_alpha(pal.accent, 0.15), pal.accent_dark)
                    } else {
                        (theme::with_alpha(pal.ink, 0.045), pal.muted)
                    };
                    painter.rect_filled(pill, CornerRadius::same(10), bg);
                    let pulse = if imminent {
                        0.55 + 0.45 * breathe(self.time, 1.6)
                    } else {
                        1.0
                    };
                    painter.circle_filled(
                        Pos2::new(pill.left() + 9.0, pill.center().y),
                        2.6,
                        theme::with_alpha(if imminent { pal.accent } else { pal.faint }, pulse),
                    );
                    painter.text(
                        Pos2::new(pill.left() + 16.0, pill.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        font,
                        fg,
                    );
                }
            }

            let now = Local::now();
            let (fh_value, fh_hint) = usage_data.fh_row(now);
            let (sd_value, sd_hint) = usage_data.sd_row(now);
            let row_y = 34.0;
            self.draw_meter(
                ui,
                inner,
                row_y,
                "5 小时",
                fh_value,
                self.bar_fh.value,
                &fh_hint,
                pal.accent,
            );
            self.draw_meter(
                ui,
                inner,
                row_y + 54.0,
                "本周",
                sd_value,
                self.bar_sd.value,
                &sd_hint,
                pal.weekly,
            );
        } else {
            painter.text(
                Pos2::new(inner.left(), inner.top() + 42.0),
                Align2::LEFT_CENTER,
                "还没有额度记录",
                semi_font(13.0),
                pal.muted,
            );
            painter.text(
                Pos2::new(inner.left(), inner.top() + 64.0),
                Align2::LEFT_CENTER,
                "Claude 运行时会自动记录；查看额度不需要额外启动。",
                ui_font(10.2),
                pal.faint,
            );
        }

        // ---- recent conversations ----
        let recent_top = usage_rect.bottom() + 14.0;
        let recent_rect = Rect::from_min_max(
            Pos2::new(content.left(), recent_top),
            Pos2::new(content.right(), actions_top - 16.0),
        );
        if recent_rect.height() > 70.0 {
            theme::card(&painter, recent_rect, 12, &pal);
            let rinner = recent_rect.shrink2(Vec2::new(18.0, 14.0));
            painter.text(
                rinner.left_top(),
                Align2::LEFT_TOP,
                "最近对话",
                semi_font(11.6),
                pal.ink,
            );
            painter.text(
                Pos2::new(rinner.right(), rinner.top() + 1.5),
                Align2::RIGHT_TOP,
                format!("共 {} 个", self.recent_sessions.len()),
                ui_font(9.4),
                pal.faint,
            );
            let rows_top = rinner.top() + 24.0;
            let available = (recent_rect.bottom() - 12.0 - rows_top).max(0.0);
            let row_h = 32.0;
            let max_rows = (available / row_h).floor().max(0.0) as usize;
            if self.recent_sessions.is_empty() {
                painter.text(
                    Pos2::new(rinner.left(), rows_top + 14.0),
                    Align2::LEFT_CENTER,
                    "这个账号还没有 Code 对话",
                    ui_font(10.4),
                    pal.faint,
                );
            } else {
                for (index, session) in self.recent_sessions.iter().take(max_rows).enumerate() {
                    let y = rows_top + index as f32 * row_h;
                    theme::icon_chat(
                        &painter,
                        Rect::from_center_size(
                            Pos2::new(rinner.left() + 6.0, y + 8.0),
                            Vec2::splat(11.0),
                        ),
                        pal.faint,
                    );
                    let title = theme::truncate(
                        ui.ctx(),
                        &session.title,
                        &ui_font(10.8),
                        (rinner.width() - 210.0).max(120.0),
                    );
                    painter.text(
                        Pos2::new(rinner.left() + 18.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        title,
                        ui_font(10.8),
                        pal.ink,
                    );
                    let folder = theme::truncate(ui.ctx(), &session.folder, &ui_font(9.4), 92.0);
                    painter.text(
                        Pos2::new(rinner.right() - 96.0, y + 8.0),
                        Align2::RIGHT_CENTER,
                        folder,
                        ui_font(9.4),
                        pal.faint,
                    );
                    painter.text(
                        Pos2::new(rinner.right(), y + 8.0),
                        Align2::RIGHT_CENTER,
                        format_when(system_time_to_local(session.updated)),
                        ui_font(9.4),
                        pal.faint,
                    );
                    if index + 1 < max_rows && index + 1 < self.recent_sessions.len() {
                        painter.line_segment(
                            [
                                Pos2::new(rinner.left(), y + 17.0),
                                Pos2::new(rinner.right(), y + 17.0),
                            ],
                            Stroke::new(1.0, pal.hairline),
                        );
                    }
                }
            }
        }

        // ---- actions ----
        let button_y = actions_top;
        let primary =
            Rect::from_min_size(Pos2::new(content.left(), button_y), Vec2::new(156.0, 40.0));
        let label = if running {
            "聚焦 Claude"
        } else {
            "打开 Claude"
        };
        let open = button_at(
            ui,
            primary,
            label,
            Btn::Primary,
            Some(if running {
                theme::icon_focus as IconFn
            } else {
                theme::icon_arrow_right as IconFn
            }),
            true,
            &pal,
        );
        if open.clicked() {
            self.launch(&account.id, ui.ctx());
        }
        if open.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let _ = open.on_hover_text("打开或聚焦该账号（Enter）");

        let transfer = Rect::from_min_size(
            Pos2::new(primary.right() + 10.0, button_y),
            Vec2::new(112.0, 40.0),
        );
        let transfer = button_at(
            ui,
            transfer,
            "转移对话",
            Btn::Secondary,
            Some(theme::icon_transfer),
            true,
            &pal,
        );
        if transfer.clicked() {
            self.open_transfer(&account.id);
        }
        if transfer.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        // Secondary actions keep a visible outline so they read as clickable next to the
        // filled primary button instead of fading into the canvas.
        let rename = Rect::from_min_size(
            Pos2::new(primary.right() + 10.0 + 112.0 + 8.0, button_y),
            Vec2::new(96.0, 40.0),
        );
        let rename = button_at(
            ui,
            rename,
            "重命名",
            Btn::Secondary,
            Some(theme::icon_pencil as IconFn),
            true,
            &pal,
        );
        if rename.clicked() {
            self.dialog = super::Dialog::Rename {
                id: account.id.clone(),
                buf: account.name.clone(),
                focus: false,
                error: None,
            };
        }
        if rename.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let folder = Rect::from_min_size(
            Pos2::new(primary.right() + 10.0 + 112.0 + 8.0 + 96.0 + 8.0, button_y),
            Vec2::new(96.0, 40.0),
        );
        let folder = button_at(
            ui,
            folder,
            "文件夹",
            Btn::Secondary,
            Some(theme::icon_folder as IconFn),
            true,
            &pal,
        );
        if folder.clicked() {
            self.open_folder(&account.id);
        }
        if folder.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        painter.text(
            Pos2::new(content.right(), button_y + 20.0),
            Align2::RIGHT_CENTER,
            format!("Claude {}", self.install.version),
            ui_font(9.4),
            theme::with_alpha(pal.faint, 0.7),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_meter(
        &self,
        ui: &Ui,
        area: Rect,
        y: f32,
        label: &str,
        value: Option<i64>,
        blend: f32,
        hint: &str,
        color: Color32,
    ) {
        let pal = self.pal;
        let painter = ui.painter();
        let center_y = area.top() + y;

        painter.text(
            Pos2::new(area.left(), center_y),
            Align2::LEFT_CENTER,
            label,
            ui_font(10.4),
            pal.muted,
        );

        let percent = value.unwrap_or(0).clamp(0, 100);
        let value_text = match value {
            Some(_) => format!("{percent}%"),
            None => "—".into(),
        };
        let value_color = if value.is_some() && percent >= 90 {
            pal.danger
        } else {
            pal.ink
        };
        painter.text(
            Pos2::new(area.left() + 52.0, center_y - 1.0),
            Align2::LEFT_CENTER,
            &value_text,
            semi_font(21.0),
            value_color,
        );

        let track_left = area.left() + 116.0;
        let track_right = area.right() - 132.0;
        let track = Rect::from_min_size(
            Pos2::new(track_left, center_y - 2.5),
            Vec2::new((track_right - track_left).max(60.0), 5.0),
        );
        painter.rect_filled(track, CornerRadius::same(2), pal.chip);
        let ratio = (blend * 100.0).clamp(0.0, 100.0) / 100.0;
        if value.is_some() && ratio > 0.002 {
            let width = (track.width() * ratio).max(6.0);
            let fill = Rect::from_min_size(track.min, Vec2::new(width, track.height()));
            painter.rect_filled(fill, CornerRadius::same(2), color);
        }

        painter.text(
            Pos2::new(area.right(), center_y),
            Align2::RIGHT_CENTER,
            hint,
            ui_font(9.8),
            if hint.starts_with("已重置") {
                pal.faint
            } else {
                pal.muted
            },
        );
    }

    // -------------------------------------------------------------- toast ----

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else {
            return;
        };
        let elapsed = toast.at.elapsed().as_secs_f32();
        let fade_in = (elapsed / 0.18).clamp(0.0, 1.0);
        let fade_out = ((4.6 - elapsed) / 0.40).clamp(0.0, 1.0);
        let alpha = fade_in.min(fade_out);
        if alpha <= 0.0 {
            return;
        }
        let text = toast.text.clone();
        let tone = tone_color(toast.tone, &self.pal);
        let pal = self.pal;
        let offset = Vec2::new(-18.0 + (1.0 - fade_in) * 10.0, 52.0);
        egui::Area::new(Id::new("toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_TOP, offset)
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(theme::with_alpha(pal.surface, alpha))
                    .corner_radius(9)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 20,
                        spread: 0,
                        color: Color32::from_black_alpha((46.0 * alpha) as u8),
                    })
                    .inner_margin(Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.set_max_width(320.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let (dot, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
                            ui.painter().circle_filled(
                                dot.center(),
                                2.8,
                                theme::with_alpha(tone, alpha),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .font(ui_font(11.4))
                                        .color(theme::with_alpha(pal.ink, alpha)),
                                )
                                .wrap(),
                            );
                        });
                    });
            });
    }

    // --------------------------------------------------------------- menu ----

    fn draw_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = &self.menu else {
            return;
        };
        let kind = menu.kind;
        let id = menu.account_id.clone();
        let pos = menu.pos;
        let opened = menu.at.elapsed().as_millis() > 120;
        let pal = self.pal;

        let items: Vec<MenuItem> = match kind {
            MenuKind::Row => {
                let account = self.account(&id).cloned();
                let Some(account) = account else {
                    self.menu = None;
                    return;
                };
                vec![
                    MenuItem {
                        label: if account.running_pid.is_some() {
                            "切换过去".to_string()
                        } else {
                            "启动".to_string()
                        },
                        shortcut: Some("Enter".into()),
                        enabled: true,
                        danger: false,
                        checked: false,
                        bold: true,
                        sep_before: false,
                        action: UiAction::Launch(account.id.clone()),
                    },
                    MenuItem {
                        label: "重命名…".into(),
                        shortcut: Some("F2".into()),
                        enabled: true,
                        danger: false,
                        checked: false,
                        bold: false,
                        sep_before: false,
                        action: UiAction::Rename(account.id.clone()),
                    },
                    MenuItem {
                        label: "更换头像…".into(),
                        shortcut: None,
                        enabled: true,
                        danger: false,
                        checked: false,
                        bold: false,
                        sep_before: false,
                        action: UiAction::SetAvatar(account.id.clone()),
                    },
                    MenuItem {
                        label: "转移 Code 对话…".into(),
                        shortcut: None,
                        enabled: true,
                        danger: false,
                        checked: false,
                        bold: false,
                        sep_before: false,
                        action: UiAction::Transfer(account.id.clone()),
                    },
                    MenuItem {
                        label: "打开数据文件夹".into(),
                        shortcut: None,
                        enabled: true,
                        danger: false,
                        checked: false,
                        bold: false,
                        sep_before: false,
                        action: UiAction::OpenFolder(account.id.clone()),
                    },
                    MenuItem {
                        label: "删除账号".into(),
                        shortcut: Some("Del".into()),
                        enabled: !account.is_default,
                        danger: true,
                        checked: false,
                        bold: false,
                        sep_before: true,
                        action: UiAction::Delete(account.id.clone()),
                    },
                ]
            }
            MenuKind::More => vec![
                MenuItem {
                    label: if self.pal.is_dark() {
                        "切换到浅色".into()
                    } else {
                        "切换到深色".into()
                    },
                    shortcut: None,
                    enabled: true,
                    danger: false,
                    checked: false,
                    bold: false,
                    sep_before: false,
                    action: UiAction::ToggleTheme,
                },
                MenuItem {
                    label: "开机自动启动".into(),
                    shortcut: None,
                    enabled: true,
                    danger: false,
                    checked: self.startup,
                    bold: false,
                    sep_before: false,
                    action: UiAction::ToggleStartup,
                },
                MenuItem {
                    label: "打开设置文件".into(),
                    shortcut: None,
                    enabled: true,
                    danger: false,
                    checked: false,
                    bold: false,
                    sep_before: true,
                    action: UiAction::OpenSettings,
                },
                MenuItem {
                    label: "打开数据文件夹".into(),
                    shortcut: None,
                    enabled: self.selected().is_some(),
                    danger: false,
                    checked: false,
                    bold: false,
                    sep_before: false,
                    action: UiAction::OpenFolder(self.selected_id.clone()),
                },
                MenuItem {
                    label: "关于".into(),
                    shortcut: None,
                    enabled: true,
                    danger: false,
                    checked: false,
                    bold: false,
                    sep_before: true,
                    action: UiAction::About,
                },
            ],
        };

        let width = 212.0;
        let height = menu_height(&items);
        let x = pos.x.min(WINDOW_W - width - 12.0).max(12.0);
        let y = pos.y.min(WINDOW_H - height - 12.0).max(TITLE_H + 4.0);

        let mut action: Option<UiAction> = None;
        let mut close = false;
        let response = egui::Area::new(Id::new("menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(x, y))
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(pal.surface)
                    .corner_radius(9)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::same(5))
                    .show(ui, |ui| {
                        ui.set_width(width - 10.0);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for item in &items {
                            if item.sep_before {
                                let (sep, _) = ui.allocate_exact_size(
                                    Vec2::new(width - 10.0, 8.0),
                                    Sense::hover(),
                                );
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(sep.left() + 8.0, sep.center().y),
                                        Pos2::new(sep.right() - 8.0, sep.center().y),
                                    ],
                                    Stroke::new(1.0, pal.hairline),
                                );
                            }
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(width - 10.0, 28.0),
                                if item.enabled {
                                    Sense::click()
                                } else {
                                    Sense::hover()
                                },
                            );
                            let hover = if item.enabled {
                                ui.ctx()
                                    .animate_bool_with_time(resp.id, resp.hovered(), 0.12)
                            } else {
                                0.0
                            };
                            if hover > 0.01 {
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(5),
                                    theme::with_alpha(pal.ink, 0.06 * hover),
                                );
                            }
                            let color = if !item.enabled {
                                pal.faint
                            } else if item.danger {
                                pal.danger
                            } else {
                                pal.ink
                            };
                            ui.painter().text(
                                Pos2::new(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &item.label,
                                if item.bold {
                                    semi_font(11.4)
                                } else {
                                    ui_font(11.4)
                                },
                                color,
                            );
                            let mut reserved = 0.0;
                            if let Some(shortcut) = &item.shortcut {
                                ui.painter().text(
                                    Pos2::new(rect.right() - 8.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    shortcut,
                                    ui_font(9.6),
                                    pal.faint,
                                );
                                reserved += 4.0;
                            }
                            if item.checked {
                                theme::icon_check(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(rect.right() - 10.0 - reserved, rect.center().y),
                                        Vec2::splat(12.0),
                                    ),
                                    pal.accent_dark,
                                );
                            }
                            if resp.hovered() && item.enabled {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }
                            if resp.clicked() && item.enabled {
                                action = Some(item.action.clone());
                                close = true;
                            }
                        }
                    });
            });

        if action.is_some()
            || close
            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
            || (opened
                && ctx.input(|i| i.pointer.any_click())
                && ctx
                    .input(|i| i.pointer.interact_pos())
                    .is_some_and(|p| !response.response.rect.contains(p)))
        {
            self.menu = None;
        }

        if let Some(action) = action {
            self.apply_action(action, ctx);
        }
    }
}

// ---------------------------------------------------------------- helpers ----

fn menu_reserved_height() -> f32 {
    10.0 + 7.0 * 28.0 + 3.0 * 9.0
}

fn context_surface(pal: Palette) -> Color32 {
    if pal.is_dark() {
        Color32::from_rgb(46, 46, 42)
    } else {
        pal.surface
    }
}

#[derive(Clone)]
struct MenuItem {
    label: String,
    shortcut: Option<String>,
    enabled: bool,
    danger: bool,
    checked: bool,
    bold: bool,
    sep_before: bool,
    action: UiAction,
}

fn menu_height(items: &[MenuItem]) -> f32 {
    let mut h = 10.0;
    for item in items {
        if item.sep_before {
            h += 8.0 + 1.0;
        }
        h += 28.0;
    }
    h
}

fn measure(ctx: &egui::Context, text: &str, font: &FontId) -> f32 {
    ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE))
        .size()
        .x
}

fn badge_letter(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .or_else(|| name.chars().next())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into())
}

fn title_button(ui: &mut Ui, rect: Rect, icon: IconFn, pal: &Palette) -> bool {
    let response = ui.interact(
        rect,
        Id::new(("title_btn", rect.left() as i32)),
        Sense::click(),
    );
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    if hover > 0.01 {
        ui.painter().rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(6),
            theme::with_alpha(pal.ink, 0.07 * hover),
        );
    }
    icon(
        ui.painter(),
        rect.shrink(8.0),
        theme::mix(pal.faint, pal.ink, hover),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.clicked()
}

fn icon_button(ui: &mut Ui, rect: Rect, icon: IconFn, pal: &Palette) -> Response {
    let response = ui.interact(
        rect,
        Id::new(("icon_btn", rect.left() as i32, rect.top() as i32)),
        Sense::click(),
    );
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    if hover > 0.01 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            theme::with_alpha(pal.ink, 0.06 * hover),
        );
    }
    icon(
        ui.painter(),
        rect.shrink(7.0),
        theme::mix(pal.muted, pal.ink, hover),
    );
    response
}

fn make_footer_button(
    ui: &mut Ui,
    footer: Rect,
    label: &str,
    icon: IconFn,
    pal: &Palette,
) -> Response {
    let rect = Rect::from_min_size(
        Pos2::new(footer.right() - 76.0, footer.top() + 10.0),
        Vec2::new(76.0, 32.0),
    );
    let response = ui.interact(rect, Id::new(("footer_btn", label)), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    if hover > 0.01 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(7),
            theme::with_alpha(pal.ink, 0.055 * hover),
        );
    }
    let color = theme::mix(pal.muted, pal.ink, hover);
    icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 14.0, rect.center().y),
            Vec2::splat(13.0),
        ),
        color,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 25.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        ui_font(10.2),
        color,
    );
    response
}

pub(super) fn button(
    ui: &mut Ui,
    size: Vec2,
    label: &str,
    kind: Btn,
    icon: Option<IconFn>,
    enabled: bool,
    pal: &Palette,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    button_at(ui, rect, label, kind, icon, enabled, pal)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn button_at(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    kind: Btn,
    icon: Option<IconFn>,
    enabled: bool,
    pal: &Palette,
) -> Response {
    let id = Id::new((
        "button",
        label,
        rect.left() as i32,
        rect.top() as i32,
        kind as u8,
    ));
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hover = if enabled {
        ui.ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered(), 0.15)
    } else {
        0.0
    };
    let pressed = response.is_pointer_button_down_on() && enabled;
    let painter = ui.painter();
    let draw_rect = if pressed { rect.shrink(0.5) } else { rect };

    let (fill, text_color, stroke) = match (kind, enabled) {
        (_, false) => (pal.chip, pal.faint, Stroke::NONE),
        (Btn::Primary, true) => (
            if pressed {
                theme::shade(pal.accent_dark, 0.06)
            } else {
                theme::mix(pal.accent, pal.accent_dark, hover)
            },
            Color32::WHITE,
            Stroke::NONE,
        ),
        (Btn::Danger, true) => (
            if pressed {
                theme::shade(pal.danger, 0.06)
            } else {
                theme::mix(pal.danger, theme::shade(pal.danger, 0.18), hover)
            },
            if pal.is_dark() {
                Color32::from_rgb(28, 20, 18)
            } else {
                Color32::WHITE
            },
            Stroke::NONE,
        ),
        (Btn::Secondary, true) => (
            theme::mix(pal.surface, pal.hover, hover),
            pal.ink,
            Stroke::new(1.0, theme::mix(pal.border, pal.border_strong, hover)),
        ),
        (Btn::Ghost, true) => (
            theme::with_alpha(pal.ink, 0.06 * hover),
            theme::mix(pal.muted, pal.ink, hover),
            Stroke::NONE,
        ),
    };

    if kind == Btn::Primary && enabled && !pal.is_dark() {
        theme::soft_shadow(painter, draw_rect, 9, 10.0, 3.0, (16.0 + 8.0 * hover) as u8);
    }
    painter.rect_filled(draw_rect, CornerRadius::same(9), fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(draw_rect, CornerRadius::same(9), stroke, StrokeKind::Inside);
    }

    let content_y = draw_rect.center().y + if pressed { 0.5 } else { 0.0 };
    let label_font = semi_font(11.8);
    let label_width = if label.is_empty() {
        0.0
    } else {
        measure(ui.ctx(), label, &label_font)
    };
    let icon_w = if icon.is_some() { 15.0 } else { 0.0 };
    let gap = if icon.is_some() && !label.is_empty() {
        7.0
    } else {
        0.0
    };
    let total = icon_w + gap + label_width;
    let start = draw_rect.center().x - total * 0.5;
    if let Some(icon) = icon {
        icon(
            painter,
            Rect::from_center_size(
                Pos2::new(start + icon_w * 0.5, content_y),
                Vec2::splat(icon_w),
            ),
            text_color,
        );
    }
    if !label.is_empty() {
        painter.text(
            Pos2::new(start + icon_w + gap, content_y),
            Align2::LEFT_CENTER,
            label,
            label_font,
            text_color,
        );
    }
    response
}
