# Feature specifications

## F01: Custom keyboard bindings

**Agreed:** Users can bind SUPER combinations to Tauiler actions and app launches, taking precedence over supported Windows defaults.

Configured combinations are intercepted by default; other Windows behavior remains available. Include an optional broader Win-key override setting, subject to protected-shortcut limits.

**Agreed override behavior:** standalone Win does nothing; unbound SUPER combinations do nothing; Win+Space opens the app launcher. The suppression applies to SUPER chords, not ordinary unmodified typing. Protected OS combinations remain outside the guarantee.

Win+X closes the focused window and overrides the Windows power-user menu. SUPER+1..9 selects workspaces 1..9; SUPER+0 selects workspace 10. Corresponding SUPER+Shift+digit bindings move the focused window. See bindings.md for agreed and proposed defaults.

**Proposed behavior:**

- Parse modifiers and keys into an explicit binding model.
- Support launch, focus, move/swap, resize, floating toggle, fullscreen, workspace, pause, and reload actions as those features are delivered.
- Validate duplicate and unsupported bindings before activating configuration.
- Keep the last valid configuration if a reload fails.
- Consume both relevant press and release events consistently; avoid stuck modifiers and accidental Start menu activation after a handled chord.
- Distinguish intentional key repeats, such as resizing, from one-shot actions such as launching.
- Keep an accessible pause/exit mechanism independent of the configurable command map.

**Open:** Left/right Win distinction and the default state of the optional override mode. Keyboard-layout semantics are delegated to implementation judgment: propose active-layout symbols initially.

**Acceptance criteria:**

- A supported overridden binding performs the Tauiler action without also performing the Windows action.
- Releasing keys after a handled chord leaves subsequent typing and modifier state correct.
- Invalid configuration reports actionable errors without disabling working bindings.
- An unsupported protected shortcut is rejected with an explanation.

## F02: Cursor-directed split tiling

**Agreed:** One managed window fills available space. A second divides it into halves. Further windows divide the tile selected by cursor location.

Available space means each monitor's work area, leaving the taskbar visible. Two monitors are required, with separate layout state per monitor and workspace.

**Agreed:** zero inner and outer gaps. Request the assigned tile dimensions even when an app advertises a larger minimum; do not automatically float merely because of that minimum. Reflow and actual size acceptance are separate from the requested geometry and cannot be promised universally.

**Proposed behavior:**

- Maintain a binary split tree per monitor and workspace.
- Each leaf represents a tiled window; each branch records an axis and ratio.
- Insert into the leaf containing the insertion point. If none is eligible, fall back to the focused tile, then a deterministic tile on the target monitor.
- Start new splits at 50:50. Split wide tiles left/right and tall tiles top/bottom.
- Use cursor position within the target tile to choose the new window's side.
- Capture insertion context when a Tauiler launch command runs. For external launches, capture it when the window becomes eligible.
- Collapse an empty leaf's parent after closure so its sibling receives the freed area.
- Preserve unrelated splits instead of rebuilding the entire tree on every event.

**Agreed refusal policy:** if a window actually resists geometry after bounded attempts, automatically float it and give the tile space back to siblings. Advertised minimum size alone does not trigger this fallback. Do not endlessly retry.

**Open:** Split-axis controls, insertion-context timeout, and exact refusal-detection thresholds.

**Acceptance criteria:**

- With zero gaps, one window fills the selected area; two receive halves.
- Opening a third window over the right tile leaves the left tile unchanged and divides the right tile.
- Closing one of those right-side windows lets its sibling fill the right half.
- Layout rectangles cover the intended area without unintended overlaps or missing pixels due to rounding.
- Advertised minimum sizes do not automatically remove a window from tiling. Actual geometry refusal is detected and handled using the chosen bounded policy, without endless resize attempts.

## F03: Application launching

**Agreed:** Provide a convenient way to launch applications. Search is apps only; general shell commands, files, web search, and plugins are outside the launcher scope.

Win+Space opens the launcher. Selecting an application requests another window even if the app is already running. Use application-specific new-window activation where supported; launching a process alone does not guarantee a new window.

**Proposed behavior:**

- Direct bindings reference an executable or registered application plus structured arguments and an optional working directory.
- A searchable palette supports app names, aliases, and pinned applications.
- Include traditional desktop and packaged applications in the eventual launcher design.
- Separate app activation from window detection: an app may reuse an existing process or window.
- Keep pending insertion requests bounded and expire them; do not assign an unrelated future window to an old launch request.
- Show launch failures and return focus appropriately when the palette is dismissed.
- Exclude the palette itself from tiling.

**Open:** App discovery sources, palette appearance, and behavior when an app only supports a single window. A searchable app launcher is part of the daily-use scope.

**Acceptance criteria:**

- A configured app launches with its arguments preserved.
- A newly eligible app window is tiled at the intended destination.
- Failed or single-instance launches do not leave permanent pending insertion state.
- Dismissing the palette without launching restores the prior interaction context where possible.

## F04: Window eligibility and floating rules

