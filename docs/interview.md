# Design interview

Status: rounds 1-3 answered; round 4 awaiting answers. Recommendations below are proposals unless recorded as agreed in decisions.md.

## Decision tree

- Product audience and operating assumptions
  - Supported Windows versions and installation privileges
  - Packaging, updates, configuration UI, compatibility expectations
- Shortcut ownership
  - Standalone Win behavior and passthrough
  - Binding syntax, keyboard layouts, escape controls
- Screen ownership
  - Work area, taskbar, gaps, fullscreen
- Monitor/workspace model
  - Workspace backend and switching semantics
  - Focus and movement across monitors
- Launcher scope
  - Discovery, activation, matching launches to windows
- Minimum daily-use scope
  - Milestone ordering and validation matrix

Later rounds must cover window rules, minimize/restore, dragging, minimum sizes, focus policy, resizing, lifecycle recovery, and measurable performance expectations. Questions that depend on current answers wait for the next round.

## Round 1: answered

Answers: personal use; configured SUPER combinations plus an optional broad override mode; visible taskbar; two monitors with layouts per monitor/workspace; native Windows desktops suggested; apps-only launcher; browser, Visual Studio, and File Explorer as initial validation targets; popups stay floating; apps/windows can be toggled to floating. See decisions.md for the resulting decisions. Original questions follow for context.

### Q1: Audience

Is this initially your personal configurable tool, or a product that nontechnical Windows users should install and understand?

Recommendation: personal power-user tool first, with readable configuration and architecture that permits a later settings UI.

### Q2: Shortcut ownership

Should Tauiler intercept only configured combinations and preserve other Win behavior, or own the Win key broadly, including replacing the Start menu interaction?

Recommendation: consume configured combinations only; retain other Windows behavior initially.

### Q3: Screen ownership

Does filling the screen mean the usable area above the taskbar, or should ordinary tiled windows occupy the entire monitor with the taskbar hidden?

Recommendation: use the work area normally; make full-monitor behavior an explicit fullscreen action.

### Q4: Monitor/workspace model

Which desktop behavior do you want: a single monitor initially, independent workspaces on each monitor, or a workspace switch that changes every monitor together?

Recommendation: independent per-monitor workspaces as the eventual model, with the implementation milestone determined by Q6.

### Q5: Launcher scope

Should the launcher initially search applications and aliases, or also files, web queries, shell commands, and plugins?

Recommendation: applications, aliases, and explicitly configured commands; broader search later.

### Q6: Minimum daily-use scope

What must work before you can use Tauiler daily? Name the non-negotiable workflows, especially monitors, workspaces, app launching, and apps that must tile reliably.

Recommendation: demonstrate launch, tile, focus, close, floating dialogs, and pause/exit with your everyday apps; include multiple monitors immediately if your daily workflow requires them.

## Round 2: answered

Answers: global workspace switching; floating toggle affects the current window only; focus follows cursor; tiled dragging like Hyprland; no normal minimization; force apps toward tile dimensions; launcher requests another window; standalone Win does nothing, Win+Space opens launcher, unbound SUPER chords do nothing in override mode; no gaps; keyboard-layout semantics delegated to implementation judgment. See D14-D23. Original questions follow for context.

### Q7: Workspace switching

Native desktops switch both monitors together. Is this acceptable, with a distinct layout per monitor on every desktop, or must each monitor switch workspaces independently?

Recommendation: native desktops with global switching, subject to the documented API limitations in architecture.md. Do not choose a custom visibility/workspace engine without a requirement for independent switching.

### Q8: Floating toggle scope

Should toggling floating affect only the current window, or all windows of that app, and should the choice survive restart?

Recommendation: temporary per-window toggle plus a separate explicit persistent app rule. Popups/dialogs float automatically.

### Q9: Focus

Should hovering focus a window or only clicking/keyboard commands? Should directional keyboard focus cross monitor boundaries?

Recommendation: click/keyboard focus by default, configurable focus-follows-mouse, and directional focus across monitors.

### Q10: Dragging tiled windows

When you drag a tiled window, should it become floating or remain tiled and be inserted/swapped at the drop destination?

Recommendation: tiled dragging reinserts at the destination; explicit floating toggle is separate.

### Q11: Minimizing

Should minimizing reclaim the tile's space, or preserve an empty slot?

Recommendation: reclaim space, retain workspace/monitor membership, and reinsert on restore. Exact reinsertion policy can follow this choice.

### Q12: Minimum window sizes

When the next split is too small for an app, should it float, overlap, or refuse insertion?

Recommendation: float the incompatible window, leave existing tiles intact, and explain the fallback.

### Q13: Launching an already-running app

Should selecting an app focus an existing window or request a new window?

Recommendation: focus an existing window by default; provide an explicit new-window action for apps that support one. Workspace-switch behavior can follow this choice.

### Q14: Broad Win-key override mode

When enabled, should standalone Win open Tauiler's app launcher, and should unbound SUPER combinations be swallowed?

Recommendation: yes to both in this optional mode, preserving configured combinations and acknowledging protected-key limits.

### Q15: Gaps

Should tiles touch, or have gaps between windows and around monitor edges?

Recommendation: zero gaps initially, configurable inner/outer gaps.

### Q16: Keyboard layout semantics

Should bindings follow printed symbols as keyboard layouts change, or fixed physical key positions?

Recommendation: symbols by default, with optional physical-key bindings for movement.

## Round 3: answered

