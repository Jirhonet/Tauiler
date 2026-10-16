# Compatibility and acceptance scenarios

Status: required target applications agreed; detailed scenarios proposed. Isolated native checks have passed as recorded in [implementation.md](implementation.md); this application compatibility matrix still requires live execution.

## Goal and boundary

Aim for broad application support. Ordinary top-level windows receive assigned tile geometry even when they advertise larger minimum sizes. Dialogs/popups float; per-window toggles remain available. Detect actual geometry refusal after bounded attempts and float the window automatically. Do not claim universal pixel-perfect tiling or forced content reflow.

The browser executable/version and Visual Studio edition/version must be recorded when integration checks are run. Visual Studio means the IDE, not Visual Studio Code.

## Required environments

- Two monitors, with independent layout trees on each selected workspace.
- Visible taskbar; tile within monitor work areas.
- Multiple workspaces using the backend selected after the interview.
- Mixed DPI and monitor removal as compatibility scenarios; record actual hardware and scaling when tested.

## Application matrix

| Target | Main windows | Floating/transient scenarios |
| --- | --- | --- |
| Browser | Open, split, resize, close, multiple windows, move between monitors. | File upload/download dialogs, authentication windows, browser menus, and tooltips. Distinguish independent browser windows from transient UI. |
| Visual Studio | Solution window, resizing, multiple IDE windows, monitor movement. | Open/save dialogs, options dialogs, modal prompts, splash screen, and detached tool windows. Detached tool-window policy remains open. |
| File Explorer | Folder windows, multiple windows, resize, monitor movement. | Properties, rename/conflict dialogs, file-operation prompts, menus, and tooltips. |

## Cross-application acceptance

- Opening a dialog does not split or shrink its parent's tile.
- Dialog ownership, focus, and modality remain usable, including when the parent changes focus or workspace.
- Explicitly switching a window to floating gives space back to tiled siblings; reinserting follows the chosen insertion policy.
- A floating toggle changes only the selected window; other windows of the same app remain tiled.
- Cursor movement focuses windows without stealing input from the launcher or violating modal dialogs, according to the confirmed focus-exception policy.
- Tiled dragging stays tiled; no gaps are introduced.
- There is no normal minimize workflow; external minimize handling is tested once selected.
- Launcher selection requests a new window; single-instance refusal follows the selected policy.
- Closing, resizing, fullscreen, and exact drag/drop behavior follow the policies selected in subsequent interview rounds.
- Switching workspace restores that workspace's layout on each monitor without mixing window membership.
- Unsupported operations fail without continuous retries or loss of access to the window.
- Verify removal/restoration of standard caption controls and document custom-titlebar exceptions in browser, Visual Studio, and File Explorer.
- Win+X requests closure and respects unsaved-work prompts without also opening the Windows power-user menu.
- All ten workspace shortcuts map correctly; 0 selects slot 10. Move-window bindings preserve correct desktop/monitor membership.
- Native fullscreen hides the taskbar and returns to the expected layout. Test F11 separately in the browser and Visual Studio to expose shortcut conflicts.
- Launching Tauiler adopts already-open eligible windows without moving them between native desktops.
- Apps that reuse an existing process/window do not corrupt pending launch placement.

## Evidence to record

Record Windows build, application versions, privilege levels, monitor geometry/scaling, exact action, observed result, and any app-specific rule required. Separate confirmed support from untested goals.