**Agreed:** Popups/dialogs remain ordinary floating windows. Users can toggle apps/windows between tiled and floating behavior. Broad application compatibility is a goal; a browser, Visual Studio, and File Explorer form the initial validation matrix.

The toggle affects only the current window; it does not change other windows of that app or establish a persistent application preference.

**Proposed behavior:**

- Manage eligible top-level application windows.
- Exclude desktop, taskbar, menus, tooltips, hidden/cloaked windows, and Tauiler UI.
- Treat dialogs and transient windows using explicit rules rather than blindly tiling every handle.
- Preserve native popup/dialog ownership, modality, and normal interaction. Do not resize a dialog merely because its parent is tiled.
- Detect dialogs conservatively from ownership, styles, and known application behavior, with rule overrides for ambiguous windows. Do not classify solely by the Win32 popup style.
- Provide rules based on executable identity, window class, and optionally title.
- Allow toggling a window between tiled and floating modes.
- Define precedence when multiple rules match.

**Open:** Matching syntax for automatic classification exceptions; treatment of fixed-size apps, games, and elevated windows; whether per-window state survives a Tauiler restart while the window remains open.

**Acceptance criteria:** Common transient windows do not disrupt the layout; toggling floating reclaims or reinserts space predictably; inaccessible windows do not cause repeated failing operations.

## F05: Focus, movement, resize, and fullscreen

**Agreed:** Focus follows the cursor. Dragging a tiled window keeps it tiled, with Hyprland-like movement. Minimizing managed windows is not an intended user action.

SUPER+left drag moves/reinserts a tiled window at the destination; SUPER+right drag resizes split boundaries. The user accepted the recommended cursor-directed split/reinsertion policy.

Remove all three standard caption buttons (minimize, maximize, close). Win+X requests closure. Scope for dialogs/floating windows and custom-drawn captions remains open. Removing controls does not block every app-driven minimize/maximize path.

F11 fullscreen must still cover the taskbar. App-native versus Tauiler-owned F11 is unresolved. A separate maximize mode was not approved. Proposed fullscreen restoration preserves the window's previous tile placement.

Hover focus pauses during launcher interaction and dragging, respects modal dialogs, and ignores geometry changes beneath a stationary pointer.

**Proposed behavior:**

- Directional commands focus geometric neighbors.
- Separate focus commands from window swapping or movement commands.
- Resize commands adjust split ratios within permitted bounds.
- Distinguish a tile filling its layout area from an explicit fullscreen mode.
- Ignore movement feedback caused by Tauiler's own layout application.
- Apply agreed hover-focus exceptions without silently changing global Windows tracking preferences.
- Provide no Tauiler minimize command. Caption-control removal is required; recovery from externally minimized windows remains open. Do not claim unconditional prevention.

**Open:** Cross-monitor keyboard focus; fullscreen ownership; external-minimize handling; native titlebar/border dragging; floating-window participation in directional focus; caption-button scope.

**Acceptance criteria:** Focus commands use a deterministic neighbor rule; resizing does not corrupt the split tree; repeated self-generated location events settle without continuous relayout.

## F06: Monitors and workspaces

**Agreed:** Support two monitors before daily use, with separate tiling trees per monitor and workspace. Native Windows virtual desktops are a candidate backend. Independent layouts do not by themselves imply independent workspace switching.

Workspace switching changes both monitors together. Preserve each monitor's layout on each desktop.

Require ten directly selectable workspaces; digit 0 maps to workspace 10, not workspace zero. Shifted bindings move the focused window. On monitor disconnection, reinsert affected windows on the remaining monitor, preserving each window's workspace.

**Proposed behavior:**

- Respect per-monitor scaling and negative desktop coordinates.
- Reconcile layouts after display changes or monitor removal.
- Track workspace membership separately from window geometry.

**Open:** Native-desktop integration mechanism, creation/mapping of ten slots, move-follow behavior, and monitor-reconnection policy. Numbered commands remain required despite the integration work. Public API coverage is documented in architecture.md.

**Acceptance criteria:** Mixed scaling does not systematically offset tiles; unplugging a display leaves windows accessible; workspace switching follows the selected model without losing membership.

## F07: Configuration and lifecycle

**Agreed:** Launching Tauiler starts management and adopts already-open eligible windows. Preserve their native workspace membership; dialogs remain floating. "All windows" does not include desktop infrastructure, transient menus, or ineligible objects. Login startup and snapshot recovery were not explicitly approved.

**Proposed behavior:**

- Use a versioned human-readable configuration file; TOML is the initial candidate.
- Validate configuration as a complete candidate before replacing active settings.
- Run one manager instance per interactive user session.
- Provide a tray menu for status, pause/resume, reload, diagnostics, and exit.
- Log operation failures without recording arbitrary typed input.
- Release hooks on normal exit and define how managed windows are restored.

**Open:** Login startup; snapshot persistence; exit/pause restoration; configuration UI timing; installer and update model.

**Acceptance criteria:** Invalid reloads preserve active behavior; pause stops enforcement and releases configured shortcut interception as specified; normal exit leaves the desktop usable.
