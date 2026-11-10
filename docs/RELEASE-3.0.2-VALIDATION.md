# v3.0.2 validation — 2026-11-09

## Changes

The Rust rewrite omitted the old PowerShell switcher's per-profile window identities. Separate `--user-data-dir` processes alone still share Claude's taskbar group. The switcher now sets a per-profile AppUserModelID on secondary-profile Electron windows, together with the profile's relaunch command, name and icon. The Claude icon has a circular account-avatar badge; without a custom avatar the badge uses the initial and stored color. An artwork digest is part of the identity so Explorer does not retain stale avatar artwork. Existing visible buttons are re-registered on identity changes, hidden windows are not added to the taskbar, and the default account's window properties are left untouched. The switcher retries during its normal polling while running, including in the tray.

An old `Claude Profile Switcher.lnk.disabled` Shell Link was also found in the user's Startup directory. Windows can open files in this directory even when their extension has been changed. The file was moved outside Startup and its SHA-256 was checked before and after the move. Normal application startup now performs this narrowly scoped migration as well. It preserves existing backups, skips reparse points/non-shortcuts, and reports failures without a copy/delete fallback.

The reported Open With dialog was not reproduced during investigation, so the timing relationship between a secondary-account launch and that legacy file is not established. This release removes the identified startup artifact rather than claiming that every possible `.disabled` dialog has the same cause.

## Automated checks

```powershell
cargo fmt --all -- --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --release --locked --offline
git diff --check
```

All local checks passed. The suite contains 38 tests, including eight additions:

- Stable, distinct taskbar identities for Unicode/punctuation paths, Windows path case equivalence, and bounded identity length.
- Windows command-line round-tripping for profile relaunch commands with spaces and Unicode.
- An actual hidden Win32 window: leave the default identity alone, apply secondary-profile properties, update a renamed display name, keep the group unchanged, and leave the window hidden.
- Move the known disabled link without overwriting an existing backup, leave enabled/unrelated links alone, and make repeated migration a no-op.
- Preserve missing/unrelated files without creating backup directories.
- Preserve the source when the backup location is unusable.
- Compose the avatar badge while preserving Claude's mark, and decode all seven ICO PNG frames at their declared sizes.
- Reuse unchanged artwork and publish a different content-addressed icon when the avatar changes, preserving previous files.

## Local application checks

- Started the release build with isolated APPDATA/LOCALAPPDATA and a temporary secondary account. The cache pointed to the installed official Claude executable.
- Clicked **Open Claude** once: a separate Claude process started with the intended Unicode/space-containing profile path and the switcher showed it as running. No Open With process/dialog was observed.
- Clicked **Focus Claude**: the existing test window was raised and the switcher reported success.
- Read the test window's property store with `--dump`: the window carried its expected per-profile identity.
- Stopped only the owned test processes, preserved the old portable launcher, replaced the original `dist/Claude Account Switcher.exe`, and restarted it.
- Confirmed the original secondary-account Claude process remained running. Before the update its two eligible windows had empty explicit AppUserModelID properties; after the update both carried the same profile-specific identity.
- Observed v3.0.2 in the original switcher's window. Further UI input stopped when user activity was detected in that window.
- The user confirmed that separate taskbar icons now opened their corresponding windows, then requested avatar badges. The badge implementation was added before publishing this release.
- Checked the generated avatar badge preview and native icon properties on the existing secondary-account windows. After feedback, enlarged the badge from half the canvas to 62.5%, offset the main mark to the upper left, and added an assertion that its center remains unobscured. The five taskbar/artwork tests and strict Clippy passed again after this adjustment.

These checks establish native window-property behavior and the isolated secondary-account launch/focus path. They do not establish real sign-in, conversation continuation, every Windows taskbar/pinning configuration, or a reproduced before/after fix for the original dialog. Local profiles, raw diagnostic dumps, executable backups and synthetic fixtures are excluded from Git and release assets.
