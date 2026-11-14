# v3.2.2 local validation — 2026-11-14

The user restarted Windows and confirmed that two Claude Code windows open.
Post-restart checks below completed before preparing the patch release.

## Findings

- The registered Claude Store version is `5.11492.5.0`. The cached
  `5.11492.2.0` executable still exists, so checking only file existence incorrectly
  treats the cache as current.
- Before restarting, the current user's package status was `Servicing`. Direct launch of the new
  executable returns access denied. Re-registering the installed manifest fails
  with `0x80073CF6`, caused by `0x80070430` while registering `CoworkVMService`
  (service marked for deletion).
- No Claude profile directory was deleted, reset or recreated. The installed
  legacy PowerShell launcher was backed up locally and its Store cache checks
  were changed to refresh even when the retired executable exists. It passed
  PowerShell syntax parsing.

## Completed

- Formatting, strict Clippy, 52 unit tests and 3 Windows update integration tests
  passed. The separately ignored network test was not part of this check.
- A regression test confirms that the current Store registration wins over an
  old but existing executable while retaining profile identity and paths.
- Native launch tests verify that a missing executable and an unregistered
  application return errors instead of reporting successful activation.
- Before restarting, the release build's read-only `--diagnose-install` reported
  the current `5.11492.5.0` location and the unhealthy package status.
- The canonical local executable was replaced with a v3.2.2 test build using the
  version handshake, with the previous executable retained as a local backup.
- The enabled per-user Run entry now includes `--background`.
- A real process started using `--background` was sampled 141 times over seven
  seconds. No sample showed an on-screen window or foreground ownership by the
  launcher. Its native style excluded a taskbar button.
- A second background invocation exited without revealing the existing window.
  A manual invocation restored the existing window to the screen.

## Post-restart checks

- Windows now reports the installed `5.11492.5.0` package status as `Ok`. Native
  installation diagnostics also report a healthy status.
- The running launcher was started with `--background`. Its executable matches
  the tested v3.2.2 release build. The current Run entry was reaffirmed with the
  same background argument.
- The user reports that two Claude Code windows open. A read-only process query
  confirms two desktop main processes from `5.11492.5.0`: one default process
  and one with an explicit secondary profile directory.
- These observations confirm startup and account launch on this machine. They
  do not independently establish focus behavior across an entire Windows sign-in,
  or renewed visual acceptance of taskbar grouping; the prior controlled
  background-start sampling is recorded above.
- No additional package repair or Claude process termination was needed after
  the restart. The release includes the executable, SHA256SUMS.txt and update.json.
