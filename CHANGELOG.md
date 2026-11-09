# Changelog

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
