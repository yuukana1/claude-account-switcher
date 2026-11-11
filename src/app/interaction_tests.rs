//! Drive the actual widgets with egui input frames. No native window, registry,
//! tray registration, process scan or Claude installation is needed.
use super::*;
use crate::test_support::TestDir;
use egui::{Event, Key, Modifiers, Pos2};

pub(super) struct Fixture {
    dir: TestDir,
    pub(super) ctx: egui::Context,
    pub(super) app: SwitcherApp,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let dir = TestDir::new();
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        for name in ["ui", "semibold", "bold", "display"] {
            let faces = fonts.families[&egui::FontFamily::Proportional].clone();
            fonts
                .families
                .insert(egui::FontFamily::Name(name.into()), faces);
        }
        ctx.set_fonts(fonts);
        let pal = theme::Palette::light();
        theme::install_style(&ctx, &pal);
        let settings = Settings::load(dir.path().join("settings.json"));
        let install = Install {
            kind: data::InstallKind::Classic,
            exe: dir.path().join("fixture.exe"),
            version: "fixture".into(),
            aumid: None,
            pfn: None,
            default_profile: dir.path().join("default"),
        };
        std::fs::write(&install.exe, b"fixture").unwrap();
        let accounts = [("Alpha", "个人"), ("Beta", "工作")]
            .map(|(id, name)| Account {
                id: id.into(),
                name: name.into(),
                path: dir.path().join(id),
                is_default: false,
                color: 0,
                running_pid: (id == "Beta").then_some(123),
                last_used: None,
                session_count: 0,
                data_path: None,
            })
            .into();
        let mut app = SwitcherApp::from_accounts(
            &ctx,
            settings,
            install,
            accounts,
            pal,
            dir.path().join("icons"),
        );
        app.identity.insert(
            "Beta".into(),
            Some(Identity {
                email: Some("Dev@Example.org".into()),
                ..Default::default()
            }),
        );
        app.avatar_textures
            .insert("Beta".into(), app.logo_large.clone());
        app.next_tray_try = Instant::now() + Duration::from_secs(3600);
        Self { dir, ctx, app }
    }

    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(WINDOW_W, WINDOW_H),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                self.app.handle_keyboard(ctx);
                self.app.draw(ctx);
            },
        )
    }

    fn key(&mut self, key: Key, mods: Modifiers) {
        self.frame(
            [true, false]
                .map(|pressed| Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: mods,
                })
                .into(),
        );
    }

    fn hover_label(&mut self, label: &str) -> Pos2 {
        self.frame(vec![]);
        let output = self.frame(vec![]);
        let pos = output
            .shapes
            .iter()
            .rev()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    let pos = t.visual_bounding_rect().center();
                    assert!(
                        s.clip_rect.contains(pos) && self.ctx.content_rect().contains(pos),
                        "clipped: {label}"
                    );
                    Some(pos)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing label: {label}"));
        self.frame(vec![Event::PointerMoved(pos)]);
        pos
    }

    pub(super) fn click_label(&mut self, label: &str) {
        let pos = self.hover_label(label);
        for pressed in [true, false] {
            self.frame(vec![Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
    }
}

#[test]
fn account_search_respects_focus_ime_and_hidden_selection() {
    let mut f = Fixture::new();
    f.frame(vec![]);
    f.key(Key::F, Modifiers::CTRL);
    f.frame(vec![Event::Text("工作 EXAMPLE".into())]);
    assert_eq!(f.app.visible_accounts(), [1]);
    f.key(Key::Delete, Modifiers::NONE);
    assert!(matches!(f.app.dialog, Dialog::None));
    f.app.ime_composing = true;
    f.key(Key::Enter, Modifiers::NONE);
    assert_eq!(f.app.selected_id, "Alpha");
    f.app.ime_composing = false;
    f.key(Key::Enter, Modifiers::NONE);
    assert_eq!(f.app.selected_id, "Beta");
    assert!(matches!(f.app.dialog, Dialog::None));
    assert!(f.app.toast.is_none());
    f.key(Key::Escape, Modifiers::NONE);
    f.click_label("仅运行中");
    assert_eq!(f.app.visible_accounts(), [1]);
    f.key(Key::F, Modifiers::CTRL);
    f.frame(vec![Event::Text("missing".into())]);
    assert!(f.app.visible_accounts().is_empty());
    f.ctx.memory_mut(|m| m.surrender_focus(account_search_id()));
    f.key(Key::Delete, Modifiers::NONE);
    assert!(matches!(f.app.dialog, Dialog::None));
    f.click_label("清除筛选");
    assert_eq!(f.app.visible_accounts(), [0, 1]);
    assert_eq!(f.app.selected_id, "Beta");
}

#[test]
fn pins_follow_profile_id_after_rename_and_unpin_restores_original_order() {
    let mut f = Fixture::new();
    f.hover_label("工作");
    f.click_label("置顶");
    assert_eq!(f.app.accounts[0].id, "Beta");
    assert_eq!(f.app.selected_id, "Alpha");
    f.app.settings.set_label("Beta", Some("另一个名字"));
    f.app.settings.save().unwrap();
    let path = f.dir.path().join("settings.json");
    assert!(Settings::load(path.clone()).pinned("Beta"));
    f.click_label("已置顶");
    assert_eq!(f.app.accounts[0].id, "Alpha");
    let reloaded = Settings::load(path);
    assert!(!reloaded.pinned("Beta"));
    assert_eq!(reloaded.label_for("Beta").as_deref(), Some("另一个名字"));
}

#[test]
fn transfer_bulk_selection_keeps_hidden_files_and_confirms_the_right_target() {
    let mut f = Fixture::new();
    let mut extra_target = f.app.accounts[1].clone();
    extra_target.id = "Gamma".into();
    extra_target.name = "设计".into();
    extra_target.path = f.dir.path().join("Gamma");
    f.app.accounts.push(extra_target);
    let store = f.dir.path().join("Alpha/claude-code-sessions/a/o");
    std::fs::create_dir_all(&store).unwrap();
    for (name, title, folder) in [
        ("a", "First", "tools"),
        ("b", "Second", "tools"),
        ("c", "Third", "misc"),
    ] {
        let json = json!({"title":title, "folder":folder});
        std::fs::write(store.join(format!("local_{name}.json")), json.to_string()).unwrap();
    }
    f.app.open_transfer("Alpha");
    f.frame(vec![]);
    f.key(Key::F, Modifiers::CTRL);
    f.frame(vec![Event::Text("TOoLs".into())]);
    f.click_label("全选当前结果");
    f.click_label("TOoLs");
    f.key(Key::A, Modifiers::COMMAND);
    f.frame(vec![Event::Text("second tools".into())]);
    f.click_label("全选当前结果"); // deselect only the visible "Second"
    let Dialog::Transfer(t) = &f.app.dialog else {
        panic!("picker closed")
    };
    assert_eq!(t.filter.visible.len(), 1);
    assert_eq!(t.filter.checked_count(&t.checked), 0);
    f.click_label("工作");
    f.click_label("设计");
    f.click_label("复制对话");
    assert!(matches!(&f.app.dialog, Dialog::Transfer(t) if t.confirm_running));
    f.click_label("返回选择");
    let Dialog::Transfer(t) = &f.app.dialog else {
        panic!("lost draft")
    };
    assert!(!t.confirm_running);
    assert_eq!(t.target_id, "Gamma");
    assert_eq!(t.filter.query, "second tools");
    assert_eq!(
        t.sessions
            .iter()
            .zip(&t.checked)
            .filter_map(|(s, on)| on.then_some(&s.file))
            .collect::<Vec<_>>(),
        [&store.join("local_a.json")]
    );
    f.click_label("查看已选");
    f.click_label("First");
    let Dialog::Transfer(t) = &f.app.dialog else {
        panic!("lost review")
    };
    assert!(t.review && !t.checked.iter().any(|on| *on));
    f.key(Key::Escape, Modifiers::NONE);
    assert!(
        matches!(&f.app.dialog, Dialog::Transfer(t) if !t.review && t.filter.query == "second tools")
    );
    f.click_label("Second");
    f.click_label("复制对话");
    f.click_label("仍然复制");
    assert!(
        matches!(&f.app.dialog, Dialog::Transfer(t) if t.notice.is_some() && !t.confirm_running && t.checked.iter().filter(|&&v| v).count() == 1)
    );
    assert!(!f.dir.path().join("Gamma/claude-code-sessions").exists());
    f.click_label("取消");
    assert!(matches!(f.app.dialog, Dialog::None));
}

#[test]
fn keyboard_browses_search_results_without_persisting_or_launching_until_chosen() {
    let mut f = Fixture::new();
    f.frame(vec![]);
    f.key(Key::F, Modifiers::CTRL);
    f.frame(vec![Event::Text("a".into())]); // both IDs match
    assert_eq!(f.app.account_filter.cursor_id.as_deref(), Some("Alpha"));
    f.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(f.app.account_filter.cursor_id.as_deref(), Some("Beta"));
    assert_eq!(f.app.selected_id, "Alpha");
    assert_eq!(f.app.settings.last_account(), None);
    f.app.ime_composing = true;
    f.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(f.app.account_filter.cursor_id.as_deref(), Some("Beta"));
    f.app.ime_composing = false;
    f.key(Key::Enter, Modifiers::NONE);
    assert_eq!(f.app.settings.last_account().as_deref(), Some("Beta"));
    assert!(matches!(f.app.dialog, Dialog::None));
    assert!(f.app.toast.is_none());
}

#[test]
fn partial_copy_retries_only_failed_files_and_never_overwrites_the_destination() {
    let mut f = Fixture::new();
    f.app.accounts[1].running_pid = None;
    let source = f.dir.path().join("Alpha/claude-code-sessions/src/org");
    let dest = f.dir.path().join("Beta/claude-code-sessions/dst/team");
    for dir in [&source, &dest] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let valid = r#"{"title":"Notes","account":"src","org":"org"}"#;
    for file in ["local_good.json", "local_old.json"] {
        std::fs::write(source.join(file), valid).unwrap();
    }
    std::fs::write(source.join("local_fix.json"), "{broken").unwrap();
    std::fs::write(dest.join("local_old.json"), "keep existing").unwrap();
    f.app.open_transfer("Alpha");
    f.click_label("全选当前结果");
    f.click_label("复制对话");
    let Dialog::Transfer(t) = &f.app.dialog else {
        panic!("lost failed entries")
    };
    assert!(
        t.review
            && t.notice
                .as_deref()
                .unwrap()
                .contains("已复制 1 个，跳过 1 个")
    );
    assert_eq!(
        t.sessions
            .iter()
            .zip(&t.checked)
            .filter_map(|(s, on)| on.then_some(&s.file))
            .collect::<Vec<_>>(),
        [&source.join("local_fix.json")]
    );
    assert!(!dest.join("local_fix.json").exists());
    assert_eq!(f.app.accounts[1].session_count, 2);
    std::fs::write(source.join("local_fix.json"), valid).unwrap();
    f.click_label("复制对话");
    assert!(
        matches!(&f.app.dialog,Dialog::Message{title,body,..} if title=="对话已复制" && body.contains("已复制 1 个") && !body.contains("已存在"))
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("local_old.json")).unwrap(),
        "keep existing"
    );
    let fixed: Value =
        serde_json::from_slice(&std::fs::read(dest.join("local_fix.json")).unwrap()).unwrap();
    assert_eq!(fixed["account"], "dst");
    assert_eq!(fixed["org"], "team");
    assert_eq!(f.app.accounts[1].session_count, 3);
}

#[test]
fn keyboard_candidate_scrolls_into_view_in_a_long_account_list() {
    let mut f = Fixture::new();
    for num in 0..20 {
        let mut account = f.app.accounts[0].clone();
        account.id = format!("Team-{num:02}");
        account.name = account.id.clone();
        f.app.accounts.push(account);
    }
    f.frame(vec![]);
    f.key(Key::F, Modifiers::CTRL);
    f.frame(vec![Event::Text("team".into())]);
    for _ in 1..20 {
        f.key(Key::ArrowDown, Modifiers::NONE);
    }
    assert_eq!(f.app.account_filter.cursor_id.as_deref(), Some("Team-19"));
    for _ in 0..40 {
        f.frame(vec![]);
    }
    f.hover_label("Team-19"); // also checks the label is inside the viewport
    f.key(Key::Enter, Modifiers::NONE);
    assert_eq!(f.app.selected_id, "Team-19");
}
