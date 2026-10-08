## Why

Ejecting a preview in a VS Code remote workspace can close the webview without opening its replacement. The preview server remains healthy, but the external browser path depends on port forwarding, which may be unavailable in a Code Tunnel session.

## What Changes

- When VS Code's integrated browser remote proxy is enabled in a remote workspace, open browser previews through the integrated browser so they can reach the remote preview server directly.
- Keep the existing external browser behavior when remote proxying is not enabled.
- Report browser opening failures while leaving the running preview server available.

## Capabilities

### New Capabilities

- `vscode-browser-preview`: Browser preview routing and failure behavior in local and remote VS Code workspaces.

### Modified Capabilities

None.

## Impact

- `editors/vscode/src/features/preview.ts`
- VS Code preview tests
- VS Code's integrated browser command and remote proxy setting
