# Proposed architecture

Status: proposal, except for the agreed Rust implementation direction.

## Components

- **Domain/layout:** Window identities, split trees, monitor/workspace state, insertion and removal, geometry calculation. Keep this independent of Win32 handles and callbacks where practical.
- **Windows adapter:** Window discovery and metadata, WinEvent subscriptions, keyboard hook, monitor/DPI discovery, positioning, and focus requests.
- **Command controller:** Serialize domain mutations from input commands, window events, configuration changes, and launcher requests.
- **Launcher:** Discover and activate apps; track bounded insertion requests without assuming process creation always produces a new window.
- **Configuration/rules:** Parse, validate, resolve conflicts, and atomically activate settings.
- **Desktop UI:** Tray, launcher, settings, and optional overlays. UI toolkit remains undecided.

Start as one Rust application with clear module boundaries. Split processes only if privilege separation or measured UI behavior warrants it.

## Windows API boundary

Use Microsoft's `windows` or `windows-sys` bindings. Windows API access is required regardless of implementation language.

- `WH_KEYBOARD_LL` for supported shortcut interception.
- `SetWinEventHook` for relevant window lifecycle, focus, and location events.
- Initial enumeration and periodic reconciliation only when needed to recover missed or ambiguous state.
- `SetWindowPos` and the deferred positioning APIs for layout application.
- Monitor work-area and DPI APIs for geometry; DWM frame bounds for visible-border accounting where needed.

Contain `unsafe` operations behind small documented interfaces. Manage hook lifetimes and callback validity explicitly.

## Event handling

1. Native callbacks capture minimal event information.
2. Input callbacks decide synchronously whether an event must be consumed, using prevalidated binding state.
3. Queue commands and observations to the controller; do not perform launching, file access, or layout work inside the keyboard callback.
4. Revalidate window identity and eligibility before mutating state or applying an operation.
5. Calculate desired geometry and apply necessary changes.
6. Reconcile resulting events against expected geometry to prevent feedback loops.

Window handles can be reused. State must distinguish a stale window record from a newly created window that happens to reuse its handle.

## Layout representation

Conceptual node types:

```rust
enum LayoutNode {
    Window(WindowId),
    Split {
        axis: Axis,
        ratio: SplitRatio,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}
```

An empty workspace has no root. Ratios must be validated. Geometry calculation must handle rounding, gaps, and minimum-size policy explicitly.

## Privileges and compatibility

Use the normal interactive user context as the starting assumption. Identify operations that fail across privilege boundaries before selecting elevation or a broker design. Do not assume Rust removes OS security restrictions, or that enabling UIAccess is a general-purpose shortcut.

Native Windows virtual-desktop integration needs separate API support research before committing to it. Avoid silently depending on undocumented interfaces.

Research finding: the documented `IVirtualDesktopManager` exposes only `GetWindowDesktopId`, `IsWindowOnCurrentVirtualDesktop`, and `MoveWindowToDesktop`. It does not expose enumeration, creation, switching, desktop-change notifications, or a direct current-desktop-ID query. Desktop tracking and Tauiler-owned switching commands therefore need a separately validated strategy. This does not establish that no other integration strategy exists.

Native desktops switch across monitors together; independent layout trees are still possible. The explicit multi-monitor evidence below is a Microsoft-hosted Independent Advisor answer, not a normative API contract. If independent monitor workspace switching is required, native desktops alone do not supply the requested model.

The workspace domain model must distinguish a desktop/workspace identity from monitor identity. Layouts are indexed by both; round 2 confirms global workspace switching across monitors. Two-monitor support belongs in the first usable version.

Window classification must preserve floating dialogs and ownership relationships. The user-facing floating toggle affects the current window only. Do not infer or persist an app-wide floating rule from it.

Focus follows cursor movement. Launcher interaction, modal dialogs, active drags, and geometry changes beneath a stationary pointer need explicit arbitration to prevent focus theft.

Apply requested tile geometry without treating advertised minimum sizes as an automatic floating rule. Observe actual geometry and bound correction attempts. Rust/Win32 cannot force arbitrary applications to reflow internal content. Actual refusal now falls back to floating: remove the leaf, reclaim space, and avoid endless reinsertion attempts.

There is no normal minimize command. Decide separately how to handle style controls, shell shortcuts, application-driven minimization, and restoration. UI style changes must be reversible if introduced.

Round 3 requires all three caption buttons removed and Win+X closure. Clearing standard system-menu/minimize/maximize style flags can remove standard caption controls while retaining WS_CAPTION, but also removes the standard icon/system menu. Custom captions may ignore those flags. Save original modified bits, refresh the frame, and restore only owned changes on orderly release; do not overwrite unrelated app style changes. Higher-integrity windows restrict mutations. Dialog/floating scope is pending.

