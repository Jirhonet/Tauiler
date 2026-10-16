# Product and scope

## Agreed direction

Build a Windows tiling manager in Rust with:

- Configurable keyboard bindings using SUPER, mapped to the Windows key.
- Precedence over Windows shortcut behavior where technically supported.
- Convenient application launching.
- Automatic tiling that fills available space and recursively splits the tile under the cursor as windows are added.

Rust was selected for the background engine, native Windows access, and control over resource use. The exact UI technology is undecided.

Round 1 established personal use, two monitors in the first usable version, a visible taskbar, and an app-only launcher. Configured SUPER combinations are intercepted by default; an optional broader Win-key override setting is also required. Popups/dialogs remain floating, and users can toggle apps/windows into floating mode. Windows virtual desktops are a candidate workspace backend pending clarification and API research.

Round 2 established global workspace switching, per-window floating toggles, cursor-driven focus, tiled dragging, no gaps, and no normal minimize workflow. Launcher selection requests a new window. Win+Space opens the launcher; in broad override mode, standalone Win and unbound SUPER chords do nothing. Apps should be fitted to their tiles rather than automatically floated for advertised minimum sizes; actual geometry refusal needs a bounded policy.

Round 3 chose floating after actual geometry refusal, removing all three caption buttons, Win+X close, SUPER mouse move/resize, taskbar-covering F11 fullscreen, and ten numbered workspace shortcuts (1..9, then 0 for workspace 10). Hover-focus exceptions and disconnected-monitor migration were accepted. On launch, Tauiler adopts existing eligible windows; login startup and restart snapshots remain undecided.

## Proposed product principles

- Keep keyboard and window handling responsive without continuous polling.
- Make behavior predictable across app launches, window closure, and manual movement.
- Provide explicit floating exceptions for windows that should not tile.
- Keep configuration readable and reloadable.
- Explain unsupported shortcuts and failed window operations.
- Make pause and exit readily accessible during development and normal use.

## Constraints

Tauiler operates through Windows APIs alongside the existing desktop. It is not a replacement compositor.

Normal low-level keyboard hooks cannot override all protected Windows combinations. Win+L and Ctrl+Alt+Delete must not be advertised as supported custom bindings.

Some applications impose minimum dimensions, reposition themselves, or run at a higher privilege level. Pixel-perfect control over every application is not guaranteed.

## Proposed delivery stages

1. **Core proof:** two-monitor discovery and separate layout state, automatic split tiling within work areas, configurable shortcuts, direct app launching, floating dialogs/toggle, and pause/exit controls.
2. **Daily use:** searchable app launcher, chosen workspace model, configuration reload, application rules, directional focus/movement, and split resizing. Validate a browser, Visual Studio, and File Explorer across both monitors.
3. **Desktop integration:** display changes, startup integration, and recovery behavior.
4. **Polish:** settings UI, optional decorations and animations, packaging, and diagnostics improvements.

Stage order remains provisional, but two-monitor support is required before daily use. Broad app compatibility is a goal; unsupported geometry must have a usable fallback rather than a promise of perfect tiling for every window.

## Proposed exclusions from the first release

- Kernel drivers or attempts to bypass protected input sequences.
- Replacing the Windows shell or compositor.
- A general plugin marketplace.
- Compatibility promises for exclusive-fullscreen games or secure desktops.

## Open product questions

- What happens when a window minimizes itself or an app cannot create another window?
- Do caption changes also apply to dialogs/floating windows? Is F11 native or overridden?
- How should ten numbered native workspaces integrate without silently relying on unstable APIs?
- What should pause/exit restore, and what settings UI is needed?
