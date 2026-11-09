#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(float_literal_f32_fallback)]
#![allow(dead_code)]

mod anim;
mod app;
mod data;
mod identity;
mod settings;
mod theme;
mod win;

use std::time::Instant;

use data::{discover_accounts, discover_install, paths, refresh_metadata, read_usage};
use settings::Settings;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|arg| arg == "--dump") {
        dump();
        return Ok(());
    }

    if !win::single_instance() {
        return Ok(());
    }

    let icon = theme::app_icon(64);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Claude 账号切换器")
            .with_inner_size([app::WINDOW_W, app::WINDOW_H])
            .with_min_inner_size([app::WINDOW_W, app::WINDOW_H])
            .with_max_inner_size([app::WINDOW_W, app::WINDOW_H])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(false)
            .with_position([120.0, 60.0])
            .with_icon(std::sync::Arc::new(icon)),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Claude 账号切换器",
        options,
        Box::new(|cc| Ok(Box::new(app::SwitcherApp::new(cc)))),
    )
}

fn dump() {
    let started = Instant::now();
    let paths = paths();
    let mut settings = Settings::load(paths.settings.clone());
    let install = discover_install(settings.install_cache());
    println!("settings   : {}", paths.settings.display());
    println!(
        "install    : {:?} {} {}",
        install.kind,
        install.version,
        install.exe.display()
    );
    println!("aumid      : {:?}", install.aumid);
    println!("pfn        : {:?}", install.pfn);
    println!("default    : {}", install.default_profile.display());
    println!("data dirs  : {:?}", data::claude_data_dirs(install.pfn.as_deref()));

    let mut accounts = discover_accounts(&install, &mut settings);
    for account in &mut accounts {
        refresh_metadata(&install, account);
        println!(
            "\n== {} (id={}, default={}) color={} sessions={}\n   path={}\n   roots={:?}\n   last_used={:?}",
            account.name,
            account.id,
            account.is_default,
            account.color,
            account.session_count,
            account.path.display(),
            data::profile_data_roots(&install, account),
            account.last_used
        );
        if let Some(usage) = read_usage(&install, account) {
            let now = chrono::Local::now();
            println!(
                "   usage: fh={:?} sd={:?} updated={} org={} fh_active={} fh_reset={:?} sd_anchor={:?}",
                usage.fh,
                usage.sd,
                usage.updated,
                usage.org,
                usage.fh_active,
                usage.fh_reset_at.map(|t| t.to_string()),
                usage.sd_anchor.map(|t| t.to_string())
            );
            println!("   rows : fh={:?} sd={:?}", usage.fh_row(now), usage.sd_row(now));
        }
        let sessions = data::list_sessions(&install, account);
        println!(
            "   identity={:?}",
            identity::identity_from_profile(&data::profile_data_roots(&install, account))
        );
        println!("   list_sessions={}", sessions.len());
        for session in sessions.iter().take(20) {
            println!(
                "     - {:?} | {} | folder={:?} | archived={} | {}",
                session.title,
                data::format_when(data::system_time_to_local(session.updated)),
                session.folder,
                session.archived,
                session.file.display()
            );
        }
        println!("   store={:?}", data::session_store(&install, account));
    }
    println!("\ndone in {:?}", started.elapsed());
}
