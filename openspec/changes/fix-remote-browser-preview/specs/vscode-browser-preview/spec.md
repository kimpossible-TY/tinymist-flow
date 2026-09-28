## ADDED Requirements

### Requirement: Browser previews use the integrated browser when remote proxying is enabled

The VS Code Tinymist extension SHALL open browser previews in VS Code's integrated browser when the extension is running in a remote workspace and integrated browser remote proxying is enabled.

#### Scenario: Eject in a remote workspace with browser proxying
- **WHEN** a user ejects a preview webview in a remote workspace with `workbench.browser.enableRemoteProxy` enabled
- **THEN** Tinymist starts the replacement preview server and opens its loopback URL in the integrated browser

#### Scenario: Normal external browser route
- **WHEN** a user starts a browser preview locally or in a remote workspace without integrated browser remote proxying enabled
- **THEN** Tinymist opens the preview through VS Code's external browser API

### Requirement: Browser launch failures preserve the preview server

The VS Code Tinymist extension SHALL report a browser launch failure without shutting down the new preview server.

#### Scenario: Integrated browser command unavailable
- **WHEN** the integrated browser command fails to launch
- **THEN** Tinymist tries the external browser API

#### Scenario: No browser opens
- **WHEN** all attempted browser routes fail
- **THEN** Tinymist reports the preview server address and keeps the preview task running