Answers: float actual placement refusals; remove all three caption buttons; Win+X closes; accept SUPER mouse controls and recommended reinsertion; F11 fullscreen covers taskbar; require numbered workspaces including SUPER+0 for slot 10; accept focus exceptions; migrate disconnected-monitor windows; launch Tauiler and adopt already-open windows. Login startup and restart snapshots were not approved. See D24-D31. Original questions follow for context.

### Q17: Apps resisting geometry

Strict geometry can clip content or be rejected despite aggressive placement. If an app keeps resizing itself back, should Tauiler stop correcting and report the conflict, or automatically float it?

Recommendation: retain intended tile membership, stop after bounded corrections, report incompatibility, and allow the user to float it manually. Do not claim the actual rectangle will always match. Clipped content is accepted as a possible outcome of strict sizing unless the user chooses otherwise.

### Q18: Minimize enforcement

Should Tauiler remove standard minimize controls and restore managed windows that minimize through a taskbar click, shell action, or app behavior, or only omit Tauiler's own minimize action?

Recommendation: remove standard controls where possible and restore/re-tile unexpected minimization while active. Native workspace changes must remain distinct from minimization. Preserve a deliberate pause escape route.

### Q19: Drag/drop and resize controls

Is SUPER+left drag to move/reinsert and SUPER+right drag to resize the desired interaction? When dropped onto a tile, should it split/reinsert there or swap with that tile?

Recommendation: those mouse bindings, with drop causing split/reinsertion using cursor position. This exact drop policy is a Tauiler proposal, not a universal Hyprland claim. Decide native titlebar/border drag handling consistently.

### Q20: Maximize and fullscreen

Should maximize temporarily fill the current monitor's work area and preserve the split tree for restoration? Should explicit fullscreen be allowed to cover the taskbar?

Recommendation: maximize fills the work area and restores the prior tree; explicit fullscreen may cover the taskbar.

### Q21: Workspace commands

Must the first version support direct numbered workspace jumps (SUPER+1 etc.) and moving windows to a numbered workspace, or are previous/next desktop controls sufficient initially?

Recommendation: previous/next native desktop controls first, retaining layout state; direct numbered commands only after validating a dependable integration. This is a scope recommendation, not a claim that numbered integration is impossible.

### Q22: Hover-focus exceptions

May hover focus pause during launcher interaction and dragging, respect modal dialogs, and require real pointer movement rather than focus changes caused by tiles moving beneath a stationary cursor?

Recommendation: yes to all, with cross-monitor hover focus. Keep native menus and dialog interaction usable.

### Q23: Monitor removal

If the second monitor disconnects, should its windows move into the remaining monitor's tiles, or stay separate/floating until it returns?

Recommendation: reinsert into the remaining monitor on each window's existing workspace. Reconnection restoration policy depends on this decision.

### Q24: Startup and restart

Should Tauiler run at login and immediately tile already-open eligible windows? When Tauiler itself restarts, should it attempt to recover layouts and per-window floating choices for windows that remain open?

Recommendation: opt-in login startup, adopt eligible existing windows when enabled, and recover a validated snapshot for still-existing windows; never infer persistent app-wide floating preferences.

## Round 4: current frontier

### Q25: F11 ownership

Should F11 remain application-native, or should Tauiler turn it into universal fullscreen? Visual Studio uses F11 for debugger Step Into, while browsers use it for fullscreen.

Recommendation: preserve native F11 and add SUPER+Enter for Tauiler-controlled fullscreen. Either fullscreen path may cover the taskbar.

### Q26: Caption-button scope

Remove buttons only from tiled main windows, or also from floating windows and dialogs?

Recommendation: tiled main windows only; floating windows/dialogs retain native controls. Custom titlebars require app-specific handling and cannot be guaranteed by standard styles.

### Q27: Unexpected minimization

Beyond removing controls, should Tauiler restore managed windows minimized through taskbar clicks or application behavior?

Recommendation: restore/re-tile unexpectedly minimized tiled windows; leave floating/dialog behavior native and do not confuse workspace hiding with minimization.

### Q28: Move-to-workspace follow behavior

Should SUPER+Shift+digit move the window without changing the current workspace, or also switch both monitors to its destination?

Recommendation: move without following. A separate switch command remains available.

### Q29: Single-window apps

If an app cannot create another window, should the launcher focus its existing one or report that another window is unsupported?

Recommendation: focus the existing window when identifiable, switching to its native desktop if necessary; do not claim a new window was created.

### Q30: Pause and exit

Should pause/exit restore caption controls and release shortcut interception? On exit should app windows remain at their current rectangles or return to their pre-Tauiler positions?

Recommendation: restore controls and release interception on pause/exit; leave current rectangles on exit. Do not promise crash rollback without a recovery mechanism.

### Q31: Monitor reconnection

After windows migrate to the remaining monitor, should reconnecting the old monitor automatically move them back?

Recommendation: keep their current positions/membership rather than unexpectedly moving apps; reconnecting provides an empty destination for subsequent moves/launches.

### Q32: Configuration interface

Is a readable configuration file with reload sufficient for the first usable version, or must there be a graphical keybinding editor?

Recommendation: TOML plus reload and clear error messages initially. Add settings UI later.

Remaining specification review: proposed shortcut defaults, insertion tie-breakers and split controls, launcher presentation/discovery, bounded latency/resource targets, and packaging. Technical backend feasibility is researched by the agent rather than delegated to the user as a question.

## Answer handling

Record answers in decisions.md, revise affected specs, and ask the next unblocked round. Resolve factual API questions through research instead of asking the user to determine platform capabilities. Confirm the shared design before starting implementation.
