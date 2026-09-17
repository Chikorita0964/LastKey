# Settings UI Contracts

Everything the settings process owns: composition, rendering rules, localization, and the UI
contracts a review has already hardened. Backend contracts live in `engine.md`.

Design source: the user-approved local React reference at
C:/Users/Administrator/Desktop/UI_improvement/src/App.tsx (outside the product). The Iced
implementation this window was first ported to (pinned revision f8127c8) was retired in T11; the
rendering rationale below is kept because the ported values and rules derive from it.

Rendering rules, stated as what to use rather than only what to avoid:

- Shapes a rectangle cannot express — arcs, beziers, diagonal strokes — are drawn as egui paths
  (`egui::Shape::line` with joins, `convex_polygon`, `CubicBezier`). The icon set is the case that
  forced this: the reference procedurally draws 24 icons, and 20 of them need an arc, a bezier, or a
  diagonal.
- Everything a rectangle can express stays a rect fill or stroke, because a rounded rectangle
  already carries border width, color, and radius. The timeline's lanes, intervals, grid, and needle
  (`src/ui/timeline.rs`) and the timing card's range rail, thumbs, and mixer slider are all rects and
  paths and stay that way. Converting working rect code to a general path is churn, not an improvement.
- (Iced-era note) A `canvas` that changes color with interaction keyed its `Cache` on the drawn color,
  not only the geometry, because the reference recolors icons on hover and focus. The egui port
  records shapes from the current color every frame, so that rule holds by construction.
- A widget that animates drives its own repaint (`egui::Context::request_repaint_after`) and only
  while it has something to show, as the timeline playhead and the preview clock do. No animation
  engine, and no repaint loop that outlives the state it renders.
- A deactivated window is an additional "nothing to show" condition, so it requests no frames at all:
  the preview clock, the timeline playhead, and hover-reveal motion all stop, and the settings IPC
  pump drops to `IPC_SLEEP_POLL_INTERVAL`. Only the settings process idles. The filter engine lives
  in the runtime process and keeps resolving SOCD while the window sleeps, so nothing here may gate
  filtering, the engine's monitor tap, or the tray. Focus restores the previous behavior, and a
  command queued on the way out still wakes the pump immediately.
- Text uses generic font families at call sites, never a named one: `theme::UI_FONT` is
  `FontFamily::Proportional` and `theme::MONO_FONT` is `FontFamily::Monospace`. The face those
  families resolve is owned by `theme::fonts()` (F24): it registers the native Windows UI face
  (Segoe UI, read from `%WINDIR%\Fonts\segoeui.ttf`) at the head of `FontFamily::Proportional` and
  keeps eframe's bundled `default_fonts` faces behind it, which epaint walks in order as the
  per-glyph fallback — Hangul, CJK, and emoji included. A missing or unreadable system file leaves
  the bundled set untouched, so the window still renders where Segoe UI is absent. A named family
  (`FontFamily::Name(..)`) pins a face that is absent on other targets and renders tofu; canvas
  text takes the same generic-family rule. Bold emphasis remains the double-stamp approximation
  (`theme::stamp_galley`) until a weighted family has call sites: egui selects a face by family,
  not by weight, so loading a bold file alone changes nothing.
- Still excluded: icon fonts (glyph coverage differs per OS) and runtime image decoding (`png` stays
  in `[build-dependencies]`).
- UI strings live in `src/ui/language/`, one file per language including `en.rs`. Each exports
  `text(&str) -> Option<&'static str>` keyed by the English source string, and `en.rs` maps every
  string to itself so it is the inventory a new language is diffed against. A missing entry falls
  back to the English source, so a partial translation renders rather than blanking. Runtime
  diagnostics and user-supplied profile names are never translated.

The `egui-ui` feature gates the **settings binary only** and is off by default, so the resident
runtime's dependency graph is unchanged and the `cargo tree --no-default-features` check in
`verification.md` still has to come back free of egui, eframe, and wgpu.

