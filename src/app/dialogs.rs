//! Modal dialogs: rename, delete confirmation, short messages, and the conversation
//! transfer picker. Layout stays close to Claude's own confirmation dialogs: a serif
//! heading, quiet body text, and the primary action bottom-right.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Id, Layout, Margin, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use super::draw::{button, Btn};
use super::{Dialog, DialogAction, PendingCopy, SwitcherApp, Tone, Transfer};
use crate::data::{self, paths, Session};
use crate::theme::{self, display_font, mix, semi_font, ui_font, with_alpha, Palette};

const DIALOG_W: f32 = 420.0;
const TRANSFER_W: f32 = 800.0;

impl SwitcherApp {
    pub(super) fn draw_dialog(&mut self, ctx: &egui::Context) {
        let dialog = std::mem::replace(&mut self.dialog, Dialog::None);
        if matches!(dialog, Dialog::None) {
            return;
        }
        let width = match &dialog {
            Dialog::Transfer(_) => TRANSFER_W,
            _ => DIALOG_W,
        };
        let pal = self.pal;
        let fade = ctx.animate_bool_with_time(Id::new("dialog_fade"), true, 0.16);
        let frame = egui::Frame::NONE
            .fill(pal.surface)
            .corner_radius(14)
            .stroke(Stroke::new(1.0, pal.border))
            .inner_margin(Margin::same(22));

        let mut next = Dialog::None;
        let mut action = DialogAction::None;
        let mut keep_open = true;
        let mut commit_rename = false;
        let mut commit_delete = false;
        let mut commit_add = false;

        let modal = egui::Modal::new(Id::new("switcher_modal"))
            // Light scrim: enough to focus the dialog, not enough to grey out the
            // sidebar behind it. Claude's own modals read as a soft veil.
            .backdrop_color(Color32::from_rgba_unmultiplied(
                28,
                26,
                24,
                (26.0 * fade) as u8,
            ))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 10.0);

