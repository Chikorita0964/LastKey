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

Two conventions in `theme.rs` are load-bearing, because both are easy to "fix" back into a defect:

- **Two colour lineages.** The reference styles the DOM with Tailwind classes but hands raw hex
  literals to its icon and canvas components. Tailwind v4 re-specified the scale in oklch, so a class
  and the v3 hex of the same name no longer agree: beside `bg-indigo-600` (`#4f39f6`) sits
  `CanvasIcon color="#4f46e5"`, indigo-600 as v3 defined it. `INDIGO_600` therefore serves painted
  fills and `PRIMARY_TEXT` serves drawn ink; they are deliberately different values and must not be
  merged.
- **Control padding carries the border.** iced lays a button out from `padding` alone — it never adds
  `Border::width` to the box, and strokes the hairline inside those bounds — while the reference sizes
  controls under CSS `box-sizing: border-box`, where padding starts inside the border. Reproducing the
  reference's drawn edge therefore needs `iced_padding = css_padding + border_width`, which is what the
  `+ 1.0` in `BUTTON_PADDING` and its siblings encode. Replacing them with the bare CSS numbers
  (`px-3 py-1.5` → `[6, 12]`) shrinks every bordered control by 2px per axis.
- **A `Fill` child owns the axis it fills.** `rule::vertical` is sized `{ width: thickness, height:
  Length::Fill }`, so inside a row it stretches to that row's height and sets it; a card whose pair
  divider used one measured 150px against the reference's 78px, because a CSS `border-l` is
  content-sized instead. Any hairline or spacer inside a row or column must have an explicit size on
  the axis it is not meant to control — `theme::pair_divider` exists for exactly that. Check a
  widget's `size()` before trusting it as decoration.

- One page starts at 1040×800 with a 960×600 minimum. The header holds branding, connection status,
  profile selection, and engine on/off as compact icon-only controls. Mappings and timing are side
  by side at matched height; timeline, measurement, and results follow in a single body. Only the
  body scrolls; actions stay pinned. Narrow reflow is outside this port. Profile and language menus
  overlay the stable page slot as panels anchored to the top-right below the header — not centered
  modals — preserving scroll position, and profile errors remain visible inside the panel.
- Existing UiView launch/focus requests navigate to the top or bottom of that body. Pre-snapshot
  requests wait until it mounts; ordinary snapshots never reset its scroll offset.
- The D-pad uses capture buttons, highlights duplicate assignments, and shows observed timeline
  holds. Its center tile carries a resting dot that follows the engine's output direction and rests while the timeline shows physical input, arrows are
  tinted per direction, and the unique/duplicate status sits in a footer below the inset. Clicking
  the selected capture again cancels it. While a capture is armed an indigo banner above the stage
  prompts for a new key and offers an explicit ESC cancel; the banner stays static (no pulse). The
  stage itself carries only the reference's "Click keycap to rebind" hint — no modifier note —
  while the engine's modifier exclusion behavior is unchanged.
- TimingField owns editability, draft access, range validation, and buffers. Each duration group has
  a two-handle 0–20 ms rail plus numeric editors supporting the existing 1000 ms ceiling. Both use
  0.1 ms units; release delay retains its 0.1 ms minimum. Values beyond the rail are preserved in the
  numeric editors. Mode changes mount only the groups the mode uses — Immediate mounts the preview,
  Press or Release Delay their own group, Random Mix the ratio and both groups — while the draft
  keeps every hidden value. A group hidden by the mode is unmounted, not disabled, and `is_editable`
  still gates its messages so no hidden control can act.
- The Random Mix slider controls complementary press/release shares shown as one `press : release`
  value; the press share derives from the stored release share, which is the editable side. The
  numeric release share retains the backend's 1–100 percent validation range. Delay decisions shown
  on the timeline come from the engine, never from the displayed share.
- The timing card explains the selected mode as numbered steps with a highlighted final step; the
  block is absent in Random Mix, where the ratio and both groups already fill the card.
- A stopped timeline collapses to its title, subtitle, and start control; the graph, source,
  decision, and scale mount only while recording.
- In Immediate mode, the timing card mounts a separate illustrative preview with three examples:
  Immediate, Press Delay, and Release Delay. Previous/next select examples without changing the
  draft. Random Mix is explained as choosing between the two delay examples; the preview never
  predicts engine randomness or reads monitor output. Four phases advance every 850 ms while
  playing. Phase 1 highlights the configured delay range, including 0 ms for Immediate.
- The preview starts paused on every UI launch: the pinned Iced API exposes no reduced-motion
  preference. Play is explicit, and its widget stops requesting redraws outside the scroll viewport.
  Long labels remain ellipsized at rest and reveal their end on pointer hover, using linear motion
  at 70 px/s clamped to 260–2400 ms. Pointer reversal continues from the displayed offset; content
  or width changes reset it, and off-screen labels do not request animation frames.
- Profile slots include four directional keycap chips. The UI reuses authoritative display names
  available in the current snapshot. Other physical keys display their explicit SC:xx or E0:xx
  scan code because the current wire does not provide inactive-profile key names; no keyboard-layout
  table or platform call is added to the settings process. Complete labels remain in tooltips.
- An untouched numeric editor is a facade button. Its first press focuses and selects the real
  input; subsequent presses place the caret normally. Focus moves rearm this behavior. Invalid
  text or inverted bounds are highlighted only for editable groups.
- Ordinary snapshots merge authoritative bindings/metadata with local timing edits. Apply success
  retains edits made during its round trip. Explicit profile load replaces the whole draft.
- Measurement is opt-in. Reset session clears an idle result or restarts an active generation.
  Recommendations update only the draft and navigate to the top; Apply remains explicit. The
  latency table orders its columns P10, P50, P90 and carries the indistinguishable-input note
  inside that pattern's own row. Recommendation tiles use the same delay names as the timing card
  and keep their hint inside the tile.
- Errors and notices occupy the common action bar so feedback never replaces the scrollable's
  widget position. Before connection they appear in the waiting body. Profile errors remain visible
  inside the dialog. English, Chinese, and Spanish are selectable for the current UI session;
  translations live in their language files and untranslated runtime diagnostics retain their text.
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