Proposed close implementation: request WM_CLOSE with nonblocking/bounded delivery; permit save prompts and cancellation. Do not terminate the owning process, which may host other windows.

Ten numbered workspace jumps and move commands are required. Keep numeric slot identity distinct from native desktop GUID and monitor ID. Direct switch/create/enumeration support needs validation; previous/next-only commands do not satisfy the agreed scope.

On launch enumerate and adopt eligible existing windows, preserving desktop membership. Distinguish cloaking due to inactive desktops from permanently ineligible windows; defer geometry when needed rather than exposing other desktops. Login startup and snapshot recovery remain open.

F11 is app-defined: Edge fullscreen and Visual Studio debugger Step Into conflict. Fullscreen/taskbar handling must be based on actual window state, not simply any F11 key press. Taskbar integration and app-native fullscreen need validation after deciding binding ownership.

### Enforcement research

- `WM_WINDOWPOSCHANGING` normally allows an app to alter proposed geometry; default processing can consult `WM_GETMINMAXINFO`. `SetWindowPos` supports `SWP_NOSENDCHANGING`, suppressing that pre-change message. This is a candidate for aggressive sizing, not a guarantee of correct app content layout. Validate it per target app before choosing it as the default.
- Clearing `WS_MINIMIZEBOX` can remove the standard minimize button; style updates require appropriate frame refresh. This does not stop app-owned controls or an app calling `ShowWindow` to minimize itself. Higher-integrity windows also restrict style changes.
- Windows supports system-wide active-window tracking. Do not silently modify that persistent global preference. Custom hover activation through `SetForegroundWindow` is subject to foreground restrictions and needs integration validation.
- Official Hyprland mouse-binding examples use SUPER+left drag for movement and SUPER+right drag for resize. They do not establish a universal center-drop-swap rule. Specify Tauiler's exact drop policy explicitly rather than infer it from the phrase "like Hyprland."

## Verification plan

- Unit tests for insertion/removal, split preservation, rounding, and chosen minimum-size policies.
- Input state-machine tests for consumed chords, key-up events, repeats, and configuration changes while keys are held.
- Manual integration checks using representative desktop apps, dialogs, packaged apps, and elevated windows.
- Display checks for mixed DPI, monitor disconnection, and taskbar placement once multi-monitor support is in scope.
- Measure idle CPU, memory, launch-to-placement latency, and command latency before setting performance claims.

## Primary references

- [Rust for Windows](https://learn.microsoft.com/en-us/windows/dev-environment/rust/rust-for-windows)
- [RegisterHotKey limitations](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)
- [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
- [PowerToys shortcut limitations](https://learn.microsoft.com/en-us/windows/powertoys/keyboard-manager)
- [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
- [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)
- [DeferWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-deferwindowpos)
- [UI security boundaries](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)
- [Hyprland Dwindle layout](https://wiki.hypr.land/0.55.0/Configuring/Layouts/Dwindle-Layout/)
- [Documented virtual desktop manager interface](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager)
- [Microsoft-hosted answer on multi-monitor desktop switching](https://learn.microsoft.com/en-us/answers/questions/5526273/how-to-switch-between-only-1-desktop-on-1-monitor)
- [FancyZones desktop/monitor work-area design precedent](https://github.com/microsoft/PowerToys/blob/main/doc/specs/FancyZones-DCR.md)
- [Application minimum/maximum sizing constraints](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-getminmaxinfo)
- [Window position pre-change handling](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-windowposchanging)
- [Window styles including minimize controls](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-styles)
- [Cross-process window style modification](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptra)
- [Application-driven minimization](https://learn.microsoft.com/en-us/windows/win32/gdi/minimized-windows)
- [Foreground activation restrictions](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
- [System active-window tracking](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfoa)
- [Hyprland mouse binding examples](https://wiki.hypr.land/0.54.0/Configuring/Binds/)
- [Custom caption implementation](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)
- [Normal window closing flow](https://learn.microsoft.com/en-us/windows/win32/learnwin32/closing-the-window)
- [Bounded message delivery](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)
- [Edge keyboard shortcuts](https://support.microsoft.com/en-us/edge/keyboard-shortcuts-in-microsoft-edge)
- [Visual Studio keyboard shortcuts](https://learn.microsoft.com/en-us/visualstudio/ide/default-keyboard-shortcuts-in-visual-studio?view=visualstudio)
- [MarkFullscreenWindow taskbar behavior](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-itaskbarlist2-markfullscreenwindow)
