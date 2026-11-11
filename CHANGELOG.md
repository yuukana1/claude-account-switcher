# Changelog

## 3.2.1 — 2026-11-11

- Check the latest stable release using its public `update.json` attachment so shared GitHub API rate limits do not block daily or manual checks. Keep the API as a fallback and continue verifying the download's size and SHA-256.
- When publishing, run `tools/release-manifest.ps1 -Executable <versioned-release-exe>` and attach the generated `update.json` alongside the executable.

## 3.2.0 — 2026-11-11

- Check for stable updates from the sidebar version, More menu or tray. Check once per local calendar day by default, persist the attempt across restarts, and allow manual checks or opting out. Background checks do not interrupt account work.
- Download the official x64 executable inside the app with progress, size and SHA-256 verification. Restart on request, preserving the executable path and a rollback copy. The new process verifies its version before the old one exits; Claude windows are not closed.
- Add a read-only `--check-update` diagnostic and a `--version` option.

## 3.1.1 — 2026-11-11

- Browse account search results with the arrow keys. The highlighted result scrolls into view without switching accounts; Enter selects it. Pin and unpin directly on a sidebar row without changing the selection.
- Review all selected conversations across searches, remove individual entries and return to the same query. Backing out of a running-account warning or choosing an unready destination preserves the draft.
- On a partial copy, show only failed entries for retry and leave existing files intact. Refresh the destination after copying instead of rescanning every profile. Archived conversation titles now leave room for the archive label; hover to read the full title and folder.
- Reveal the switcher when choosing Add Account from the tray, so the confirmation is visible even when the window was closed to the notification area.

## 3.1.0 — 2026-11-11

- Find accounts by label, profile ID or email with Ctrl+F, and optionally show only running accounts. Search does not change the active account; Enter in the search box selects the first result and Esc restores the full list.
- Pin frequently used profiles from their context menu. Pins use the profile ID, persist across renames and restarts, and apply in the sidebar and notification-area menu.
- Search conversations by title and folder when copying. Bulk selection applies to the current results and preserves earlier choices. The picker shows how many selected conversations are hidden, scrolls long lists, and keeps long destination names inside a dropdown.
- Fix a debug-build panic when drawing the default account logo or a custom avatar.

Validated on Windows with 41 tests, including keyboard input, cross-filter selection and switching the destination before confirming a copy. Native window previews use sample profiles; no signed-in account was used for this validation.

## 3.0.2 — 2026-11-09

- Restore a separate taskbar group for each secondary account. The switcher applies a stable per-profile AppUserModelID to existing and newly created Claude windows, including while it is in the tray. The default account keeps Claude's original identity.
- Add the account's avatar as a large circular corner badge on Claude's taskbar icon. The main mark sits at the upper left so its center remains visible; the avatar fills 62.5% of the full icon width at the lower right. Accounts without an avatar use their initial and stored color. Avatar changes use a new artwork revision so Explorer refreshes cached icons; PNG-compressed ICO frames cover 16–256 pixels.
- Keep secondary-account taskbar relaunch commands tied to the correct profile, including Unicode names and paths with spaces. Renaming updates the taskbar display name; the group identity stays stable while its artwork is unchanged.
- Move the known legacy `Claude Profile Switcher.lnk.disabled` shortcut out of the Windows Startup folder into `ClaudeProfiles/.switcher/startup-backups`. Changing a startup file's extension can cause an Open With prompt. The shortcut is preserved, existing backups are never overwritten, and unrelated or enabled startup items are left alone.
- Include read-only per-window taskbar identities and icon metadata in `--dump` and add eight regression tests, including actual hidden Win32 window properties, avatar changes, ICO frames, and backup collision/failure cases.

## 3.0.1 — 2026-11-09

- Preserve settings and avatars when a save fails. Settings errors are reported, pending changes remain available for retry, and unreadable settings are not replaced with defaults.
- Copy conversation indexes through parsed JSON, changing only account/organization metadata. Conversation content, paths and the source file stay unchanged; concurrent copies cannot overwrite an existing destination.
- Delete the profile the user actually selected, validate its resolved location, refuse linked or default profiles, and refresh running-process information before deletion. The confirmation and README now describe the permanent removal of local conversations.
- Recognize Windows profile arguments with spaces, quoting and both `--user-data-dir` forms. Decode native process information with alignment and bounds checks and reduced process permissions.
- Refresh cached account identities, isolate usage history by organization, ignore invalid samples, and calculate old weekly windows without repeated iteration.
- Keep distinct account names from sharing avatar files, bound image decoding, and align the image picker with supported formats.
- Fix quick `Ctrl+N` presses, preserve add-account error dialogs, clear cancelled copy operations, and keep long account names out of fixed-width action buttons.
- Add Windows CI for formatting, strict Clippy, 30 regression tests and release builds. The diagnostic command and UI use the package version.

Validation used isolated profile fixtures and the actual Windows UI. It covered Unicode renaming, settings persistence, conversation copying and duplicate skipping, add cancellation, both themes, and restoring the window after closing it. Real Claude sign-in, account launching and conversation continuation were not exercised.
