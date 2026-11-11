//! Drive the actual widgets with egui input frames. No native window, registry,
//! tray registration, process scan or Claude installation is needed.
use super::*;
use crate::test_support::TestDir;
use egui::{Event, Key, Modifiers, Pos2};

struct Fixture {
    dir: TestDir,
    ctx: egui::Context,
    app: SwitcherApp,
}

impl Fixture {
    fn new() -> Self {
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

    fn click_label(&mut self, label: &str) {
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
    f.app
        .apply_action(UiAction::TogglePin("Beta".into()), &f.ctx);
    assert_eq!(f.app.accounts[0].id, "Beta");
    f.app.settings.set_label("Beta", Some("另一个名字"));
    f.app.settings.save().unwrap();
    let path = f.dir.path().join("settings.json");
    assert!(Settings::load(path.clone()).pinned("Beta"));
    f.app
        .apply_action(UiAction::TogglePin("Beta".into()), &f.ctx);
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
    assert!(matches!(
        f.app.dialog,
        Dialog::Message {
            action: DialogAction::PerformCopy,
            ..
        }
    ));
    let pending = f.app.pending_copy.as_ref().unwrap();
    assert_eq!(pending.target_id, "Gamma");
    assert_eq!(pending.sessions.len(), 1);
    assert_eq!(pending.sessions[0].file, store.join("local_a.json"));
    f.click_label("取消");
    assert!(f.app.pending_copy.is_none());
}
