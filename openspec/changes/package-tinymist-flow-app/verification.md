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

## Pending macOS consent and live verification

The migration adapter was applied and the GUI Restart action launched the bundled engine under the existing preview job label. macOS TCC attributed the Documents request to `io.github.kimpossible-ty.tinymist-flow`, confirming the new identity. The process then waited for Documents consent. Computer Use refused access to the system UserNotificationCenter app for safety reasons; the user was asked to complete the prompt.

The previous Tail Hosting registry, launcher, and LaunchAgent were restored from the migration backup. The legacy engine was restarted, but HTTPS still returned 502 while the OS access request remained pending. **Live preview recovery and migration are not yet verified.** Do not interpret the running process or a successful installer as completed document delivery.

After consent: prepare migration again, Restart the imported profile, verify HTTPS and a complete WebSocket document frame, inspect the new focus file PID against launchd, and confirm the Files & Folders entry. No source document needs to be moved. No Tailscale routes were changed.

The locally installed app and recovery tooling are usable. The release does not yet establish unattended login behavior or iPhone behavior after this packaging migration. Those remain deployment checks; the preview engine's existing iPhone implementation was not changed here.
