# Claude Account Switcher

English | [简体中文](README.zh-CN.md)

A portable Windows tray app for running several Claude desktop accounts side by side.
Each account keeps its own profile directory, so switching never asks you to sign in again.

It targets the **official Claude desktop app for Windows** (tested with 4.23118.x).

![Claude Account Switcher](dist/preview-light.png)
*Screenshots use redacted account details.*

Previews with sample profiles: [account search and pins](dist/preview-accounts-3.1.png) · [filtered conversation copying](dist/preview-transfer-3.1.png) · [reviewing a selection](dist/preview-selection-3.1.1.png).

Two accounts running side by side:

![Two Claude accounts running side by side](dist/preview-multi.png)

## Features

- **Accounts** — lists the profiles already on this PC with e-mail, running state and Claude Code conversation count.
- **Find an account** — press `Ctrl+F` to search labels, profile IDs or e-mail addresses. Words can match different fields. Filter to running accounts, use the arrow keys to browse matches, press Enter to select one, or Esc to clear the filters.
- **Pin accounts** — hover over an account and click its pin, or use its context menu. Pins apply to the sidebar and tray menu and survive restarts and renaming.
- **Usage** — live 5-hour / weekly usage, reset countdown, and a warning shortly before a reset.
- **Switching** — opens or focuses an account in its own window, and copies Claude Code conversations between accounts.
- **Batch copy** — select conversations across several searches, review the full selection and remove individual entries before copying. Returning from confirmation or choosing an unready destination keeps your draft. After a partial copy, only failed entries remain selected for retry.
- **Taskbar** — each secondary account has its own taskbar group, with its avatar in a corner of the Claude icon (or its initial/color if no avatar is set). Already-running windows are updated without restarting Claude. Keep the switcher running in the tray so it can also tag new windows.
- **Personal** — avatar and nickname per account; light and dark themes.
- **Removal** — permanently removes a selected non-default profile after confirmation, including its local conversations. Back up anything you need first.
- **Tray** — the titlebar × only closes the window; the app stays in the notification area. Quit lives in the tray menu.

## Run

`dist/Claude Account Switcher.exe` — one file, no installer, no .NET or PowerShell.

## Build

Requires Rust's MSVC toolchain and Visual Studio Build Tools with the C++ workload and Windows SDK.

```powershell
cargo build --release --locked
```

Output: `target\release\claude-account-switcher.exe`.

Add `--offline` if the dependencies are already cached. A fresh build needs to download them once.

## Development checks

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests use temporary directories and synthetic profile data. They do not change your Claude profiles, sign in, launch Claude or modify startup entries. GitHub Actions runs these checks and builds the executable on Windows.

## Data & safety

The app reads Claude's profiles and settings and makes no network requests. Adding an account creates a separate profile directory and launches Claude. Explicit conversation copies write new index files without overwriting existing copies; the source stays unchanged. Confirming removal permanently deletes the selected non-default profile directory and its local conversations. The default account cannot be removed.

The switcher's settings and avatars are saved by replacing a complete temporary file. Failed saves are reported and leave the previous file intact. If an existing settings file cannot be read or parsed, it is preserved; repair it and restart the switcher before saving changes.

On startup, the switcher also checks for the old `Claude Profile Switcher.lnk.disabled` shortcut in the current user's Windows Startup folder. If it is a regular Shell Link file, it is moved to `%LOCALAPPDATA%\ClaudeProfiles\.switcher\startup-backups`, preserving its contents and any earlier backups. A disabled shortcut should be kept outside Startup to avoid Windows' file-association prompt. Active startup shortcuts and other applications' entries are not migrated.

Anthropic Sans/Serif are extracted locally from the installed Claude app and are not bundled.

## Credits

Based on [Claude Code Desktop Switcher](https://github.com/PriyanshuGeTRekT/Claude-Code-Desktop-Switcher) (MIT). See [LICENSE](LICENSE).