Palette, styling, and metrics live in `src/ui/theme.rs`; the application loop and edit state live in
`src/ui/app.rs` and `src/ui/state.rs`; the range, logo, and timeline drawing have their own owners
(`src/ui/timing.rs`, `src/ui/timeline.rs`, and the header module).

The conventions below were derived against the retired Iced implementation; the cited Iced mechanics
are historical, kept because they explain why the ported values are what they are. Two of them are
load-bearing, because both are easy to "fix" back into a defect:

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
- **A reference-matched tint is pre-composited, never handed to iced with an alpha.** A browser
  blends alpha in sRGB; iced blends in linear space, so the same value lands visibly paler — the
  Immediate slot card measured `#CED1F6` where the reference draws `#EBEEFD`. `theme::slot_tint`
  composites each layer in sRGB (`over`, `dim`) and returns an opaque colour instead, so the
  difference is removed rather than compensated for. A card's edge composites over its own fill, not
  over the panel behind it, because a CSS `border-box` background reaches under its border.
- **A content-sized widget must report `Shrink`, not `Fill`, from `size()`.** A row hands every child
  the same loose limits, and `Limits::resolve_width` maps `Fill` to `max.width`, so a custom widget
  that declares `Fill` while sizing itself from its content silently claims the whole row. The
  clipped profile-name label did this: it measured 336px inside a 336px card interior and pushed its
  trailing pencil to the far edge, where the reference's box is 135px. `hover_text::Label` now reports
  `Length::Shrink` and resolves to `state.full.min_width()`; the incoming `max` still clamps, so an
  over-long name still truncates and still scrolls on hover. Squaring the keycap chips uses the mirror
  image of the same rule: their width is `Length::Shrink.min(theme::CHIP_CONTENT_MIN)`.
- **Padding is added outside a resolved length, and a container aligns content Left/Top by default.**
  iced's `layout::positioned` resolves the explicit size first and then expands it by the padding, so
  `.width(Length::X)` is the *content* width and the drawn box is `X + padding.left + padding.right`
  — `theme::CHIP_CONTENT_MIN` is the square minus both horizontal insets for that reason. `Container`
  defaults to `Horizontal::Left` / `Vertical::Top` and centres only when asked. The profile keycap
  relies on the combination: its insets are symmetric, so the label's own box equals the content box
  and cannot sit anywhere but centred, which is why it needs no `align_x`/`align_y` at all.
- **The whole interface is rebuilt and re-laid out after every message, so content-sized widgets
  resize as they are edited.** The winit runner builds a fresh `UserInterface` whenever messages are
  pending, and `build_user_interfaces` lays out from scratch. The rename box exploits this rather than
  fighting it: it is `Length::Shrink` and replaces the name box in place, so it starts at the name
  box's own width and extends to the right as the name is typed. That is a deliberate departure from
  the reference, which pins its field to a fixed `w-28` — the owner asked for the box to grow
  (`글자 크기가 늘어나면 입력창도 같이 우측으로 길어지게`). Its left padding matches
  `theme::SLOT_NAME_PADDING`'s, so the name does not shift when the box becomes editable.

- One page starts at 1040×800 with a 960×600 minimum. The header holds branding, connection status,
  profile selection, and engine on/off as compact icon-only controls. Mappings and timing are side
  by side at matched height; timeline, measurement, and results follow in a single body. Only the
  body scrolls, and the header and the action bar sit outside that one scroll owner in the page's
  vertical flow, separated from the body by `theme::SECTION_GAP`. Both keep the card chrome (rounded
  `theme::CARD_RADIUS`, the 2px card border, and the card shadow) inset by `theme::PAGE_PADDING`
  from the page edge, and the body reserves the action bar's full frame (`theme::SECTION_GAP` plus
  `ACTION_BAR_HEIGHT`), so the bar's margin survives. The body clips its content at its own viewport
  edge: a scrolled card is clipped there rather than sliding behind the bars. Narrow reflow is
  outside this port. Profile and language menus overlay the stable page slot as panels anchored to
  the top-right below the header — not centered modals — preserving scroll position, and profile
  errors remain visible inside the panel.
