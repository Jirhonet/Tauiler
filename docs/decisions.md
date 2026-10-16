# Decision log

## Agreed

| ID | Decision | Basis |
| --- | --- | --- |
| D01 | Target Windows. | Explicit user goal. |
| D02 | Use Rust for the core. | User accepted the Rust recommendation before requesting specs. |
| D03 | Support configurable SUPER bindings. | Explicit user requirement. |
| D04 | Override supported Windows shortcut defaults. | Explicit user requirement, subject to OS constraints. |
| D05 | Automatically split the tile selected by cursor location. | Explicit user description of desired tiling. |
| D06 | Include convenient application launching. | Explicit user requirement; apps-only scope settled in D11. |
| D07 | Personal use is the initial audience. | Round 1, answer 1. |
| D08 | Intercept configured SUPER combinations by default; include an optional broader Win-key override mode. | Round 1, answer 2; override semantics settled in D21. |
| D09 | Leave the taskbar visible and tile within monitor work areas. | Round 1, answer 3. |
| D10 | Require two monitors with separate tiling state per monitor and workspace. | Round 1, answers 4 and 6; switching semantics settled in D14. |
| D11 | Scope the launcher to applications. | Round 1, answer 5. No general command, file, or web-search palette. |
| D12 | Use a browser, Visual Studio, and File Explorer as the initial compatibility matrix. | Round 1, answer 6. Broad application compatibility remains a goal. |
| D13 | Keep popups/dialogs floating and support toggling windows out of tiling. | Round 1 follow-up; per-window scope settled in D15. |
| D14 | Switch workspaces on both monitors together. | Round 2, answer 7. Native Windows desktops fit this behavior; integration mechanism remains a technical decision. |
| D15 | Floating toggle affects only the selected window. | Round 2, answer 8. No implied persistent app-wide rule. |
| D16 | Focus windows under the cursor. | Round 2, answer 9. Details around dialogs, launcher, and layout-induced pointer crossings need definition. |
| D17 | Dragged tiled windows remain tiled, with Hyprland-like movement. | Round 2, answer 10. Exact drop behavior to clarify. |
| D18 | Minimize is not part of the intended managed-window workflow. | Round 2, answer 11. Enforcement and external-minimize recovery need definition. |
| D19 | Attempt assigned tile dimensions even when an app advertises larger minimum sizes. | Round 2, answer 12; D24 settles actual refusal with automatic floating. |
| D20 | Launcher selection requests another window, rather than focusing an existing window. | Round 2, answer 13. Single-instance application fallback remains unresolved. |
| D21 | In broad Win override mode, standalone Win and unbound SUPER chords do nothing. Win+Space opens the launcher. | Round 2, answer 14. Protected combinations remain excluded. |
| D22 | No inner or outer tiling gaps. | Round 2, answer 15. |
| D23 | Keyboard layout semantics are delegated to implementation judgment. | Round 2, answer 16. Proposed default: active-layout symbols, no additional physical-key feature initially. |
| D24 | Automatically float a window that actually resists assigned geometry after bounded placement attempts. | Round 3, answer 17. Advertised minimum alone does not trigger fallback. Supersedes D19's unresolved refusal policy. |
| D25 | Remove minimize, maximize, and close caption buttons; Win+X closes the focused window. | Round 3, answer 18. Scope for dialogs/custom titlebars and external-minimize handling remain open. |
| D26 | SUPER+left drag moves/reinserts; SUPER+right drag resizes. | Round 3, answer 19 accepted the recommended controls and cursor-directed reinsertion. Resolves D17's drop-policy question. |
| D27 | F11 fullscreen must be able to hide the taskbar. | Round 3, answer 20. App-native versus Tauiler-owned F11 remains open. Separate maximize mode was not approved. |
| D28 | Require ten numbered workspaces: SUPER+1..9 and SUPER+0 for workspace 10, with corresponding SUPER+Shift+digit move-window commands. | Round 3, answer 21 accepted numbered-jump/move proposal and added slot 10. |
| D29 | Pause hover focus for launcher interaction/dragging, respect modal dialogs, and ignore tiles moving under a stationary cursor. | Round 3, answer 22. Resolves D16's interaction questions. |
| D30 | Reinsert disconnected-monitor windows on the remaining monitor, retaining each window's workspace. | Round 3, answer 23. |
| D31 | On launch, start managing and adopt already-open eligible windows. | Round 3, answer 24. Login startup and snapshot recovery were not explicitly approved. |

## Constraints

- Protected combinations cannot all be intercepted by normal keyboard hooks.
- Rust still requires Windows API integration.
- Some windows cannot satisfy arbitrary target geometry or accept every operation.

## Proposals awaiting approval

- Rust module boundaries and event-driven controller described in architecture.md.
- A readable reloadable configuration file, initially TOML.
- Binary split trees with 50:50 insertion and shape-based split direction.
- Capture insertion context at launch time for Tauiler-launched apps.
- A tray interface with pause/resume and exit.
- Staged delivery described in product.md.

## Interview decisions

Rounds 1-3 completed. Answers are recorded as D07-D31; later decisions resolve or supersede earlier open points. Workspace switching is global across monitors. Native Windows desktops are the candidate backend. Direct access to ten numbered workspaces is required; do not silently downgrade to next/previous-only commands because API integration is incomplete.

The user wants every app to work and prefers enforced tile dimensions. Attempt strict placement, then float actual refusals after bounded retries. Do not claim arbitrary apps can be forced to reflow content or accept geometry. The first validation matrix is a browser, Visual Studio, and File Explorer, including dialogs and secondary windows.
