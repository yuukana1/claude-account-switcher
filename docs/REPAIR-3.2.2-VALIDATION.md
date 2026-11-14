# v3.2.2 local validation — 2026-11-14

This version is not published. Wait for the user to restart Windows and validate
Claude before creating a release.

## Findings

- The registered Claude Store version is `5.11492.5.0`. The cached
  `5.11492.2.0` executable still exists, so checking only file existence incorrectly
  treats the cache as current.
- The current user's package status is `Servicing`. Direct launch of the new
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
- The release build's read-only `--diagnose-install` reports the current
  `5.11492.5.0` location and the unhealthy package status.
- The canonical local executable was replaced with a v3.2.2 test build using the
  version handshake, with the previous executable retained as a local backup.
- The enabled per-user Run entry now includes `--background`.
- A real process started using `--background` was sampled 141 times over seven
  seconds. No sample showed an on-screen window or foreground ownership by the
  launcher. Its native style excluded a taskbar button.
- A second background invocation exited without revealing the existing window.
  A manual invocation restored the existing window to the screen.

## Pending after the user's restart

1. Confirm actual Windows sign-in starts the launcher in the notification area
   without focusing or displaying its window.
2. Inspect the Claude package status. If necessary, retry registration of the
   installed manifest without removing the package or account data.
3. Open the default account and an existing secondary account and verify their
   windows, profile identity and taskbar grouping.
4. Only after these checks, finalize the changelog and publish the patch release
   with its executable, SHA256SUMS.txt and update.json.
