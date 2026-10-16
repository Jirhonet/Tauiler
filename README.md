# Tauiler

A Rust tiling manager for Windows 11. Tauiler runs in the system tray, adopts eligible windows, and maintains cursor-directed Dwindle layouts across monitors and native Windows desktops. Win+Space opens Windows Start for now.

## Run

Double-click `dist/tauiler.exe` after building. Keep the DLLs and helper executable beside it. For development, build the complete package first and then run:

```powershell
cargo run -p tauiler --release
```

Starting Tauiler immediately tiles eligible windows on the active desktop, keeping each existing window on its own monitor. Other desktops are adopted when visited; windows already present at startup retain their original monitor even when discovered later. Newly opened windows use the cursor's monitor. A single tiled window on a monitor is natively maximized, leaving the taskbar visible. When another tiled window appears on that monitor, both return to split tiling. Floating and minimized windows do not count as tiles; each monitor and workspace is handled independently. Dialogs and owned/tool windows remain native and floating.

New ordinary app windows use a creation hook to supply their predicted tile coordinates before `WM_NCCREATE`, rather than waiting for a window to settle. A second hook rechecks placement immediately before the first show, covering Explorer and apps that restore saved geometry after creation. It bypasses windows already known to Tauiler. Both x64 and x86 hooks are supplied. Pause, floating/ignore rules, dialogs, tool windows, minimized/maximized creation, and unsupported DPI models bypass the override. Title-dependent rules fall back when the app's title is not available yet. Apps can still change their geometry after creation; normal adoption remains the fallback. This is an experimental compatibility improvement, with no startup debounce or deliberate hiding.

**SUPER+M or a left click on the tray icon pauses/resumes automatic tiling. SUPER+V toggles floating.** The tray icon shows a padded, bold white workspace digit on a rounded square: black when active, red when paused. Workspace 10 displays `0`; its tooltip still says “Workspace 10.” It follows workspace changes made through Tauiler or Windows. SUPER+M is reserved for pause even with a custom binding map. Right-click for “Pause tiling” / “Resume tiling,” reload, open configuration, and “Quit Tauiler completely.” Quit restores Tauiler's caption and stacking changes and releases shortcut interception, leaving windows at their current rectangles. Tauiler-controlled fullscreen ends on exit.

Pause restores window decorations and normal stacking, ends Tauiler-controlled fullscreen, and stops automatic positioning, resizing, gap filling, and focus-following. All Tauiler bindings stay active, including workspace switching, launching, focus, floating, fullscreen, and minimize/restore. While paused, move/resize shortcuts and SUPER-drag affect only the selected window; other windows keep their positions and sizes. Resume reapplies tiling and respects monitor changes made while paused.

