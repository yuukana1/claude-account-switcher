# Code review and validation — 2026-11-09

The review started from `88acedff55ef7724f5bc84c1e0beb197510f9f48` (`main`, v3.0.0) and focused on profile data integrity, Windows process discovery, account operations and maintainability. The initial four tests passed, while formatting and strict Clippy checks failed. The fixes are included in v3.0.1.

## Findings and fixes

| Priority | Problem | Resulting behavior |
| --- | --- | --- |
| P1 | Settings failures were swallowed, a failed rename fell back to writing the live file, and unreadable settings became writable defaults. | Publish complete temporary files, return save errors, keep failed changes dirty for retry, and preserve unreadable files until repaired and reloaded. |
| P1 | Conversation copying replaced matching strings throughout raw JSON and checked destination existence before a separate write. | Parse JSON and change only the top-level `account` and `org` fields. Atomically publish without replacement, including when another copy wins a race. |
| P1 | Deletion resolved the profile again after selection and could choose an alternate directory; validation covered lexical paths only. | Delete the selected path, check its canonical parent, refuse reparse points/default profiles/traversal, and recheck process state before removal. |
| P2 | Running-account discovery used substring parsing and missed classic installs and quoted Windows arguments. Native string decoding formed an unaligned reference and assumed an inline offset. | Parse arguments with Windows' API, match executable paths, validate the returned native pointer and length, and query without remote-memory-read permission. |
| P2 | An initial identity lookup miss stayed cached, and reloaded accounts did not load identity or avatar data. | Refresh identities periodically, retry misses, load new accounts and remove obsolete cache entries. |
| P2 | Usage records could mix organizations, trust malformed numbers and iterate across every elapsed week. | Validate and sort samples, isolate the current organization, use checked date arithmetic and advance weekly windows in constant time. |
| P2 | Avatar filenames collapsed punctuation, large images were decoded without explicit limits, and the picker advertised unsupported WebP. | Escape filenames without collisions, use unambiguous legacy fallback, limit decoding and offer PNG/JPEG. |
| P2 | Fast `Ctrl+N` could miss the modifier state; an add error dialog was discarded; long account names overflowed a copy-result button. | Consume shortcut events with their own modifiers, retain errors, and use a fixed action label. Cancelled copy confirmations also release pending work. |
| P2 | README claimed account deletion never happened, and a fresh checkout was told to build offline. | Document destructive behavior, Windows prerequisites, online first-build requirements, development checks and test boundaries in both languages. |
| P3 | The version was duplicated and compiler/lint failures were easy to miss. | Use Cargo's version in the UI/diagnostic command, propagate resource compilation errors, format the source and add Windows CI. |

## Automated validation

Run on Windows with Rust 1.98.1 and the locked dependency graph:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --release --locked --offline
```

The suite contains 30 tests. New cases include locked-file replacement and retry, preserving corrupt settings and unknown keys, BOM/Unicode handling, eight concurrent publishers, content-preserving copies, invalid session JSON, deletion confinement and alternate-profile preservation, native command-line decoding, quoted profile arguments, invalid quota samples, historical weekly windows, avatar collisions/limits, and a shortcut press/release in a single input frame.

Test fixtures live in owned temporary directories. No unit test changes process-wide APPDATA variables, touches real profiles, signs in, launches Claude, or changes startup registration.

## Windows UI validation

The release executable was started with separate APPDATA and LOCALAPPDATA directories and a synthetic installation cache. Existing switcher processes were closed with user authorization. The checks exercised the actual application window:

- Selected each account and checked its usage/session view.
- Renamed an account to `测试工作账号` and verified persisted JSON.
- Copied a synthetic conversation through the picker and verified target metadata, unchanged message content and source preservation.
- Repeated the copy and observed zero copies plus one existing conversation.
- Opened and cancelled account creation; verified that no new profile directory appeared.
- Changed the theme, restarted, and verified the saved theme and label.
- Closed the window to the notification area, then launched a second instance; the existing process/window returned and the second process exited.
- Reproduced the original quick-shortcut and button-overflow issues, then verified both fixes in the rebuilt executable.
- Ran `--dump` against the same fixture and checked that it did not modify settings.

## Scope and limitations

These results establish behavior with controlled local files and Windows UI interaction. They do not establish compatibility with every Claude data format, real sign-in, launching multiple authenticated Claude accounts, resuming transferred conversations, or recovery from all storage/power failures. Identity extraction still depends on the desktop client's IndexedDB representation, and usage resets remain estimates from local history.

The GitHub workflow performs formatting, strict linting, tests and release compilation on `windows-latest`. Its remote result must be checked separately from local results before publishing a release.

Temporary UI fixtures and local build evidence are excluded by `/build`. The automatic command policy rejected recursive cleanup of the synthetic UI directory; it contains test data only and is not included in Git or release assets.
