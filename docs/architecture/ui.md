# Settings UI Contracts

Everything the settings process owns: composition, rendering rules, localization, and the UI
contracts a review has already hardened. Backend contracts live in `engine.md`.

Design source: the user-approved local React reference at
C:/Users/Administrator/Desktop/UI_improvement/src/App.tsx (outside the product). The pinned Iced
revision (f8127c8) supplies system-font widgets, the advanced quad renderer, and `canvas`.

Rendering rules, stated as what to use rather than only what to avoid:

- Shapes a quad cannot express — arcs, beziers, diagonal strokes — are drawn with
  `iced::widget::canvas` (`Path`, `Stroke`, `Fill`). The icon set is the case that forced this: the
  reference procedurally draws 24 icons, and 20 of them need an arc, a bezier, or a diagonal.
- Everything a rectangle can express stays on `fill_quad` through a custom `Widget`, because
  `renderer::Quad` already carries border width, color, and radius. `src/ui/widgets.rs` (logo, range
  slider) and `src/ui/timeline.rs` (lanes, intervals, grid, needle) are all quads and stay that way.
  Porting working quad code to `canvas` is churn, not an improvement.
- A `canvas` that changes color with interaction keys its `Cache` on the drawn color, not only on the
  geometry. The reference recolors icons on hover and focus; a cache keyed on shape alone keeps
  painting the old color, and no cache at all retessellates every frame.
- A widget that animates drives its own repaint from `Event::Window(RedrawRequested)` plus
  `shell.request_redraw_at`, as `src/ui/timeline.rs:165` does, and only while it has something to
  show. No animation engine, and no repaint loop that outlives the state it renders.
- Text uses generic font families, never a named one. `UI_FONT` is `Font::DEFAULT`
  (`Family::SansSerif`) and `MONO_FONT` is `Font::MONOSPACE`, so the shaper resolves the OS default
  and then walks its own fallback chain for glyphs that face lacks — Hangul and CJK included. A named
  family (`Font::new("Segoe UI")`) pins a face that is absent on other targets and renders tofu.
  Canvas text takes the same rule.
- Still excluded: icon fonts (glyph coverage differs per OS) and runtime image decoding (`png` stays
  in `[build-dependencies]`).
- UI strings live in `src/ui/language/`, one file per language including `en.rs`. Each exports
  `text(&str) -> Option<&'static str>` keyed by the English source string, and `en.rs` maps every
  string to itself so it is the inventory a new language is diffed against. A missing entry falls
  back to the English source, so a partial translation renders rather than blanking. Runtime
  diagnostics and user-supplied profile names are never translated.

Enabling the `canvas` feature adds `lyon` tessellation to the **settings binary only**. `iced-ui` is
off by default, so the resident runtime's dependency graph is unchanged and the
`cargo tree --no-default-features` check in `verification.md` still has to come back free of Iced, wgpu,
and lyon.

Palette, styling, and metrics live in src/ui/theme.rs; the application and edit state live in
src/ui/app.rs; native range/logo widgets and the bounded timeline have their own rendering owners.

- One page starts at 1040×800 with a 960×600 minimum. The header holds branding, connection status,
  profile selection, and engine on/off. Mappings and timing are side by side; timeline, measurement,
  and results follow in a single body. Only the body scrolls; actions stay pinned. Narrow reflow is
  outside this port. Profile dialogs overlay the stable page slot, preserving scroll position.
- Existing UiView launch/focus requests navigate to the top or bottom of that body. Pre-snapshot
  requests wait until it mounts; ordinary snapshots never reset its scroll offset.
- The D-pad uses capture buttons, highlights duplicate assignments, and shows observed timeline
  holds. Clicking the selected capture again cancels it. Modifier exclusions stay unchanged.
- TimingField owns editability, draft access, range validation, and buffers. Each duration group has
  a two-handle 0–20 ms rail plus numeric editors supporting the existing 1000 ms ceiling. Both use
  0.1 ms units; release delay retains its 0.1 ms minimum. Values beyond the rail are preserved in the
  numeric editors. Mode changes keep unused groups visible and muted without discarding values.
- The Random Mix slider controls complementary press/release shares. The numeric release share
  retains the backend's 1–100 percent validation range. Delay decisions shown on the timeline come
  from the engine, never from the displayed share.
- The timing card carries the reference's mode preview: a looping demonstration of the selected
  mode with play/pause, previous/next to step through the modes, the two keycaps lighting per phase,
  a badge showing the configured range for that mode, and a state line naming what the game receives.
  It is in scope. `canvas` and self-driven repaint cover it; the earlier blanket ban on animation is
  what kept it out, and that ban now reads as "no animation engine", not "no motion". Start paused
  when the OS asks for reduced motion, and stop the repaint while the card is off screen.
- An untouched numeric editor is a facade button. Its first press focuses and selects the real
  input; subsequent presses place the caret normally. Focus moves rearm this behavior. Invalid
  text or inverted bounds are highlighted only for editable groups.
- Ordinary snapshots merge authoritative bindings/metadata with local timing edits. Apply success
  retains edits made during its round trip. Explicit profile load replaces the whole draft.
- Measurement is opt-in. Reset session clears an idle result or restarts an active generation.
  Recommendations update only the draft and navigate to the top; Apply remains explicit.
- Errors and notices occupy the common action bar so feedback never replaces the scrollable's
  widget position. Before connection they appear in the waiting body. Profile errors remain visible
  inside the dialog. English is the current UI language; the reference's zh/es dictionaries are
  English placeholders and no nonfunctional selector is presented.
- Under iced-ui, build.rs unpacks the 32×32 PNG layer from
  assets/icons/ico/socd-light.ico for both the header logo and native window icon. Asset decode
  failure aborts the build; an invalid runtime pixel buffer yields no native icon.

Acceptance covers the composed page, two-handle timing controls, explicit capture, four profiles,
confirmed engine toggle, bounded timeline, and measurement actions. Tests and executable builds
cover behavior and compilation; the native visual scope is recorded in Unverified Areas.

## Do Not Regress

Each item looks removable until its reason is known. Changing one requires a replacement test that fails against the old behavior.

| Contract | Why it exists |
| --- | --- |
| Timing is locally authoritative until Apply; resets apply locally at click time | Nothing correlates a reply with its request, so an older Snapshot must not undo a revert; every Snapshot merges local timing while bindings converge on the last reply |
| Uncommitted text buffers count toward the dirty state | Typed text reaches the draft only on submit, so comparing drafts alone left Apply disabled — and the click dead — after typing into a clean window |
| Apply passes `draft.validate()` locally before any IPC | Turns a round-trip `ValidationFailed` into immediate feedback while the server stays the authoritative gate |
| A successful submit clears only the parse error it produced | The bottom bar is the only place a runtime failure is shown; clearing every error would hide a server message on an unrelated keystroke |
| Settings and measurement share one pinned feedback bar | Measurement updates must not discard Apply feedback or move the action buttons. Recommendations update only the draft and scroll to settings while keeping their guidance visible |
| The timeline renders `MonitorEvent` only, never a front-end simulation of the filter | Delays come from a per-overlap random range the engine owns. Any UI-side reproduction diverges from what the game actually received, which is the one thing this view exists to show |
| The on/off button sends `SetFilterEnabled(!current)` and waits for `FilterChanged` | The engine is authoritative. An optimistic flip displays a state the runtime may have rejected, and the label then disagrees with the tray menu |
| `FilterChanged` and `Snapshot` clear held output state (`MonitorState::resynchronize`) | Apply and filter toggles deliberately do not stream their lifecycle output as monitor events, so held lanes would stay lit forever without this |