Native workspaces use `winvd` 0.0.49, which targets Windows 11 24H2 build 26100.2605 and later. It uses undocumented Windows interfaces and can break after OS updates. Tauiler checks the backend and notification subscription before adopting windows. Set `native_workspaces = false` to run tiling without desktop integration; numbered workspace commands then report that they are unavailable. See the [upstream library](https://github.com/Ciantic/VirtualDesktopAccessor/tree/rust/) for supported builds.

## Shortcuts

SUPER is either Windows key. Default shortcuts:

- **SUPER+Space:** open Windows Start, after releasing SUPER.
- **SUPER+Q:** launch PowerShell 7 (`C:\Program Files\PowerShell\7\pwsh.exe`).
- **SUPER+X:** request normal close, respecting save/cancel dialogs.
- **SUPER+I:** minimize the focused managed window and reclaim its tile.
- **SUPER+Shift+I:** restore all minimized eligible windows on the cursor's monitor in the current workspace, including windows already minimized when Tauiler started. Other monitors and workspaces are left alone.
- **SUPER+V:** toggle the current managed window between floating and tiled; floating windows stay on top.
- **SUPER+M:** pause/resume automatic tiling; all bindings remain active.
- **SUPER+Enter:** toggle borderless, full-monitor fullscreen for a tiled window.
- **SUPER+1..9 / 0:** switch to native desktop 1..9 / 10 on all monitors.
- **SUPER+Shift+1..9 / 0:** move the focused managed window to that desktop and switch to it.
- **SUPER+arrows:** focus a geometric neighbor, including across monitors.
- **SUPER+Shift+arrows:** move/reinsert at the neighbor's tile.
- **SUPER+Ctrl+arrows:** resize the nearest split supporting that direction; holding the key repeats resizing.
- **SUPER+left drag:** move; tiled windows reinsert/split at the drop point.
- **SUPER+right drag:** resize a split, or resize a floating window.
- **SUPER+Ctrl+R:** reload configuration.

F11 stays application-native. Tauiler recognizes borderless native fullscreen and leaves it alone. Only one Tauiler-controlled fullscreen window per monitor is active. Custom/native fullscreen behavior still needs application-specific validation.

Workspace switches focus the eligible window under the stationary cursor as soon as the destination windows are visible and placed. Workspace shortcuts include this focus step while paused; ordinary hover focus remains paused.

Workspaces map to the current Windows desktop order. Missing desktops are created on demand, up to the requested slot. Tauiler does not delete desktops on exit. Existing layout trees are keyed by desktop GUID and monitor device identity, so external switching preserves separate trees.

## Configuration

On first normal startup, Tauiler creates `%APPDATA%\Tauiler\config.toml` from [config.example.toml](config.example.toml). Logs are written to `%APPDATA%\Tauiler\tauiler.log`. You can supply `--config PATH`. Open the file from the tray, edit it, and reload. Invalid reloads retain the last valid configuration. Changing `native_workspaces` requires a restart.

Omitted fields use built-in defaults. `broad_override = false` intercepts configured SUPER chords and retains other Windows shortcuts. Set it to `true` to swallow standalone Win and unbound SUPER chords. Normal typing remains available. Protected combinations such as Win+L and Ctrl+Alt+Delete remain outside Tauiler's control.

Adding `[[bindings]]` **replaces the built-in binding map**, except the reserved SUPER+M pause shortcut. Each entry has a `chord`, `action`, and any action fields. Supported actions: `launcher`, `close`, `minimize`, `restore_minimized`, `toggle_floating`, `fullscreen`, `pause`, `reload`, `exit`, `workspace`/`move_workspace` with `slot`, `focus`/`move`/`resize` with `direction`, and `launch` with `executable` plus optional structured `args`. Accepted keys are ASCII letters/digits, Space, Enter, arrows, Tab, Escape, and F1..F24; modifiers are SUPER, Shift, Ctrl, Alt. Any older custom SUPER+M exit entry must be changed to `action = "pause"`; quitting is always available in the tray menu.

Minimized windows stay minimized and are removed from layout calculations until restored. Restoring preserves their floating choice and workspace membership. The legacy top-level `restore_minimized` configuration flag is still accepted for old files but no longer forces minimized windows to reopen automatically. Exit leaves deliberate minimization intact.

`ignore_classes` excludes exact window classes. Optional `[[rules]]` match an executable name, class, and/or title substring, and select `ignore`, `floating`, or `tile`. Match fields combine with AND; the first matching rule wins. Floating/tile rules apply on initial adoption of a window; explicit per-window toggles persist for the current process lifetime. Ignoring an existing window via reload releases it immediately. Ownership/dialog exclusions always take precedence. Rule changes do not establish restart snapshots.

## Build and checks

Install Rust with the MSVC toolchain and Visual Studio C++ build tools, then:

```powershell
./scripts/build.ps1
```

This formats/checks the source, runs tests and Clippy, builds the release executable, runs isolated native integration checks, and packages `dist/tauiler.exe` plus a portable ZIP. No installer or login startup is enabled.

Read-only/isolated diagnostics:

```powershell
cargo run -p tauiler -- --check
cargo run -p tauiler -- --inspect
cargo run -p tauiler -- --self-test
```

`--inspect` lists monitors, eligible windows, and native desktop availability without adopting them. `--self-test` creates disposable windows and checks hooks/tray setup, desktop backend notifications, native placement, caption restoration, floating, fullscreen, resize coalescing, and exit cleanup. It does not move existing app windows or switch the user's desktop. Do not run it while another Tauiler instance is active.

Building the complete package requires both the `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc` Rust targets and the MSVC x64/x86 libraries. `scripts/build.ps1` builds and checks both hook architectures, runs the isolated cross-process probes, and packages the DLLs/helper. The probe executables are only used by `--self-test`.

## Compatibility and boundaries

Mixed-DPI awareness and DWM visible-frame compensation are implemented. Disconnected-monitor windows are reinserted on a remaining monitor within their existing desktops; reconnecting does not move them back. Geometry corrections are bounded: three failed placement observations separated by at least 250 ms float the window and reclaim its tile. A settling interval prevents brief geometry acceptance from endlessly resetting the refusal budget.

Interactive SUPER resizing keeps the latest pointer coordinate, uses a 16 ms frame interval, caches frame offsets for the drag, and skips unchanged tile requests. Releasing the button immediately flushes the final coordinate. The ordinary geometry-refusal backoff does not delay interactive resize frames; the idle timer remains at 50 ms.

Managed floating windows are made always-on-top without taking focus or changing their rectangle. This applies to explicit toggles, floating rules, and geometry-refusal fallback. Retiling, releasing a window, or exiting restores any topmost change owned by Tauiler; an app that was already topmost retains its own setting.

Standard caption buttons are removed only for tiled windows, with owned style bits restored on orderly release. Custom captions may ignore those flags. Apps at higher privilege levels can reject geometry, styling, focus, and close requests; Tauiler does not auto-elevate. Native titlebar dragging reinserts on release, and border resizing adjusts the relevant split. Geometry enforcement and hover focus pause during user mouse interaction. SUPER dragging follows the cursor using the original drag position, without consuming cursor movement. Cursor focus pauses for native menus, dialogs, launcher interaction, and dragging, and requires actual pointer movement. Windows foreground restrictions can still reject focus requests.

The initial validation record is in [docs/implementation.md](docs/implementation.md). Browser, Visual Studio IDE, File Explorer dialogs, elevated windows, physical shortcut/mouse interaction, and mixed-DPI/disconnection still need live acceptance checks. Automated native tests do not establish universal application compatibility. There is no crash rollback, automatic startup, persistent layout snapshot, graphical binding editor, or custom launcher yet.

The original design specifications are indexed in [docs/README.md](docs/README.md).