                match dialog {
                    Dialog::None => {}
                    Dialog::Rename {
                        id,
                        mut buf,
                        mut focus,
                        error,
                    } => {
                        dialog_heading(ui, "重命名账号", &pal);
                        ui.add(
                            egui::Label::new(
                                RichText::new("只改显示名称，登录状态、目录和对话都不受影响。")
                                    .font(ui_font(11.8))
                                    .color(pal.muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(2.0);
                        let field = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(width, 34.0),
                        );
                        ui.allocate_rect(field, Sense::hover());
                        let hovered = ui.rect_contains_pointer(field);
                        ui.painter().rect_filled(
                            field,
                            CornerRadius::same(8),
                            mix(pal.surface, pal.hover, if hovered { 1.0 } else { 0.35 }),
                        );
                        ui.painter().rect_stroke(
                            field,
                            CornerRadius::same(8),
                            Stroke::new(
                                1.0,
                                if error.is_some() {
                                    pal.danger
                                } else if hovered {
                                    pal.border_strong
                                } else {
                                    pal.border
                                },
                            ),
                            StrokeKind::Inside,
                        );
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(field.shrink2(Vec2::new(10.0, 6.0))),
                        );
                        let response = child.add_sized(
                            Vec2::new(field.width() - 20.0, 22.0),
                            egui::TextEdit::singleline(&mut buf)
                                .font(ui_font(13.0))
                                .frame(false)
                                // Keep Enter out of the widget's own handling: the dialog
                                // decides what Enter means, so an IME conversion does not
                                // blur the field the user is still typing in.
                                .return_key(None)
                                .hint_text("例如：工作账号"),
                        );
                        if !focus {
                            response.request_focus();
                            focus = true;
                        }
                        if let Some(message) = &error {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(message).font(ui_font(11.2)).color(pal.danger),
                                )
                                .wrap(),
                            );
                        }
                        // Enter confirms, unless a Windows IME just handled it. Three
                        // signals work together: an IME event in the same frame, a
                        // composition seen on the previous frame (the buffer is cleared
                        // before the confirming Enter reaches egui), and a commit within
                        // the last 200 ms.
                        let ime_event = ui.input(|i| {
                            i.events
                                .iter()
                                .any(|event| matches!(event, egui::Event::Ime(_)))
                        });
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter && !ime_event && !self.ime_recently_active() {
                            commit_rename = true;
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(
                                    ui,
                                    Vec2::new(92.0, 36.0),
                                    "重命名",
                                    Btn::Primary,
                                    None,
                                    true, &pal)
                                .clicked()
                                {
                                    commit_rename = true;
                                }
                                ui.add_space(2.0);
                                if button(
                                    ui,
                                    Vec2::new(78.0, 36.0),
                                    "取消",
                                    Btn::Ghost,
                                    None,
                                    true, &pal)
                                .clicked()
                                {
                                    keep_open = false;
                                    next = Dialog::None;
                                }
                            });
                        });
                        if commit_rename {
                            keep_open = false;
                            match self.rename_account(&id, &buf, ctx) {
                                Ok(()) => next = Dialog::None,
                                Err(message) => {
                                    next = Dialog::Rename {
                                        id: id.clone(),
                                        buf: buf.clone(),
                                        focus: true,
                                        error: Some(message),
                                    };
                                }
                            }
                        } else if keep_open {
                            next = Dialog::Rename {
                                id,
                                buf,
                                focus,
                                error,
                            };
                        }
                    }
                    Dialog::AddAccount { name } => {
                        dialog_heading(ui, "添加账号", &pal);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "将在独立目录中创建 '{name}'，并打开 Claude 的登录窗口。"
                                ))
                                .font(ui_font(12.6))
                                .color(pal.ink),
                            )
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "需要在新窗口里登录另一个账号。已有账号的登录状态和对话都不受影响。",
                                )
                                .font(ui_font(11.8))
                                .color(pal.muted),
                            )
                            .wrap(),
                        );
                        let folder = paths().local.join(format!("{}{}", data::PROFILE_PREFIX, name));
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("目录：{}", folder.display()))
                                    .font(ui_font(10.2))
                                    .color(pal.faint),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(
                                    ui,
                                    Vec2::new(104.0, 36.0),
                                    "添加并登录",
                                    Btn::Primary,
                                    None,
                                    true,
                                    &pal,
                                )
                                .clicked()
                                {
                                    commit_add = true;
                                }
                                ui.add_space(2.0);
                                if button(
                                    ui,
                                    Vec2::new(78.0, 36.0),
                                    "取消",
                                    Btn::Ghost,
                                    None,
                                    true,
                                    &pal,
                                )
                                .clicked()
                                {
                                    keep_open = false;
                                    next = Dialog::None;
                                }
                            });
                        });
                        if commit_add {
                            keep_open = false;
                            next = Dialog::None;
                            self.add_account(ctx, &name);
                        } else if keep_open {
                            next = Dialog::AddAccount { name };
                        }
                    }
                    Dialog::Delete { id } => {
                        let name = self
                            .account(&id)
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| id.clone());
                        dialog_heading(ui, "删除账号", &pal);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("删除 '{name}' 并移除它在本机的登录？"))
                                    .font(ui_font(12.6))
                                    .color(pal.ink),
                            )
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "它的目录、保存的登录和快捷方式都会被移除，其他账号不受影响。",
                                )
                                .font(ui_font(11.8))
                                .color(pal.muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(
                                    ui,
                                    Vec2::new(92.0, 36.0),
                                    "删除",
                                    Btn::Danger,
                                    None,
                                    true, &pal)
                                .clicked()
                                {
                                    commit_delete = true;
                                }
                                ui.add_space(2.0);
                                if button(
                                    ui,
                                    Vec2::new(78.0, 36.0),
                                    "取消",
                                    Btn::Ghost,
                                    None,
                                    true, &pal)
                                .clicked()
                                {
                                    keep_open = false;
                                    next = Dialog::None;
                                }
                            });
                        });
                        if commit_delete {
                            keep_open = false;
                            self.delete_account(&id, ctx);
                            next = std::mem::replace(&mut self.dialog, Dialog::None);
                        } else if keep_open {
                            next = Dialog::Delete { id };
                        }
                    }
                    Dialog::Message {
                        title,
                        body,
                        tone,
                        primary,
                        secondary,
                        action: pending,
                    } => {
                        dialog_heading(ui, &title, &pal);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&body).font(ui_font(12.2)).color(pal.muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let primary_kind = if tone == Tone::Error {
                                    Btn::Danger
                                } else {
                                    Btn::Primary
                                };
                                if button(
                                    ui,
                                    Vec2::new(104.0, 36.0),
                                    &primary,
                                    primary_kind,
                                    None,
                                    true,
                                    &pal,
                                )
                                .clicked()
                                {
                                    action = pending.clone();
                                    keep_open = false;
                                    next = Dialog::None;
                                }
                                if let Some(secondary) = &secondary {
                                    ui.add_space(2.0);
                                    if button(
                                        ui,
                                        Vec2::new(78.0, 36.0),
                                        secondary,
                                        Btn::Ghost,
                                        None,
                                        true,
                                        &pal,
                                    )
                                    .clicked()
                                    {
                                        keep_open = false;
                                        if matches!(pending, DialogAction::PerformCopy) {
                                            self.pending_copy = None;
                                        }
                                        next = Dialog::None;
                                    }
                                }
                            });
                        });
                        if keep_open {
                            next = Dialog::Message {
                                title,
                                body,
                                tone,
                                primary,
                                secondary,
                                action: pending,
                            };
                        }
                    }
                    Dialog::Transfer(mut transfer) => {
                        let source_name = self
                            .account(&transfer.source_id)
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| transfer.source_id.clone());
                        let targets: Vec<(String, String)> = self
                            .accounts
                            .iter()
                            .filter(|a| a.id != transfer.source_id)
                            .map(|a| (a.id.clone(), a.name.clone()))
                            .collect();
                        let total = transfer.sessions.len();
                        let picked: usize = transfer.checked.iter().filter(|v| **v).count();

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("转移 Code 对话")
                                    .font(display_font(21.0))
                                    .color(pal.ink),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let all = total > 0 && picked == total;
                                let partial = picked > 0 && !all;
                                let (all_rect, response) =
                                    ui.allocate_exact_size(Vec2::new(64.0, 24.0), Sense::click());
                                if response.clicked() {
                                    let value = !all;
                                    for checked in &mut transfer.checked {
                                        *checked = value;
                                    }
                                }
                                let box_rect = Rect::from_center_size(
                                    Pos2::new(all_rect.left() + 8.0, all_rect.center().y),
                                    Vec2::splat(16.0),
                                );
                                check_box(
                                    ui,
                                    box_rect,
                                    all,
                                    partial,
                                    ui.ctx().animate_bool_with_time(
                                        Id::new("transfer_all_anim"),
                                        all,
                                        0.14,
                                    ),
                                    &pal,
                                );
                                ui.painter().text(
                                    Pos2::new(box_rect.right() + 8.0, box_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    "全选",
                                    ui_font(11.4),
                                    pal.muted,
                                );
                                if response.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                }
                            });
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "'{source_name}' 有 {total} 个 Claude Code 对话，勾选要复制到别的账号的那些。"
                                ))
                                .font(ui_font(11.8))
                                .color(pal.muted),
                            )
                            .wrap(),
                        );

                        // A fixed list viewport keeps the dialog height predictable and
                        // lets the rows scroll without ever crossing the footer.
                        let list_h = 300.0;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(width, list_h),
                            Sense::hover(),
                        );
                        let header = Rect::from_min_size(
                            list_rect.min,
                            Vec2::new(width, 24.0),
                        );
                        let painter = ui.painter();
                        painter.text(
                            Pos2::new(header.left() + 34.0, header.center().y),
                            Align2::LEFT_CENTER,
                            "对话",
                            ui_font(10.4),
                            pal.faint,
                        );
                        painter.text(
                            Pos2::new(header.right() - 214.0, header.center().y),
                            Align2::LEFT_CENTER,
                            "文件夹",
                            ui_font(10.4),
                            pal.faint,
                        );
                        painter.text(
                            Pos2::new(header.right() - 4.0, header.center().y),
                            Align2::RIGHT_CENTER,
                            "更新时间",
                            ui_font(10.4),
                            pal.faint,
                        );
                        painter.line_segment(
                            [
                                Pos2::new(header.left(), header.bottom() - 0.5),
                                Pos2::new(header.right(), header.bottom() - 0.5),
                            ],
                            Stroke::new(1.0, pal.hairline),
                        );

                        let body = Rect::from_min_max(
                            Pos2::new(list_rect.left(), header.bottom()),
                            list_rect.max,
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(body),
                            |ui| {
                                ui.set_clip_rect(body);
                                ui.set_width(width);
                                ui.set_max_height(body.height());
                                egui::ScrollArea::vertical()
                            .id_salt("transfer_list")
                            .max_height(body.height())
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                let count = transfer.sessions.len();
                                for index in 0..count {
                                    let session = transfer.sessions[index].clone();
                                    let (rect, response) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), 40.0),
                                        Sense::click(),
                                    );
                                    let response =
                                        response.on_hover_cursor(CursorIcon::PointingHand);
                                    let checked = transfer.checked[index];
                                    let hover = ui.ctx().animate_bool_with_time(
                                        response.id,
                                        response.hovered(),
                                        0.12,
                                    );
                                    if hover > 0.01 || checked {
                                        ui.painter().rect_filled(
                                            rect.shrink2(Vec2::new(2.0, 1.0)),
                                            CornerRadius::same(6),
                                            if checked {
                                                pal.accent_soft
                                            } else {
                                                with_alpha(pal.ink, 0.04 * hover)
                                            },
                                        );
                                    }
                                    let box_rect = Rect::from_center_size(
                                        Pos2::new(rect.left() + 14.0, rect.center().y),
                                        Vec2::splat(16.0),
                                    );
                                    check_box(
                                        ui,
                                        box_rect,
                                        checked,
                                        false,
                                        ui.ctx().animate_bool_with_time(
                                            response.id.with("check"),
                                            checked,
                                            0.14,
                                        ),
                                        &pal,
                                    );

                                    let text_left = rect.left() + 34.0;
                                    let folder_left = rect.right() - 214.0;
                                    let time_left = rect.right() - 132.0;
                                    let title_max =
                                        (folder_left - text_left - 12.0).max(80.0);
                                    let title = theme::truncate(
                                        ui.ctx(),
                                        &session.title,
                                        &ui_font(12.2),
                                        title_max,
                                    );
                                    ui.painter().text(
                                        Pos2::new(text_left, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        &title,
                                        ui_font(12.2),
                                        pal.ink,
                                    );
                                    if session.archived {
                                        let width = measure(
                                            ui.ctx(),
                                            &title,
                                            &ui_font(12.2),
                                        );
                                        ui.painter().text(
                                            Pos2::new(
                                                text_left
                                                    + width.min(title_max - 40.0)
                                                    + 8.0,
                                                rect.center().y,
                                            ),
                                            Align2::LEFT_CENTER,
                                            "· 已归档",
                                            ui_font(10.2),
                                            pal.faint,
                                        );
                                    }
                                    ui.painter().text(
                                        Pos2::new(folder_left, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        theme::truncate(
                                            ui.ctx(),
                                            &session.folder,
                                            &ui_font(10.4),
                                            (time_left - folder_left - 8.0).max(40.0),
                                        ),
                                        ui_font(10.4),
                                        pal.muted,
                                    );
                                    ui.painter().text(
                                        Pos2::new(rect.right() - 4.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        data::format_when(data::system_time_to_local(
                                            session.updated,
                                        )),
                                        ui_font(10.4),
                                        pal.faint,
                                    );
                                    if response.clicked() {
                                        transfer.checked[index] = !checked;
                                    }
                                    ui.painter().line_segment(
                                        [
                                            Pos2::new(rect.left() + 34.0, rect.bottom() - 0.5),
                                            Pos2::new(rect.right(), rect.bottom() - 0.5),
                                        ],
                                        Stroke::new(1.0, pal.hairline),
                                    );
                                }
                            });
                            },
                        );

                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("复制到")
                                    .font(ui_font(11.8))
                                    .color(pal.muted),
                            );
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (id, name) in &targets {
                                let selected = transfer.target_id == *id;
                                let font = if selected {
                                    semi_font(11.8)
                                } else {
                                    ui_font(11.8)
                                };
                                let text_w = measure(ui.ctx(), name, &font);
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new((text_w + 28.0).max(78.0), 30.0),
                                    Sense::click(),
                                );
                                let hover = ui.ctx().animate_bool_with_time(
                                    response.id,
                                    response.hovered(),
                                    0.12,
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(6),
                                    if selected {
                                        pal.accent_soft
                                    } else {
                                        with_alpha(pal.ink, 0.045 * hover)
                                    },
                                );
                                if selected {
                                    ui.painter().rect_stroke(
                                        rect,
                                        CornerRadius::same(6),
                                        Stroke::new(1.0, with_alpha(pal.accent, 0.45)),
                                        StrokeKind::Inside,
                                    );
                                }
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    name,
                                    font,
                                    if selected { pal.accent_dark } else { pal.muted },
                                );
                                if response.clicked() {
                                    transfer.target_id = id.clone();
                                }
                                if response.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                }
                            }
                        });

                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "对话在原账号也会保留一份。两边续写的是同一份历史，请先在一个账号里结束，再到另一个账号继续。",
                                )
                                .font(ui_font(10.4))
                                .color(pal.faint),
                            )
                            .wrap(),
                        );

                        ui.add_space(2.0);
                        let mut copy_clicked = false;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("已选 {picked} / {total}"))
                                    .font(ui_font(10.8))
                                    .color(pal.faint),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(
                                    ui,
                                    Vec2::new(108.0, 36.0),
                                    "复制对话",
                                    Btn::Primary,
                                    None,
                                    picked > 0, &pal)
                                .clicked()
                                {
                                    copy_clicked = true;
                                }
                                ui.add_space(2.0);
                                if button(
                                    ui,
                                    Vec2::new(78.0, 36.0),
                                    "取消",
                                    Btn::Ghost,
                                    None,
                                    true, &pal)
                                .clicked()
                                {
                                    keep_open = false;
                                    next = Dialog::None;
                                }
                            });
                        });

                        if copy_clicked {
                            keep_open = false;
                            let picked_sessions: Vec<Session> = transfer
                                .sessions
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| transfer.checked[*i])
                                .map(|(_, s)| s.clone())
                                .collect();
                            if let Some(target) = self.account(&transfer.target_id).cloned() {
                                self.pending_copy = Some(PendingCopy {
                                    target_id: target.id.clone(),
                                    sessions: picked_sessions,
                                });
                                if target.running_pid.is_some() {
                                    next = Dialog::Message {
                                        title: "账号正在运行".into(),
                                        body: format!(
                                            "'{}' 正在运行，复制来的对话可能要退出并重新打开后才会显示。仍然复制吗？",
                                            target.name
                                        ),
                                        tone: Tone::Warning,
                                        primary: "仍然复制".into(),
                                        secondary: Some("取消".into()),
                                        action: DialogAction::PerformCopy,
                                    };
                                } else {
                                    next = Dialog::None;
                                    action = DialogAction::PerformCopy;
                                }
                            }
                        } else if keep_open {
                            next = Dialog::Transfer(Box::new(Transfer {
                                source_id: transfer.source_id,
                                target_id: transfer.target_id,
                                sessions: transfer.sessions,
                                checked: transfer.checked,
                                hover: transfer.hover,
                            }));
                        }
                    }
                }
            });

        if modal.should_close() {
            // Backdrop click or Esc always dismisses. A branch that produced a
            // follow-up dialog only does so from an explicit button press, so it is not
            // affected by this.
            keep_open = false;
            next = Dialog::None;
        }
        self.dialog = next;
        if !keep_open {
            self.apply_dialog_action(action, ctx);
        }
    }
}

fn dialog_heading(ui: &mut Ui, title: &str, pal: &Palette) {
    ui.label(
        RichText::new(title)
            .font(display_font(21.0))
            .color(pal.ink),
    );
}

fn check_box(
    ui: &Ui,
    rect: Rect,
    checked: bool,
    partial: bool,
    t: f32,
    pal: &Palette,
) {
    let painter = ui.painter();
    let fill = mix(pal.surface, pal.accent, t);
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, mix(pal.border_strong, pal.accent, t.max(0.0))),
        StrokeKind::Inside,
    );
    if partial && t < 0.5 {
        let line = Rect::from_center_size(rect.center(), Vec2::new(8.0, 2.0));
        painter.rect_filled(line, CornerRadius::same(1), pal.accent);
    } else if checked || t > 0.01 {
        theme::icon_check(
            painter,
            rect.shrink(1.5),
            with_alpha(Color32::WHITE, t.max(if checked { 1.0 } else { 0.0 })),
        );
    }
}

fn measure(ctx: &egui::Context, text: &str, font: &egui::FontId) -> f32 {
    ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE))
        .size()
        .x
}
