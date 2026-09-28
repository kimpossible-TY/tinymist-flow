## Context

The VS Code `Eject` command disposes its preview webview and starts a browser preview on a new random loopback port. Today Tinymist calls `vscode.env.openExternal` without waiting for the result. In a Code Tunnel session, both automatic opening and manual port forwarding can fail while the server itself remains reachable on the remote machine. VS Code's integrated browser can reach the same server when its remote proxy setting is enabled.

## Goals / Non-Goals

**Goals:**
- Open the new browser preview automatically through a working remote path when the user has enabled integrated browser remote proxying.
- Keep the browser preview server alive and report a usable address when opening fails.
- Preserve the current external browser route for local sessions and remote sessions without proxying enabled.

**Non-Goals:**
- Provide a new public network tunnel or change the preview server's bind address.
- Alter the normal VS Code webview preview.

## Decisions

- The browser launch path checks `vscode.env.remoteName` and `workbench.browser.enableRemoteProxy`. When both indicate a remote session using browser proxying, it invokes VS Code's `workbench.action.browser.open` command with the original loopback HTTP URL. VS Code's own debug-server-ready extension uses this command with a URL string.
- Otherwise the path awaits `vscode.env.openExternal` and checks its boolean result. A failed integrated browser command can fall back to `openExternal` for compatibility with VS Code versions without the new command.
- Browser-opening failures produce a warning and leave the server registered. Users can still reach it by another route; failed UI launch does not invalidate the running preview.

## Risks / Trade-offs

- [The integrated browser command is unavailable in older VS Code versions] → Catch command failure and try the existing external browser API.
- [Remote proxying is enabled but the integrated browser cannot load the page] → The launch command can only report invocation failure; users can disable the setting or use the webview preview.
- [The integrated browser does not preserve the former webview's state] → The replacement preview starts a new task, as Eject already does.