- Existing UiView launch/focus requests navigate to the top or bottom of that body. Pre-snapshot
  requests wait until it mounts; ordinary snapshots never reset its scroll offset.
- The Key mappings card groups its title and subtitle tightly in a column beside the Restore button.
  Direction accents match the four SOCD mode colors (UP = Immediate, LEFT = Press Delay, RIGHT = Random Mix,
  DOWN = Release Delay). Keycaps distinguish rebinding (accent fill, 8px outer glow ring, "..." in black font)
  from live physical keypresses (accent fill, pressed shadow, key name in bold white). Long and compound key
  names (e.g., "Numpad 8", "Arrow Up", "Backspace") are automatically split into two auto-scaled lines.
  The center D-pad joystick is drawn on an 80×80 canvas with rounded-2xl corners, a 48px dashed guide ring,
  an 8px resting guide dot, and a dynamic moving dot (18px cardinal, 13px diagonal) with active accent glow,
  resolving real-time opposite inputs via Last-Input-Priority SOCD. While capture is armed, an indigo banner
  prompts for input with a high-contrast `INDIGO_700`/`INDIGO_800` ESC Cancel button. Unique/duplicate status
  sits in the card footer with bold iconography and typography.
  Keycap and D-pad feedback reads the window's own key events, so it reacts whether or not the
  timeline is recording — matching the reference, which animates keycaps with its timeline switch
  off. Recording adds the timeline's held state on top; it never gates the feedback. Deactivating the
  window clears the pressed set, because a key held across the transition never delivers its release.
- TimingField owns editability, draft access, range validation, and buffers. Each duration group has
  a two-handle 0–20 ms rail plus numeric editors supporting the existing 1000 ms ceiling. Both use
  0.1 ms units; release delay retains its 0.1 ms minimum. Values beyond the rail are preserved in the
  numeric editors. Mode changes mount only the groups the mode uses — Immediate mounts the preview,
  Press or Release Delay their own group, Random Mix the ratio and both groups — while the draft
  keeps every hidden value. A group hidden by the mode is unmounted, not disabled, and `is_editable`
  still gates its messages so no hidden control can act.
- The Random Mix ratio controls complementary press/release shares shown as one `press : release`
  pill. The stored release share is the single value both numeric editors and the mixer slider
  write: the release box edits it directly, the press box edits its complement, and committing
  either refreshes the other box. The editors retain the backend's percent validation range.
  Delay decisions shown on the timeline come from the engine, never from the displayed share.
- The timing card explains the selected mode as numbered steps with a highlighted final step; the
  block is absent in Random Mix, where the ratio and both groups already fill the card.
- A stopped timeline collapses to its title, subtitle, and start control; the graph, source,
  decision, and scale mount only while recording.
- In Immediate mode, the timing card mounts a separate illustrative preview with three examples:
  Immediate, Press Delay, and Release Delay. Previous/next select examples without changing the
  draft. Random Mix is explained as choosing between the two delay examples; the preview never
  predicts engine randomness or reads monitor output. Four phases advance every 850 ms while
  playing. Phase 1 highlights the configured delay range, including 0 ms for Immediate.
- The preview starts paused on every UI launch: the port has no reduced-motion preference to
  consult. Play is explicit, and its widget stops requesting redraws outside the scroll viewport.
  Long labels remain ellipsized at rest and reveal their end on pointer hover, using linear motion
  at 70 px/s clamped to 260–2400 ms. Pointer reversal continues from the displayed offset; content
  or width changes reset it, and off-screen labels do not request animation frames.
- Profile slots include four directional keycap chips. The UI reuses authoritative display names
  available in the current snapshot. Every other physical key resolves through the platform
  key-name resolver (`platform::windows::physical_key_name`), the same function the runtime fills
  the wire names with, so a key held only by an inactive profile still reads as a key name; the
  port adds no keyboard-layout table of its own. The resolved name is the chip's accessible label.
