# Keyboard and mouse bindings

SUPER means the Windows key. Bindings are configurable. Proposed defaults are not approved requirements.

| Binding | Action | Status |
| --- | --- | --- |
| SUPER+Space | Open app-only launcher. | Agreed |
| SUPER+Q | Launch PowerShell 7 (`C:\Program Files\PowerShell\7\pwsh.exe`). | Agreed during implementation |
| SUPER+X | Close focused window; suppress Windows power-user menu. | Agreed |
| SUPER+1..9 | Switch both monitors to workspaces 1..9. | Agreed |
| SUPER+0 | Switch both monitors to workspace 10. | Agreed |
| SUPER+Shift+1..9 | Move focused window to workspaces 1..9 and follow it. | Accepted numbered-workspace proposal |
| SUPER+Shift+0 | Move focused window to workspace 10 and follow it. | Same proposal, extended to slot 10 |
| SUPER+left mouse drag | Move/reinsert; tiled windows stay tiled. | Agreed |
| SUPER+right mouse drag | Resize tiled split boundaries. | Agreed |
| F11 | Fullscreen may cover the taskbar. | Outcome agreed; binding ownership unresolved |
| SUPER+V | Toggle current window floating/tiled. | Agreed during implementation |
| SUPER+M | Pause/resume window management; pause restores decorations and leaves only custom bindings active. | Latest implementation request; reserved even in custom binding maps |
| SUPER+I | Minimize the focused managed window and reclaim its tile. | Agreed in latest implementation request |
| SUPER+Shift+I | Restore minimized eligible windows on the cursor's monitor in the current workspace. | Agreed in latest implementation request; other workspaces remain separate |
| SUPER+arrows | Directional focus, including across monitors. | Proposed |
| SUPER+Shift+arrows | Rearrange window directionally. | Proposed |
| SUPER+Ctrl+R | Reload configuration. | Proposed |

## Ownership modes

- **Configured combinations:** intercept configured SUPER chords; retain other Windows behavior.
- **Broad override:** standalone Win does nothing; unbound SUPER chords do nothing; configured chords perform commands.

The default mode remains open. Ordinary unmodified typing passes through. Neither mode guarantees interception of protected OS combinations such as Win+L and Ctrl+Alt+Delete.

Keyboard-layout semantics were delegated to implementation judgment. Proposed defaults: active-layout symbols; treat either Win key as SUPER. No separate physical-key feature is required initially.

## Closing semantics

Proposed: request normal window closure so the app can save, prompt, or cancel. Do not terminate its process. Modal prompts must remain interactive, and a hung app must not block Tauiler's controller.

## Fullscreen caveat

F11 is application-defined: Edge uses it for fullscreen while Visual Studio uses it for debugger Step Into. Decide whether to preserve native F11 and offer a separate Tauiler fullscreen shortcut, or deliberately override F11 globally.
