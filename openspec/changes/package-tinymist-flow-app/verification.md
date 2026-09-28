# Verification and deployment — 2026-09-28

## Verified

- Swift 5 compilation with warnings treated as errors.
- Eight targeted configuration and update-recovery tests passed.
- Six Tail Hosting management/focus tests passed against a temporary patched copy.
- A real signed app bundle was installed, updated, and rolled back in an isolated temporary directory.
- Strict bundle signature, engine SHA-256, app version, and engine execution checks passed.
- Native UI opened with the branded icon and imported project controls; login startup was enabled.
- Installed version: 0.1.0, source revision `47149701d1ee9d79c5a0e189425f2faf8383e4b9`, clean build, ad-hoc signing.
- OpenSpec strict validation passed. Generated README consistency passed; the existing book template reports its unavailable `Source Han Serif SC` font on this host.

## macOS consent and live verification

The migration adapter was applied and the GUI Restart action launched the bundled engine under the existing preview job label. macOS TCC attributed the Documents request to `io.github.kimpossible-ty.tinymist-flow`, confirming the new identity. Computer Use refused access to the system UserNotificationCenter app for safety reasons, so the user completed Documents consent.

During the wait for consent, the previous Tail Hosting registry, launcher, and LaunchAgent were restored from the migration backup. The legacy preview resumed after consent. The migration was then reapplied and the imported profile restarted. The app and launchd both reported PID `63386` and `managedByApp: true`. The local HTTP endpoint returned 200. The unchanged Tailscale Serve route proxied the HTTPS URL to `127.0.0.1:23625`; the WebSocket returned 101 and delivered a complete 16,273,700-byte document frame.

System Settings showed `tinymist-flow` → `Documents Folder: on`. The app window showed `프리뷰 준비됨 · PID 63386` and login startup enabled. A Safari tap on the cover title recorded source `cover.typ` line 9 in the new focus file. Tail Hosting reported `service_matches: true`, `source_excerpt_matches_disk: true`, and `needs_reselection: false`; its focus PID was also `63386`. Six Tail Hosting management/focus tests passed after migration. No source document was moved and no Tailscale route was changed.

Unattended login after a real macOS login cycle and iPhone behavior after this packaging migration were not rechecked. The preview engine's existing iPhone implementation was not changed here. Ad-hoc signing caused a second Documents prompt when the final bundle replaced the earlier trial bundle. Future releases should use a stable code-signing identity and verify permission continuity.
