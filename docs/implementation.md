# Implementation and validation

## 0.1.0 — 2026-10-14

The specifications preceded the application. A native Rust executable now implements the tiling manager, including keyboard/mouse hooks, a message-loop controller, tray controls, TOML configuration/reload, direct executable launching, cursor focus, floating toggles, caption changes, fullscreen, and native workspace commands. Win+Space delegates to Windows Start as explicitly requested during implementation.

Provisional implementation choices for earlier open questions: configured-only shortcut ownership by default; native F11 with SUPER+Enter for Tauiler fullscreen; caption changes only for tiled main windows; restore unexpected tiled minimization; move-to-workspace without following; pause/exit restores styles and leaves current rectangles; reconnecting monitors does not move migrated windows back; TOML configuration rather than a settings editor. These are implementation defaults, not retroactively recorded interview approvals.

Native desktops use pinned `winvd` 0.0.49 and its undocumented Windows 11 interfaces. Startup validates current-desktop access and event subscription. Slot identity is current Windows desktop order; layout identity is desktop GUID plus monitor device name. No desktop is deleted by Tauiler. Unsupported backends fail clearly before adoption, rather than silently providing next/previous-only shortcuts. Source: [upstream library](https://github.com/Ciantic/VirtualDesktopAccessor/tree/rust/).

## Executed checks

- Rust 1.99.0 / Cargo 1.99.0, Windows build 26300, normal user context.
- Eleven unit tests covering cursor-targeted splits, collapse, exact pixel coverage, duplicate insertion, bounded resizing, shortcut repeats/releases/pause/reload, SUPER mouse Start masking, configuration defaults, invalid fields/rules, and structured launch arguments.
- `cargo fmt --check` and Clippy with warnings denied.
- The final release build passed the same isolated native self-test and was packaged as `dist/tauiler.exe` and `dist/tauiler-0.1.0-windows-x64.zip`.
- Native enumeration detected two 1920×1080 monitors, each with a 1920×1032 work area, and ten native desktops. No monitor disconnection or mixed scaling was simulated.
- Disposable native windows verified visible-frame placement, standard caption removal/restoration, floating space reclamation and reinsertion, fullscreen and tile restoration, and caption cleanup on pause.
- Real hook/emergency-shortcut/tray registration and cleanup, plus native desktop event subscription creation and teardown, passed during the isolated self-test. This does not prove physical shortcut precedence or delivery of every native desktop event.

The self-test does not adopt existing app windows, change their styles or positions, create native desktops, or switch the user's active desktop.

## Required live acceptance

The browser/Visual Studio/File Explorer matrix in compatibility.md remains outstanding. Record exact app and Windows versions, integrity levels, and scaling. Validate physical SUPER shortcuts and key-up ordering, both mouse drags, launcher selection, native desktop switches/moves, mixed DPI, disconnect/reconnect, native fullscreen/taskbar behavior, custom captions, dialogs/save prompts, and elevated/hung/refusing applications. Measure idle CPU and latency before asserting performance guarantees.

No installer, login startup, snapshots, graphical binding editor, or custom searchable launcher is implemented. A portable binary/package and build script are supplied. Crash rollback and universal caption/focus/geometry enforcement are not promised.

## Dragging correction — 2026-10-14

After a report that dragging fought user input, geometry enforcement now suspends during native mouse interactions and move/size loops. Native titlebar moves reinsert on release; native border resizing adjusts split boundaries. Both WinEvent move/size notifications and mouse-button tracking protect the interaction, including custom captions. SUPER drags pass mouse movement through the hook while consuming the initiating button, and calculate positions from the original rectangle and total pointer displacement. Tiled resizing uses the starting split tree, so reversing a resize does not accumulate rounding/clamping drift.

The rebuilt release passed 13 unit tests, formatting, Clippy, and the native self-test. New disposable-window regressions verify that correction leaves a dragged window untouched and preserves tiled membership on release, and that rapid SUPER movements preserve total displacement. New unit tests cover movement with lagging window updates and reversing a floating resize after hitting its minimum. Physical dragging across target applications still needs live validation. Win32 references: [mouse-hook propagation](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc), [native move/size events](https://learn.microsoft.com/en-us/windows/win32/winauto/event-constants).

## Startup monitor adoption correction — 2026-10-14

Startup captures existing eligible window identities, monitor device names, and insertion positions, including cloaked and minimized windows. This replaces the single first-enumeration flag: existing windows discovered later retain their startup monitor, while newly opened windows use the cursor monitor. If the original display disappears, adoption falls back to the window's current monitor. Window identity is rechecked before using the captured context.

The rebuilt release passed 15 unit tests, formatting, Clippy, and native integration checks. A disposable window on each of the two attached monitors was adopted in separate passes with the insertion pointer fixed on monitor one; both retained their respective monitor and filled its work area. Unit coverage also verifies negative coordinates, new-window cursor placement, and disconnected-monitor fallback. Existing application windows were not rearranged by the checks.

## Shortcut and resizing revision — 2026-10-14

The user changed floating to SUPER+V, requested SUPER+M for quitting, and removed pausing. Pause state/actions, the tray pause item, double-click pause, and Ctrl+Alt+P/Q registration are removed. SUPER+M remains an exit shortcut even with a custom map, and cannot be remapped to another action. Existing pause bindings are rejected with a configuration error. This supersedes the earlier pause defaults recorded above.

Interactive resizing now coalesces pointer positions on a 16 ms frame interval, caches Win32/DWM frame offsets once per affected window, and sends changed tile geometry only. Release flushes the latest coordinate immediately. Ordinary correction/backoff is suspended during interactive resizing. The 50 ms idle timer switches to 16 ms only during SUPER resizing. Keyboard resize accepts repeat events, while launch/close/exit remain one-shot commands.

Nineteen unit tests and isolated native regressions pass. The resize regression holds the frame gate closed during 80 incoming pointer positions, verifies no intermediate placement is sent, then releases and checks both windows against the layout from the final position. Diagonal resizing updates the nearest horizontal and vertical splits independently, rather than discarding one axis. Live application resize latency still needs measurement; this is a behavior regression check, not a performance benchmark.

## Floating stacking revision — 2026-10-14

Managed floating windows now use Win32 topmost stacking for explicit toggles, floating-rule adoption, and actual geometry-refusal fallback. The operation preserves position, size, and focus. Tauiler tracks ownership of its topmost changes, releases them on retiling/removal/exit, and preserves app-owned pre-existing topmost settings. Current-desktop enforcement reinstates topmost stacking if an app clears it; failed operations are logged without constant retries. Source: [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).

Disposable-window regressions check topmost promotion, floating-over-tiled order after raising a tiled window, normal stacking after retiling, preservation of pre-existing topmost state, and release of Tauiler-owned topmost state on exit. Existing app windows are not modified by these checks.

## Numbered tray and tiling-only pause — 2026-10-14

The latest user request replaces SUPER+M exit with a pause/resume toggle, also available by left-clicking the tray icon. The icon displays the current workspace's number in white on an opaque black texture when active, or red when paused. Native desktop notifications update the number even during pause. The icon cache owns and releases native HICON handles; generated icons need no external texture files. Right-click retains exit, configuration, reload, and pause/resume controls.

Pausing guards automatic geometry enforcement only. Input hooks and binding maps remain active, as do launcher processing, native workspace operations, focus, floating topmost enforcement, and explicit move/resize/fullscreen commands. Resuming clears stale geometry-refusal state before enforcing the layout. SUPER+M is reserved for pause even with a custom binding map; custom exit commands can use other chords.

Twenty unit tests cover distinct workspace textures, opaque backgrounds, identical number shapes across pause colors, and the revised shortcut. The native self-test creates and updates all twenty tray variants, validates their HICONs, checks paused geometry is left alone, and verifies launcher and explicit resize operations remain available while paused. Icon creation follows [Win32 icon ownership](https://learn.microsoft.com/en-us/windows/win32/menurc/using-icons); updates use [Shell_NotifyIcon](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicona).

## Tray polish — 2026-10-14

The icon now uses Segoe UI semibold rendered at four times the target resolution with grayscale antialiasing and downsampling. A padded layout keeps the digits away from icon edges; workspace ten uses `0`, while the tooltip retains its actual slot number. Rendering uses installed Windows font resources, with no bundled texture/font dependency. The contact sheet was visually checked in both active and paused colors. See [CreateFontW](https://learn.microsoft.com/windows/win32/api/wingdi/nf-wingdi-createfontw) for the font quality contract.

The right-click menu offers Pause/Resume tiling and “Quit Tauiler completely.” Mouse right-up and context-menu notifications are handled, and menu commands return their selected action without duplicate command notifications. Twenty-one unit tests cover smooth intermediate shades, opaque colors, at least four pixels of padding at 32 px, unique workspace textures, and slot-ten labeling. Native release checks validate all twenty HICON variants and orderly quit cleanup.

## Tray input repair and minimize — 2026-10-15

The previous icon checks did not validate Shell callback delivery. Handling only messages returned by GetMessage missed notifications delivered synchronously to the default window procedure. A dedicated controller window procedure now captures both sent and posted notifications and queues actions for the controller. The icon negotiates notification version 4, decodes the low-word event, accepts modern selection/context-menu events or legacy mouse events as appropriate, and avoids double toggles. Popups are opened outside the callback. Shell restart registration renegotiates the protocol and retains tooltips.

Digits use a heavier font weight. Antialiased transparent rounded corners use premultiplied alpha with a matching monochrome icon mask. Black/red states, padding, and the workspace-ten zero remain intact; the updated contact sheet was visually inspected.

SUPER+I minimizes the focused managed window, removes its layout leaf, and lets siblings reclaim space. SUPER+Shift+I restores eligible minimized windows on the cursor's monitor in the current workspace. Initially minimized windows are tracked on adoption. Monitor/workspace membership and floating choice are preserved; asynchronous minimize/restore requests are tracked until accepted or timed out. Native external restore reinserts a tiled window. Automatic forced restoration is retired; the legacy restore_minimized configuration field is accepted only for compatibility. Quit does not restore deliberately minimized windows. Both new bindings remain usable during tiling pause.

Twenty-two unit tests, formatting, Clippy, and release native checks passed. Native regressions now send and post tray notifications to the real window procedure, observe a native context menu opening and cancel it, verify pause/resume and quit routing, and check minimize space reclamation, monitor-scoped restoration, and adoption of already-minimized windows. Existing application windows are not modified by the checks. Callback layout: [NOTIFYICONDATAW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw).


## Pause releases window management — 2026-10-15

Pause restores owned caption controls and fullscreen frame styles, releases Tauiler-owned topmost state, and blocks every automatic layout application, including applications previously allowed after explicit commands. Focus-following is disabled. Observation and all custom bindings remain active. Paused move/resize commands and SUPER-drag change only the selected window; minimizing or closing a window does not expand its siblings. Explicit fullscreen saves and restores the selected window's free rectangle. Resume honors current monitor membership before reapplying tiling.

Native regressions verify restored decorations, released topmost state, geometry preservation, isolated manual resizing, fullscreen restoration, native dragging without snapping, and launcher availability during pause.


## Launch delay removed — 2026-10-15

The geometry settling experiment was removed after live feedback: its extra wait made startup feel worse. New windows are again adopted immediately, with no quiet-period debounce or post-adoption startup grace.


## Creation-time placement — 2026-10-15

The application now ships architecture-matched x64/x86 CBT-hook DLLs and an x86 message-loop host. `HCBT_CREATEWND` edits the supplied `CREATESTRUCT` coordinates before `WM_NCCREATE`; it never vetoes creation, hides a window, or changes another process's creation style. All notifications continue through `CallNextHookEx`. The DLL does no loader-lock work. See [CBTProc](https://learn.microsoft.com/windows/win32/winmsg/cbtproc) and [SetWindowsHookExW](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwindowshookexw).

The injected callback requests a placement prediction using a fixed-width, pointer-free WM_COPYDATA payload. Only copied payload fields are used; the response travels through temporary window properties. The request has a 15 ms SendMessageTimeout deadline and falls back unchanged if unavailable. The controller validates process/thread identity and uses a separate published snapshot of configuration and layout, so handling synchronous requests cannot borrow the live mutable controller or send blocking window messages back to the app. Prediction snapshots refresh on timer ticks and state changes rather than every pointer packet. The captured insertion point survives into adoption; taskbar positions are clamped to the nearest work-area edge to select the same split.

Only normal, unowned, resizable, per-monitor-DPI-aware windows are overridden. Pause and drag modes disable predictions. Floating/ignore rules, built-in shell/dialog exclusions, fullscreen monitors and missing process metadata bypass the override. Unknown titles conservatively bypass title-dependent rules. Standard invisible frame borders are estimated at the destination monitor's DPI. Unsupported applications and apps which reposition after creation retain normal adoption/correction. Simultaneous launches may receive predictions from the same snapshot and require subsequent layout correction.

RAII owners remove the x64 hook and its DLL reference and stop only their own x86 helper on exit. The helper also monitors the controller window/PID. The portable package must retain the DLLs/helper alongside tauiler.exe. Both native probe binaries are included solely for --self-test.

Separate-process x64/x86 probes record the CREATESTRUCT received by WM_NCCREATE before any ShowWindow. Native checks cover full-monitor and split placement, pause, tool-window, legacy-DPI, floating and ignore bypasses, and restored activation. During these checks the placement service only accepts the disposable probe PID, protecting unrelated application windows. Existing pause/tray/minimize/drag/fullscreen regressions remain in place.


## Explorer first-show placement — 2026-10-15

The creation-only hook did not cover the full launch sequence. Live Explorer probing confirmed that CabinetWClass received a creation hint and then replaced it with saved geometry before showing. A separate-process regression also reproduced a SetWindowPos restore after WM_NCCREATE; HCBT_MOVESIZE was not delivered for those programmatic moves.

Both architectures now also install WH_CALLWNDPROC hooks. Immediately before a normal WM_SHOWWINDOW show notification is dispatched, an eligible unknown window requests its tile again and applies synchronous SetWindowPos on its own thread. The original message is passed on unchanged. There is no subclass callback to leave behind, startup wait, hiding, or extension of automatic tiling into pause mode. Snapshots include known windows to exclude already-managed or startup windows. The original creation insertion point is reused where available. The protocol version was increased for explicit creation/show phases.

Native x64/x86 cases include an app restoring geometry between creation and first show and an app which only becomes resizable after creation. An opt-in real Explorer check (`TAUILER_EXPLORER_PROBE=wine`) opens Explorer through WIN+E, samples its first visible native rectangle, verifies it remains on the intended tile, and closes only the newly opened test window. The normal integration suite uses disposable probe apps only. The show-message approach still cannot cover every application: Windows omits WM_SHOWWINDOW for certain show modes, and apps may reposition after they are visible. See [WM_SHOWWINDOW](https://learn.microsoft.com/windows/win32/winmsg/wm-showwindow) and [CallWndProc](https://learn.microsoft.com/windows/win32/winmsg/callwndproc).


## Follow moved windows — 2026-10-15

SUPER+Shift+1..9 / 0 now moves the focused managed window and then switches to that destination desktop. The switch runs only after a successful move; invalid/unmanaged focused windows retain their existing no-op behavior. Custom move_workspace bindings use the same move-and-follow behavior, including while tiling is paused.


## Native maximize for a lone tile — 2026-10-16

A monitor/workspace layout with one tiled leaf now uses native maximization through asynchronous WINDOWPLACEMENT, preserving the app's normal restore rectangle. The native maximize capability is retained for a lone tile so Windows calculates work-area bounds rather than covering the taskbar. Apps without that capability keep the existing full-work-area tiling behavior. The window is first moved to its assigned monitor when necessary. Adding or restoring a second tiled window clears native maximization and restores the split layout. Floating/minimized windows are excluded, fullscreen remains distinct, and pause leaves native window management alone. Explicit floating restores an automatically maximized tile to its normal state.

Native regression fixtures use actual monitor work areas and check IsZoomed for the last tile after minimizing a sibling, normal split states after restoring it, and independent maximization on two monitors.


## Stationary cursor focus on workspace switches — 2026-10-16

Desktop changes schedule a one-shot focus request independently of cursor movement. The request waits for the destination desktop and cursor monitor's visible layout, retries while the shell transition or foreground policy blocks focus, and expires after two seconds without adding a settling delay. Known menu, capture, dialog and drag guards remain in force. Native workspace changes honor focus_follows_cursor and pause. Explicit workspace/move-and-follow shortcuts focus their destination as part of the command, including while paused. Successful focus clears the request so later keyboard focus choices are not overwritten.

A native regression uses two disposable windows and a stationary real cursor to check active and explicitly paused focus requests, plus paused implicit-focus suppression. It never switches actual desktops or reconciles existing application windows. A deterministic state check covers destination matching, configuration/pause rules and expiry.