- A slot card paints from two inputs: its mode and its interaction state
  (`theme::{SlotState, slot_tint, slot_ink, slot_mode_ink}`). The loaded slot always draws active;
  otherwise the card under the pointer draws hovered and the rest draw idle, which is the
  reference's `opacity-75` applied to the whole card — wash, name, keycaps and hairline alike. The
  active class has no hover variant, so the loaded slot does not change under the pointer. The
  state is derived in `SettingsApp::slot_state` from `hovered_slot`, which the card's `mouse_area`
  sets: a `container` carries no interaction status, so a card's hover cannot be styled the way a
  button's can. Every cell of an inactive card is the load target, as it is in the reference, where
  only the name box excludes itself; the keycap row draws no chrome of its own, because the card's
  hover is the whole affordance. A card's press is published when it happens rather than when it is
  released, so that a press which both commits an open rename and selects the card under the pointer
  acts on one card. That is also why the keycap row is no longer a `button`: a press target and a
  release target on one card cannot coexist, since the press can change what the pointer is over
  before the release arrives.
- The name box carries a hover of its own, which the card does not: its fill takes
  `theme::NAME_HOVER_FILL`, its edge `theme::NAME_HOVER_BORDER`, and its name and pencil both take
  `INDIGO_600`, an idle card's pencil included — the reference's `group-hover/slot` is one class at
  full strength, so it replaces the idle dim rather than stacking on it. This is the one part of a
  card whose ink is not a function of the card's state alone, and it lives in
  `SettingsApp::hovered_name`, fed by a `mouse_area` around the box. It cannot live in the button's
  style: `Button::draw` forwards a single `text_color` to every child, and the box needs two inks at
  rest (name and pencil) and one on hover, which no one colour can express.
- Profile slot keycaps use `theme::CHIP_FONT`, the monospace face the reference puts on every
  `<kbd>`. A proportional bold renders W/S/A/D at different widths and widens the pair row past the
  reference's. Each chip is square (`theme::CHIP_SIZE`, `rounded-md`, radius 6) and the label is
  centred on both axes; the reference's chip is wider than it is tall. The height is fixed on every
  chip while the width is a floor, so a long key name grows only its own chip sideways and no
  label can make the row taller. Squaring the chip is +4px on the card, so
  `theme::SLOT_ROW_GAP` takes 2 of them (10 → 8) and the card is 80px rather than the reference's 78.
- The slot name box and the box that replaces it while renaming are one control in two states. The
  rename field keeps the name box's fill, radius and left inset — measured at the same `x` — so only
  the edge changes, to `theme::INDIGO_400`, which is how the reference marks editing. The field is
  `Length::Shrink`, so the box starts at the name's own width and extends rightward as the name is
  typed, and the pencil is dropped while it is open, because the field is now the control. This is
  the one place the port departs from the reference, which pins the field to a fixed `w-28`.
  `Message::EditProfileName` focuses and selects the field, never `Message::ProfileNameChanged`, so a
  click while typing does not pull the caret back to the end.
- An open rename ends on any press that reaches the list rather than the field. The panel surface,
  the backdrop, and the close button all carry the same commit, so pressing anywhere outside the
  field saves the typed name and leaves the panel as the press asked — the reference does the same
  for a press inside its panel, where each card's own `onClick` commits as it bubbles. Enter saves
  too, then closes the box. Escape is the only discard inside the window, and it also drops the
  typed text back to the stored name. A blank name is never sent: `Settings::validate` rejects it, so
  the box closes on Enter and reopens on the stored name for a press elsewhere. The reference's
  backdrop discards instead — unmounting its input before the browser's blur can run — and that is
  the one exit this port keeps, because a press on "somewhere else" reads as leaving the edit rather
  than cancelling it. Deactivating the window discards as well, which the reference's browser blur
  would commit; an edit still open when the window loses focus is likelier to be interrupted than
  finished.
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
- Under `egui-ui`, build.rs unpacks the 32×32 PNG layer from
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
