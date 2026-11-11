//! Conversation picker. The draft owns its selection until the copy actually starts.
use super::dialogs::dialog_heading;
use super::draw::{button, search_field, Btn};
use super::{SwitcherApp, Transfer};
use crate::data;
use crate::theme::{self, mix, ui_font, with_alpha, Palette};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Id, Layout, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Ui, Vec2,
};

pub(super) const WIDTH: f32 = 800.0;
pub(super) enum Outcome {
    Stay,
    Cancel,
    Copy,
}

pub(super) fn draw_picker(ui: &mut Ui, app: &SwitcherApp, transfer: &mut Transfer) -> Outcome {
    let pal = app.pal;
    let width = WIDTH;
    if transfer.confirm_running {
        let name = app
            .account(&transfer.target_id)
            .map_or(transfer.target_id.as_str(), |a| &a.name);
        let count = transfer.checked.iter().filter(|&&v| v).count();
        dialog_heading(ui, "账号正在运行", &pal);
        ui.label(format!(
            "要把 {count} 个对话复制到「{name}」。这个账号正在运行，可能要重新打开后才能看到副本。"
        ));
        let mut result = Outcome::Stay;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if button(
                ui,
                Vec2::new(104.0, 36.0),
                "仍然复制",
                Btn::Primary,
                None,
                true,
                &pal,
            )
            .clicked()
            {
                result = Outcome::Copy;
            }
            if button(
                ui,
                Vec2::new(94.0, 36.0),
                "返回选择",
                Btn::Ghost,
                None,
                true,
                &pal,
            )
            .clicked()
            {
                transfer.confirm_running = false;
            }
        });
        return result;
    }
    let mut return_to_main = false;
    let source_name = app
        .account(&transfer.source_id)
        .map(|a| a.name.clone())
        .unwrap_or_else(|| transfer.source_id.clone());
    let total = transfer.sessions.len();
    dialog_heading(ui, "转移 Code 对话", &pal);
    ui.label(
        RichText::new(format!("从「{source_name}」选择要复制的对话"))
            .font(ui_font(11.8))
            .color(pal.muted),
    );

    let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::hover());
    let picked_before = transfer.checked.iter().filter(|&&v| v).count();
    let review_rect = Rect::from_min_max(Pos2::new(bar.right() - 94.0, bar.top()), bar.max);
    let label = if transfer.review {
        "返回搜索"
    } else {
        "查看已选"
    };
    if super::draw::button_at(
        ui,
        review_rect,
        label,
        Btn::Secondary,
        None,
        transfer.review || picked_before > 0,
        &pal,
    )
    .clicked()
    {
        transfer.review = !transfer.review;
    }
    let search_id = Id::new("session_search");
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
        transfer.review = false;
        ui.memory_mut(|m| m.request_focus(search_id));
    }
    let mut query_changed = false;
    let rows: std::borrow::Cow<'_, [usize]> = if transfer.review {
        ui.painter().text(
            bar.left_center(),
            Align2::LEFT_CENTER,
            "已选清单 · 点击对话可移出清单",
            ui_font(12.0),
            pal.muted,
        );
        transfer
            .checked
            .iter()
            .enumerate()
            .filter_map(|(i, &on)| on.then_some(i))
            .collect::<Vec<_>>()
            .into()
    } else {
        let mut field = bar;
        field.max.x = review_rect.left() - 132.0;
        let response = search_field(
            ui,
            field,
            search_id,
            &mut transfer.filter.query,
            "搜索对话标题或文件夹",
            &pal,
        );
        query_changed = response.changed();
        if query_changed {
            transfer.filter.refresh();
        }
        let shown = transfer.filter.visible.len();
        let selected_here = transfer.filter.checked_count(&transfer.checked);
        let mut all = shown > 0 && selected_here == shown;
        let toggle = egui::Checkbox::new(&mut all, "全选当前结果")
            .indeterminate(selected_here > 0 && selected_here < shown);
        let toggle_rect = Rect::from_min_max(
            Pos2::new(field.right() + 8.0, bar.top()),
            Pos2::new(review_rect.left() - 8.0, bar.bottom()),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(toggle_rect), |ui| {
            ui.add_enabled_ui(shown > 0, |ui| {
                if ui.put(toggle_rect, toggle).changed() {
                    transfer.filter.toggle_visible(&mut transfer.checked);
                }
            });
        });
        transfer.filter.visible.as_slice().into()
    };
    let shown = rows.len();

    // Leave room for the destination and the copy confirmation at 1000 × 624.
    let list_h = 270.0;
    let (list_rect, _) = ui.allocate_exact_size(Vec2::new(width, list_h), Sense::hover());
    let header = Rect::from_min_size(list_rect.min, Vec2::new(width, 24.0));
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

    let body = Rect::from_min_max(Pos2::new(list_rect.left(), header.bottom()), list_rect.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body);
        ui.set_width(width);
        ui.set_max_height(body.height());
        if shown == 0 {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(
                    RichText::new(if transfer.review {
                        "清单已空，点「返回搜索」重新选择"
                    } else {
                        "没有匹配的对话，试试其他标题或文件夹"
                    })
                    .color(pal.muted),
                );
            });
            return;
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("transfer_list", transfer.review))
            .max_height(body.height())
            .auto_shrink([false, false]);
        if query_changed {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show_rows(ui, 40.0, shown, |ui, viewport| {
            for row in viewport {
                let index = rows[row];
                let session = &transfer.sessions[index];
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
                let response = ui
                    .interact(
                        rect,
                        ui.id().with(("session", &session.file)),
                        Sense::click(),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand);
                let checked = transfer.checked[index];
                let hover = ui
                    .ctx()
                    .animate_bool_with_time(response.id, response.hovered(), 0.12);
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
                    ui.ctx()
                        .animate_bool_with_time(response.id.with("check"), checked, 0.14),
                    &pal,
                );

                let text_left = rect.left() + 34.0;
                let folder_left = rect.right() - 214.0;
                let time_left = rect.right() - 132.0;
                let title_max = (folder_left
                    - text_left
                    - 12.0
                    - if session.archived {
                        measure(ui.ctx(), "· 已归档", &ui_font(10.2)) + 8.0
                    } else {
                        0.0
                    })
                .max(80.0);
                let title = theme::truncate(ui.ctx(), &session.title, &ui_font(12.2), title_max);
                ui.painter().text(
                    Pos2::new(text_left, rect.center().y),
                    Align2::LEFT_CENTER,
                    &title,
                    ui_font(12.2),
                    pal.ink,
                );
                if session.archived {
                    let width = measure(ui.ctx(), &title, &ui_font(12.2));
                    ui.painter().text(
                        Pos2::new(text_left + width + 8.0, rect.center().y),
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
                    data::format_when(data::system_time_to_local(session.updated)),
                    ui_font(10.4),
                    pal.faint,
                );
                if response.clicked() {
                    transfer.checked[index] = !checked;
                }
                response.on_hover_ui(|ui| {
                    ui.label(&session.title);
                    if !session.folder.is_empty() {
                        ui.weak(&session.folder);
                    }
                });
                ui.painter().line_segment(
                    [
                        Pos2::new(rect.left() + 34.0, rect.bottom() - 0.5),
                        Pos2::new(rect.right(), rect.bottom() - 0.5),
                    ],
                    Stroke::new(1.0, pal.hairline),
                );
            }
        });
    });

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("复制到").font(ui_font(11.8)).color(pal.muted));
        ui.spacing_mut().item_spacing.x = 6.0;
        let name = app
            .account(&transfer.target_id)
            .map_or("", |a| a.name.as_str());
        egui::ComboBox::from_id_salt("transfer_target")
            .width(300.0)
            .height(160.0)
            .selected_text(theme::truncate(ui.ctx(), name, &ui_font(12.0), 250.0))
            .show_ui(ui, |ui| {
                for account in app.accounts.iter().filter(|a| a.id != transfer.source_id) {
                    let (id, name) = (&account.id, &account.name);
                    let label = theme::truncate(ui.ctx(), name, &ui_font(12.0), 260.0);
                    if ui
                        .selectable_value(&mut transfer.target_id, id.clone(), label)
                        .on_hover_text(name)
                        .changed()
                    {
                        transfer.notice = None;
                    }
                }
            })
            .response
            .on_hover_text(name);
    });

    let hint = transfer
        .notice
        .as_deref()
        .unwrap_or("原账号会保留这份对话。要在另一账号续写，请先结束当前账号里的会话。");
    ui.add(
        egui::Label::new(RichText::new(hint).font(ui_font(10.4)).color(
            if transfer.notice.is_some() {
                pal.danger
            } else {
                pal.faint
            },
        ))
        .truncate(),
    );

    ui.add_space(2.0);
    let picked = transfer.checked.iter().filter(|v| **v).count();
    let hidden = picked - transfer.filter.checked_count(&transfer.checked);
    let selection = if transfer.review {
        format!("清单中共 {picked} 个对话")
    } else if hidden > 0 {
        format!("已选 {picked} 个（含筛选外 {hidden} 个） · 显示 {shown} / {total}")
    } else {
        format!("已选 {picked} 个 · 显示 {shown} / {total}")
    };
    let mut copy_clicked = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(selection)
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
                picked > 0,
                &pal,
            )
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
                true,
                &pal,
            )
            .clicked()
            {
                return_to_main = true;
            }
        });
    });

    if return_to_main {
        return Outcome::Cancel;
    }
    if copy_clicked {
        if app
            .account(&transfer.target_id)
            .is_some_and(|a| a.running_pid.is_some())
        {
            transfer.confirm_running = true;
        } else {
            return Outcome::Copy;
        }
    }
    Outcome::Stay
}

fn check_box(ui: &Ui, rect: Rect, checked: bool, partial: bool, t: f32, pal: &Palette) {
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
