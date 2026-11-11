//! Modal dialogs: rename, delete confirmation, short messages, and the conversation
//! transfer picker. Layout stays close to Claude's own confirmation dialogs: a serif
//! heading, quiet body text, and the primary action bottom-right.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Id, Layout, Margin, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use super::draw::{button, Btn};
use super::{Dialog, DialogAction, SwitcherApp, Tone};
use crate::data::{self, paths};
use crate::theme::{display_font, mix, ui_font, Palette};

const DIALOG_W: f32 = 420.0;

impl SwitcherApp {
    pub(super) fn draw_dialog(&mut self, ctx: &egui::Context) {
        let dialog = std::mem::replace(&mut self.dialog, Dialog::None);
        if matches!(dialog, Dialog::None) {
            return;
        }
        let width = match &dialog {
            Dialog::Transfer(draft) if !draft.confirm_running => super::transfer::WIDTH,
            _ => DIALOG_W,
        };
        let pal = self.pal;
        let fade = ctx.animate_bool_with_time(Id::new("dialog_fade"), true, 0.16);
        let frame = egui::Frame::NONE
            .fill(pal.surface)
            .corner_radius(14)
            .stroke(Stroke::new(1.0, pal.border))
            .inner_margin(Margin::same(22));

        let was_confirming = matches!(&dialog, Dialog::Transfer(t) if t.confirm_running);
        let was_reviewing = matches!(&dialog, Dialog::Transfer(t) if t.review);
        let mut submitted = None;
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
                            self.add_account(ctx, &name);
                            next = std::mem::replace(&mut self.dialog, Dialog::None);
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
                                    "这会永久删除该账号目录中的登录信息和本地对话。请先备份需要保留的内容；默认账号不能删除。",
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
                    Dialog::Transfer(mut draft) => {
                        match super::transfer::draw_picker(ui, self, &mut draft) {
                            super::transfer::Outcome::Stay => next = Dialog::Transfer(draft),
                            super::transfer::Outcome::Cancel => keep_open = false,
                            super::transfer::Outcome::Copy => submitted = Some(draft),
                        }
                    }
                }
            });

        if modal.should_close() {
            submitted = None;
            action = DialogAction::None;
            keep_open = false;
            match &mut next {
                Dialog::Transfer(t) if was_confirming => t.confirm_running = false,
                Dialog::Transfer(t) if was_reviewing => t.review = false,
                _ => next = Dialog::None,
            }
        }
        if let Some(draft) = submitted {
            self.perform_copy(draft, ctx);
            return;
        }
        self.dialog = next;
        if !keep_open {
            self.apply_dialog_action(action, ctx);
        }
    }
}

pub(super) fn dialog_heading(ui: &mut Ui, title: &str, pal: &Palette) {
    ui.label(RichText::new(title).font(display_font(21.0)).color(pal.ink));
}
