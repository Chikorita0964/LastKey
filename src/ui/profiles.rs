//! The profile / language overlay: the backdrop, the anchored panel, the slot
//! cards, the in-place rename box, and the language rows.
//!
//! `docs/architecture/ui.md` owns the contract; this module paints and pushes
//! [`Message`]s only.
//!
//! # Layering and press routing
//!
//! The overlay is a full-window backdrop, an opaque panel container, and the
//! panel's own press catcher, so a press that reaches the panel but not one of
//! its controls commits an open rename (`SaveProfileNameIfEditing`). egui's hit
//! test gives the same precedence when the containers themselves sense clicks:
//! a widget's background interaction is registered before its content, so the
//! deepest control under the pointer always wins and the surface only receives
//! the presses nothing else claimed. The backdrop is therefore an interactive
//! `Area` in `Order::Middle`, the panel an interactive `Area` in
//! `Order::Foreground`, and each card an interactive `Ui` scope inside it.
//!
//! One translation difference: card presses publish on the press, while egui
//! reports a click on release. A single press still produces one message per
//! control, and a press that ends outside the control it began on is the
//! cancel case both models treat the same way.
//!
//! # Accessible names
//!
//! Panel controls have no visible label, so the port invents English names:
//! `Profile slot 1..4` for card backgrounds, `Close` for the panel close
//! button, and the profile name for a name box. Localizing accessible names is
//! the open policy T2's report raised.

use egui::{
    Align, Align2, Area, Color32, Context, FontId, Id, Layout, Margin, Order, Pos2, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
    vec2,
};

use crate::core::PhysicalKey;
use crate::platform::windows::physical_key_name;
use crate::protocol::UiSnapshot;
use crate::settings::ProfileSlot;

use super::{
    header, language,
    message::Message,
    state::{ProfileDialog, SlotState, State},
    theme, timing,
};

/// The panel's top offset from the window's top edge: the page padding, the
/// header, and an 8px gap.
pub const PROFILE_PANEL_TOP: f32 = theme::PAGE_PADDING + header::HEADER_HEIGHT + 8.0;

/// The rename box's stable id, so `Effect::FocusProfileName` can find it.
pub fn profile_name_input_id() -> Id {
    Id::new("profile-name-input")
}

/// The deferred focus request [`focus_profile_name`] stores and the rename
/// field consumes.
fn focus_request_id() -> Id {
    Id::new("profile-name-focus-request")
}

fn backdrop_id() -> Id {
    Id::new("profile-backdrop")
}

fn panel_id() -> Id {
    Id::new("profile-panel")
}

/// Bridges the state layer's [`SlotState`] to the token module's
/// [`theme::SlotState`]. The two enums are distinct types carrying the same
/// three-state contract, and the view is the only place that needs both, so
/// the bridge lives here rather than in either owner. Consolidating them is a
/// Master-level decision (see the T6 report).
fn theme_slot_state(state: SlotState) -> theme::SlotState {
    match state {
        SlotState::Idle => theme::SlotState::Idle,
        SlotState::Hovered => theme::SlotState::Hovered,
        SlotState::Active => theme::SlotState::Active,
    }
}

/// Draws the overlay and returns the messages this frame produced. Draws
/// nothing when the panel is closed or no snapshot has mounted yet, the two
/// cases that have nothing to anchor the panel to.
pub fn profile_overlay(ui: &mut Ui, state: &State) -> Vec<Message> {
    let mut messages = Vec::new();
    if matches!(state.profiles, ProfileDialog::Closed) || state.snapshot.is_none() {
        return messages;
    }
    let ctx = ui.ctx().clone();
    backdrop(&ctx, &mut messages);
    panel(&ctx, state, &mut messages);
    messages
}

/// Ask for the rename box to take focus with its whole value selected, the
/// focus-and-select-all pair the rename flow needs. The app loop calls this
/// when it executes `Effect::FocusProfileName`.
///
/// The request is stored, not applied here: the effect arrives on the frame
/// the name box was replaced by the field, so the widget this id names does
/// not exist yet, and leaving a focused id without a node makes AccessKit
/// report a dangling focus (egui_kittest panics on it). The field consumes
/// the request on the frame it mounts instead.
pub fn focus_profile_name(ui: &Ui) {
    ui.ctx()
        .data_mut(|data| data.insert_temp(focus_request_id(), true));
}

/// The full-window press catcher under the panel. A press that reaches it is a
/// press outside the panel, which closes the dialog.
fn backdrop(ctx: &Context, messages: &mut Vec<Message>) {
    Area::new(backdrop_id())
        .order(Order::Middle)
        .fixed_pos(Pos2::ZERO)
        .sense(Sense::click())
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if response.clicked() {
                messages.push(Message::CloseProfiles);
            }
        });
}

/// The anchored panel and its surface: the header block, then the body the
/// dialog state selects. The panel's own clicks are the presses that reached
/// the surface rather than a control, and they commit an open rename.
fn panel(ctx: &Context, state: &State, messages: &mut Vec<Message>) {
    let languages = matches!(state.profiles, ProfileDialog::Languages);
    // A focus request belongs to the rename that produced it; drop a stale one
    // when no rename is open.
    if !matches!(
        state.profiles,
        ProfileDialog::Rename { .. } | ProfileDialog::Renaming { .. }
    ) {
        ctx.data_mut(|data| data.remove::<bool>(focus_request_id()));
    }
    let width = if languages {
        theme::LANGUAGE_PANEL_WIDTH
    } else {
        theme::PROFILE_PANEL_WIDTH
    };
    let area = Area::new(panel_id())
        .order(Order::Foreground)
        .anchor(
            Align2::RIGHT_TOP,
            vec2(-theme::PAGE_PADDING, PROFILE_PANEL_TOP),
        )
        .sense(Sense::click())
        .show(ctx, |ui| {
            // A selectable name box would fight the drag-to-rename
            // interaction; more importantly, a selectable
            // label senses clicks and would swallow the surface press before
            // it reaches the panel.
            ui.style_mut().interaction.selectable_labels = false;
            theme::profile_panel()
                .inner_margin(theme::PANEL_PADDING)
                .show(ui, |ui| {
                    ui.set_width(width);
                    panel_header(ui, state, languages, messages);
                    let scroller_padding = if languages {
                        theme::LANGUAGE_SCROLLER_PADDING
                    } else {
                        theme::PROFILE_SCROLLER_PADDING
                    };
                    egui::Frame::new()
                        .inner_margin(scroller_padding)
                        .show(ui, |ui| {
                            if languages {
                                messages.extend(language::language_rows(ui, state));
                            } else {
                                slot_list(ui, state, messages);
                            }
                        });
                });
        });
    if area.response.clicked() {
        messages.push(Message::SaveProfileNameIfEditing);
    }
}

/// The panel's header: the section glyph, the title and its subtitle (or the
/// panel's live error), and the borderless close circle. Port of
/// the panel's content column; the text sizes and paddings come from the theme.
fn panel_header(ui: &mut Ui, state: &State, languages: bool, messages: &mut Vec<Message>) {
    egui::Frame::new()
        .inner_margin(theme::PROFILE_HEADER_PADDING)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                theme::paint_icon(
                    ui.painter(),
                    rect,
                    if languages {
                        theme::Icon::Languages
                    } else {
                        theme::Icon::Layers
                    },
                    theme::INDIGO_600,
                );
                // The heading block keeps the subtitle inside the title column
                // so the close button adds no height of its own.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme::PROFILE_HEADER_GAP;
                    ui.label(
                        RichText::new(state.language.text(if languages {
                            "Language"
                        } else {
                            "Profile Slots"
                        }))
                        .size(14.0)
                        .strong(),
                    );
                    if !languages {
                        ui.label(
                            RichText::new(
                                state
                                    .language
                                    .text("Changes are saved when you click Apply."),
                            )
                            .size(11.0)
                            .color(theme::SLATE_500),
                        );
                    }
                    if let Some(error) = &state.error {
                        ui.label(
                            RichText::new(state.language.text(error))
                                .size(12.0)
                                .color(theme::RED_600),
                        );
                    }
                });
                // Only a load hides the close button.
                if !matches!(state.profiles, ProfileDialog::Loading) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (box_size, icon_size) = if languages {
                            (28.0, 12.0)
                        } else {
                            (36.0, 14.0)
                        };
                        if close_button(ui, box_size, icon_size).clicked() {
                            messages.push(Message::CloseProfiles);
                        }
                    });
                }
            });
        });
}

/// The borderless close circle: a muted glyph at
/// rest, an indigo-50 wash and an indigo-600 glyph under the pointer. The
/// reference sizes it on the box -- 36px on the slot dialog, 28px on the
/// language menu -- and centres the glyph by its padding.
fn close_button(ui: &mut Ui, box_size: f32, icon_size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(box_size), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Close"));
    let hovered = response.hovered();
    let painter = ui.painter();
    if hovered {
        // `Color { a: 0.7, ..INDIGO_50 }`, the hover fill, derived from
        // the theme token instead of re-typing its channels.
        let fill = Color32::from_rgba_unmultiplied(
            theme::INDIGO_50.r(),
            theme::INDIGO_50.g(),
            theme::INDIGO_50.b(),
            179,
        );
        painter.circle_filled(rect.center(), box_size / 2.0, fill);
    }
    let ink = if hovered {
        theme::INDIGO_600
    } else {
        theme::SLATE_400
    };
    theme::paint_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
        theme::Icon::Close,
        ink,
    );
    response
}

/// The body for the slot dialog: the loading line, or one card per bank slot.
/// A rename in flight renders through the same card list, because swapping the
/// list for a progress line is the panel flickering.
fn slot_list(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) {
    let Some(snapshot) = &state.snapshot else {
        return;
    };
    if matches!(state.profiles, ProfileDialog::Loading) {
        ui.label(RichText::new(state.language.text("Loading and activating profile…")).size(16.0));
        return;
    }
    let bank = snapshot.saved.profile_bank();
    ui.spacing_mut().item_spacing.y = theme::SLOT_GAP;
    let mut hovered_name = None;
    for (index, profile) in bank.slots.iter().enumerate() {
        let slot = index as u8;
        if let Some(hovered) = slot_card(ui, state, snapshot, slot, profile, bank.active, messages)
        {
            hovered_name = Some(hovered);
        }
    }
    // The name box's own hover is one value in the state, so it is cleared by
    // the list when no box carries the pointer (the mouse_area published
    // its exit the same way). Emitting only the positive side from the boxes
    // keeps the message order irrelevant.
    if hovered_name.is_none()
        && let Some(slot) = state.hovered_name
    {
        messages.push(Message::NameHovered(slot, false));
    }
}

/// One slot in the panel: a mode-tinted card whose first row is the name box
/// (which doubles as the rename target) and whose second row is the axis-paired
/// keycap chips beside the mode label. The whole card is the load target except
/// while it is active or wearing its confirm banner.
///
/// Returns the slot whose name box the pointer is over, if any.
fn slot_card(
    ui: &mut Ui,
    state: &State,
    snapshot: &UiSnapshot,
    slot: u8,
    profile: &ProfileSlot,
    active_slot: u8,
    messages: &mut Vec<Message>,
) -> Option<u8> {
    let card_state = state.slot_state(slot, active_slot);
    let theme_state = theme_slot_state(card_state);
    let tint = theme::slot_tint(profile.timing.mode, theme_state);
    let ink = theme::slot_ink(theme_state);
    let renaming = matches!(
        &state.profiles,
        ProfileDialog::Rename { slot: editing, .. }
            | ProfileDialog::Renaming { slot: editing, .. }
            if *editing == slot
    );
    let confirming = matches!(&state.profiles, ProfileDialog::Confirm(pending) if *pending == slot);
    // The name box's own hover, read from the state the hover message
    // published; the pencil and the box's indigo ink both fold it in.
    let name_hovered = state.hovered_name == Some(slot) && !renaming;
    let name_ink = theme::slot_ink_hovering(theme_state, name_hovered);

    let mut hovered_name = None;
    let card = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        let frame = theme::tinted_slot(tint)
            .inner_margin(theme::SLOT_CARD_PADDING)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme::SLOT_ROW_GAP;
                    // The banner covers the whole card, so a box under it would
                    // show only as a sliver; the edit survives in the state.
                    if !(confirming && renaming) {
                        hovered_name =
                            name_row(ui, state, slot, &profile.name, name_ink, ink, messages);
                    }
                    chips_row(ui, state, snapshot, profile, theme_state, ink);
                });
            });
        if confirming {
            confirm_overlay(ui, frame.response.rect, state, slot, messages);
        }
    });
    let response = card.response;
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("Profile slot {}", slot + 1),
        )
    });

    // The card's hover is a `contains_pointer` test: like the mouse_area,
    // it fires while the pointer is over a child control too. The state update
    // makes the exit order-safe for a pointer that moves between cards.
    let card_hovered = response.contains_pointer();
    if card_hovered && state.hovered_slot != Some(slot) {
        messages.push(Message::SlotHovered(slot));
    } else if !card_hovered && state.hovered_slot == Some(slot) {
        messages.push(Message::SlotUnhovered(slot));
    }
    // The name box's hover enters the state the same way; only the positive
    // side is published here and `slot_list` clears it, so the message order
    // cannot cancel a box that took the pointer in the same frame.
    if hovered_name == Some(slot) && state.hovered_name != Some(slot) {
        messages.push(Message::NameHovered(slot, true));
    }
    // The card is not a load target while it is the loaded one, nor while its
    // own confirm banner is up. Its controls shield it: the name box and the
    // field take their own presses, so a press that reaches here was on the
    // card itself.
    if response.clicked() && card_state != SlotState::Active && !confirming {
        messages.push(if renaming {
            Message::SaveProfileNameIfEditing
        } else {
            Message::LoadProfile(slot)
        });
    }
    hovered_name
}

/// The card's first row in whichever of its three states applies: the
/// committed box (also drawn while a rename is in flight), the live field, or
/// the resting name box. Returns the slot when the pointer is over the name
/// box.
fn name_row(
    ui: &mut Ui,
    state: &State,
    slot: u8,
    profile_name: &str,
    name_ink: theme::SlotInk,
    ink: theme::SlotInk,
    messages: &mut Vec<Message>,
) -> Option<u8> {
    let pending_name = match &state.profiles {
        ProfileDialog::Renaming { slot: saving, name } if *saving == slot => Some(name.as_str()),
        _ => None,
    };
    if let Some(pending) = pending_name {
        // The committed box, not the live field: the field would take the next
        // keystroke into a write that has already left. It keeps the pencil so
        // the box does not shorten by 16px and jump on either side of the
        // round trip. Nothing here is interactive.
        committed_box(ui, pending, ink);
        return None;
    }
    if let ProfileDialog::Rename {
        slot: editing,
        name,
    } = &state.profiles
        && *editing == slot
    {
        rename_field(ui, state, name, messages);
        return None;
    }
    name_box(ui, slot, profile_name, name_ink, messages)
}

/// The resting name box: a white pill holding the name and its pencil. Its
/// fill, edge and ink move to the indigo hover pair under the pointer, which
/// is the reference's `group-hover/slot` on the box alone. Every click opens
/// the rename edit, published on the press so the box never lets it through to
/// the card behind it.
fn name_box(
    ui: &mut Ui,
    slot: u8,
    name: &str,
    ink: theme::SlotInk,
    messages: &mut Vec<Message>,
) -> Option<u8> {
    let (galley, response, rect) = name_box_frame(ui, name, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
    let hovered = response.contains_pointer();
    let (fill, edge) = if hovered {
        (theme::INDIGO_50_60, theme::INDIGO_300)
    } else {
        (theme::WHITE, ink.hairline)
    };
    paint_name_box(ui, rect, fill, edge);
    paint_name_content(ui, rect, &galley, ink);
    if response.clicked() {
        messages.push(Message::EditProfileName(slot));
    }
    hovered.then_some(slot)
}

/// The box shown while a rename is in flight: the same contents and chrome as
/// the resting box, without a control of its own.
fn committed_box(ui: &mut Ui, name: &str, ink: theme::SlotInk) {
    let (galley, response, rect) = name_box_frame(ui, name, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), name));
    paint_name_box(ui, rect, theme::WHITE, ink.hairline);
    paint_name_content(ui, rect, &galley, ink);
}

/// The name box made editable in place: the same fill, radius and left inset
/// as the box it replaces, so the name does not move -- only the edge changes,
/// to `INDIGO_400`. The field takes the text's own width and grows to the
/// right as the name is typed (the owner's requested departure from the
/// reference's fixed `w-28`). Enter saves; Escape unwinds through the app's
/// global key path, exactly as it did before.
///
/// The field's width is measured from the committed name, which the state
/// layer updates one frame behind the keystroke that produced it. With egui's
/// default singleline clipping the widget is then clamped to that stale width
/// (`text_edit/builder.rs`), the overflowing text scrolls left to keep the
/// caret in view, and the scroll offset resets on the next frame when the
/// measured width catches up -- the per-keystroke jump. `clip_text(false)`
/// turns that clamping off, so the widget takes the live galley's width in the
/// same frame: the box grows rightward and the text never moves.
fn rename_field(ui: &mut Ui, state: &State, name: &str, messages: &mut Vec<Message>) {
    let id = profile_name_input_id();
    let mut text = name.to_owned();
    let hint = state.language.text("Profile name");
    let galley = layout_label(ui, name, 12.0);
    let hint_galley = layout_label(ui, hint, 12.0);
    let width = galley.size().x.max(hint_galley.size().x);
    let frame = egui::Frame::new()
        .fill(theme::WHITE)
        .stroke(Stroke::new(1.0, theme::INDIGO_400))
        .corner_radius(theme::SEGMENT_RADIUS)
        .inner_margin(theme::SLOT_NAME_INPUT_PADDING)
        .show(ui, |ui| {
            ui.add(
                TextEdit::singleline(&mut text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(Margin::ZERO)
                    .desired_width(width)
                    .clip_text(false)
                    .font(FontId::proportional(12.0))
                    .text_color(theme::SLATE_900)
                    .hint_text(hint.to_owned())
                    .vertical_align(Align::Center),
            )
        })
        .inner;
    frame.widget_info(|| {
        let mut info = WidgetInfo::labeled(WidgetType::TextEdit, ui.is_enabled(), hint);
        info.current_text_value = Some(name.to_owned());
        info
    });
    // A pending `focus_profile_name` request is honoured here, where the field
    // exists: focus and selection both land on the next frame.
    if ui
        .ctx()
        .data(|data| data.get_temp::<bool>(focus_request_id()).unwrap_or(false))
    {
        frame.request_focus();
        super::timing::select_all_text(ui, id, name);
        ui.ctx()
            .data_mut(|data| data.remove::<bool>(focus_request_id()));
    }
    // Text first, then submit: a frame that types and commits in the same
    // keystroke must hand the state the typed value before the save reads it.
    if frame.changed() {
        messages.push(Message::ProfileNameChanged(text));
    }
    if (frame.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
        || (frame.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
    {
        messages.push(Message::SaveProfileName);
    }
}

/// The measured box both name-box states share: the galley, the allocated
/// response, and the rect to paint into.
fn name_box_frame(
    ui: &mut Ui,
    name: &str,
    sense: Sense,
) -> (std::sync::Arc<egui::Galley>, Response, Rect) {
    const PENCIL: f32 = 12.0;
    let galley = layout_label(ui, name, 12.0);
    let content = galley.size().x + theme::SLOT_NAME_GAP + PENCIL;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(
            content + theme::SLOT_NAME_PADDING.left + theme::SLOT_NAME_PADDING.right,
            galley.size().y + theme::SLOT_NAME_PADDING.top + theme::SLOT_NAME_PADDING.bottom,
        ),
        sense,
    );
    (galley, response, rect)
}

fn paint_name_box(ui: &Ui, rect: Rect, fill: Color32, edge: Color32) {
    let painter = ui.painter();
    painter.add(theme::SHADOW_NAME_BOX.as_shape(rect, theme::SEGMENT_RADIUS));
    painter.rect(
        rect,
        theme::SEGMENT_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
}

/// The name and its pencil, both in the ink the caller chose. The name is
/// stamped because the label is emphasised (the port's weight
/// approximation; the font-weight decision stays open).
fn paint_name_content(
    ui: &Ui,
    rect: Rect,
    galley: &std::sync::Arc<egui::Galley>,
    ink: theme::SlotInk,
) {
    const PENCIL: f32 = 12.0;
    let painter = ui.painter();
    let text_pos = Pos2::new(
        rect.left() + theme::SLOT_NAME_PADDING.left,
        rect.center().y - galley.size().y / 2.0,
    );
    theme::stamp_galley(painter, text_pos, galley, ink.name, 12.0);
    let pencil_rect = Rect::from_center_size(
        Pos2::new(
            rect.right() - theme::SLOT_NAME_PADDING.right - PENCIL / 2.0,
            rect.center().y,
        ),
        Vec2::splat(PENCIL),
    );
    theme::paint_icon(painter, pencil_rect, theme::Icon::Edit, ink.pencil);
}

/// The chips row: the two axis pairs around their hairline, with the mode
/// label at the trailing edge. Bindings are stored vertical-first,
/// vertical-second, horizontal-first, horizontal-second.
fn chips_row(
    ui: &mut Ui,
    state: &State,
    snapshot: &UiSnapshot,
    profile: &ProfileSlot,
    card_state: theme::SlotState,
    ink: theme::SlotInk,
) {
    let bindings = profile.bindings;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::CHIP_GAP;
            chip(ui, bindings[0], snapshot, ink);
            chip(ui, bindings[1], snapshot, ink);
        });
        // The pair divider is a filled box sized to the chips, not a rule: the
        // reference's `border-l` is content-sized, and an egui rule would fill
        // the row (ui.md's Do Not Regress note).
        let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, theme::CHIP_SIZE), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, ink.hairline);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::CHIP_GAP;
            chip(ui, bindings[2], snapshot, ink);
            chip(ui, bindings[3], snapshot, ink);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(timing::mode_label(profile.timing.mode, state.language))
                    .size(10.0)
                    .strong()
                    .color(theme::slot_mode_ink(profile.timing.mode, card_state)),
            );
        });
    });
}

/// One keycap chip: a square floor that grows sideways for a longer label, with
/// the monospace label on its centre line. The key name comes from the snapshot
/// when the wire names it; every other key resolves through
/// [`physical_key_name`], the resolver the runtime builds the wire names with,
/// so a key held only by an inactive profile still reads as a key name rather
/// than the resolver's `Scan code 0x{:02X}` fallback.
fn chip(ui: &mut Ui, physical: PhysicalKey, snapshot: &UiSnapshot, ink: theme::SlotInk) {
    let name = snapshot
        .keys
        .iter()
        .find(|key| key.physical == physical)
        .map(|key| key.name.clone())
        .unwrap_or_else(|| physical_key_name(physical));
    let galley = ui.painter().layout_no_wrap(
        name.clone(),
        FontId::new(10.0, theme::MONO_FONT),
        Color32::PLACEHOLDER,
    );
    let width = (galley.size().x + theme::CHIP_PADDING.left + theme::CHIP_PADDING.right)
        .max(theme::CHIP_CONTENT_MIN + theme::CHIP_PADDING.left + theme::CHIP_PADDING.right);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::CHIP_SIZE), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), &name));
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CHIP_RADIUS,
        theme::WHITE,
        Stroke::new(1.0, ink.chip_edge),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink.chip_label);
}

/// The confirm banner over a card whose press would discard a dirty draft.
/// The banner covers the card so the card's own load
/// target is not reachable, and its two buttons are the only controls on it.
fn confirm_overlay(ui: &mut Ui, rect: Rect, state: &State, slot: u8, messages: &mut Vec<Message>) {
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        theme::WHITE_95,
        Stroke::new(1.0, theme::SLATE_300),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(12.0);
    let mut banner = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    banner.spacing_mut().item_spacing.x = 8.0;
    banner.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new(state.language.text("Load this slot?"))
                .size(12.0)
                .strong(),
        );
        ui.label(
            RichText::new(
                state
                    .language
                    .text("Unapplied draft changes will be discarded."),
            )
            .size(11.0)
            .color(theme::SLATE_500),
        );
    });
    banner.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if banner_button(ui, state.language.text("Load"), true).clicked() {
            messages.push(Message::ConfirmProfile(slot));
        }
        if banner_button(ui, state.language.text("Cancel"), false).clicked() {
            messages.push(Message::OpenProfiles);
        }
    });
}

/// A label-only control for the confirm banner: the filled primary Load and
/// the outlined Cancel, at the `[6, 12]` padding. `theme::secondary_button`
/// always paints an icon and neither of these has one, so this is the
/// label-only arm of the same shell.
fn banner_button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 6.0;
    let galley = layout_label(ui, label, theme::BUTTON_TEXT_SIZE);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + 2.0 * PAD_X, galley.size().y + 2.0 * PAD_Y),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let hovered = response.hovered();
    let (fill, edge, ink) = if primary {
        let fill = if hovered {
            theme::INDIGO_700
        } else {
            theme::INDIGO_600
        };
        (fill, theme::INDIGO_700, theme::WHITE)
    } else if hovered {
        (theme::INDIGO_50_60, theme::INDIGO_300, theme::INDIGO_600)
    } else {
        (theme::WHITE, theme::SLATE_200, theme::SLATE_600)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let position = rect.center() - galley.size() / 2.0;
    if primary {
        theme::stamp_galley(painter, position, &galley, ink, theme::BUTTON_TEXT_SIZE);
    } else {
        painter.galley(position, galley, ink);
    }
    response
}

/// One measured line at the proportional family, laid out in `PLACEHOLDER` so
/// the paint colour is the one handed to `Painter::galley`.
fn layout_label(ui: &Ui, text: &str, size: f32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(size),
        Color32::PLACEHOLDER,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::PhysicalKey,
        protocol::{DisplayKey, UiCommand},
        settings::Settings,
        ui::{
            language::Language,
            state::{Effect, TimingInputs, update},
        },
    };

    fn baseline_snapshot() -> UiSnapshot {
        let keys = std::array::from_fn(|index| DisplayKey {
            physical: PhysicalKey::new(0x11 + index as u16, false),
            name: ["W", "S", "A", "D"][index].into(),
        });
        UiSnapshot {
            filter_enabled: true,
            saved: Settings::default(),
            draft: Settings::default(),
            keys,
            capture_slot: None,
            measurement_active: false,
            measurement: None,
        }
    }

    fn baseline_state() -> State {
        let draft = Settings::default();
        State {
            connected: true,
            inputs: TimingInputs::from_timing(&draft.timing),
            draft: Some(draft),
            snapshot: Some(baseline_snapshot()),
            ..State::default()
        }
    }

    /// The harness's stand-in for the runtime: it feeds the overlay's messages
    /// through `state::update`, records the commands, and executes the focus
    /// effect the way the app loop will.
    struct FakeRuntime {
        state: State,
        sent: Vec<UiCommand>,
        focused: bool,
    }

    impl FakeRuntime {
        fn frame(&mut self, ui: &mut Ui) {
            let messages = profile_overlay(ui, &self.state);
            for message in messages {
                for effect in update(&mut self.state, message) {
                    match effect {
                        Effect::Send(command) => self.sent.push(command),
                        Effect::FocusProfileName => {
                            focus_profile_name(ui);
                            self.focused = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn harness(state: State) -> egui_kittest::Harness<'static, FakeRuntime> {
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state,
                    sent: Vec::new(),
                    focused: false,
                },
            )
    }

    fn open_list(state: &mut State) {
        state.profiles = ProfileDialog::List;
    }

    #[test]
    fn a_card_loads_the_slot_it_covers() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        let mut harness = harness(state);

        harness.get_by_label("Profile slot 2").click();
        harness.run();
        assert!(
            harness.state().sent.contains(&UiCommand::LoadProfile(1)),
            "a clean draft must load the card's slot directly"
        );
        assert!(matches!(
            harness.state().state.profiles,
            ProfileDialog::Loading
        ));
    }

    #[test]
    fn a_dirty_draft_confirms_before_the_slot_loads() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        // One timing edit is enough to make the draft dirty.
        state
            .draft
            .as_mut()
            .unwrap()
            .timing
            .socd_transition_max_micros += 1;
        let mut harness = harness(state);

        harness.get_by_label("Profile slot 2").click();
        harness.run();
        assert!(matches!(
            harness.state().state.profiles,
            ProfileDialog::Confirm(1)
        ));
        harness.get_by_label("Load this slot?");

        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(matches!(
            harness.state().state.profiles,
            ProfileDialog::List
        ));
        assert!(
            !harness
                .state()
                .sent
                .iter()
                .any(|command| matches!(command, UiCommand::LoadProfile(_))),
            "Cancel must not load"
        );

        harness.get_by_label("Profile slot 2").click();
        harness.run();
        harness.get_by_label("Load").click();
        harness.run();
        assert!(
            harness.state().sent.contains(&UiCommand::LoadProfile(1)),
            "the confirm banner's Load must reach the state layer"
        );
    }

    #[test]
    fn the_rename_box_opens_and_commits_the_typed_name() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        let stored = state.stored_profile_name(0).unwrap();
        let mut harness = harness(state);

        // The name box is the edit target; clicking it must open the field and
        // ask for focus, not load the slot.
        harness.get_by_label(&stored).click();
        harness.run();
        let opened = matches!(
            &harness.state().state.profiles,
            ProfileDialog::Rename { slot: 0, .. }
        );
        assert!(opened, "the name box must open the rename edit");
        assert!(harness.state().focused, "the edit must request field focus");

        // Let the field mount, consume the focus request, and take focus with
        // its value selected; then one typed run replaces the stored name.
        harness.run();
        harness.run();
        assert!(
            harness.get_by_label("Profile name").is_focused(),
            "the rename field must hold focus"
        );
        harness.get_by_label("Profile name").type_text("Renamed");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness.state().sent.contains(&UiCommand::RenameProfile {
                slot: 0,
                name: "Renamed".into()
            }),
            "Enter must commit the typed name"
        );
    }

    #[test]
    fn the_panel_surface_commits_an_open_rename() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        let stored = state.stored_profile_name(0).unwrap();
        let mut harness = harness(state);

        harness.get_by_label(&stored).click();
        harness.run();

        // Press the panel's title block, which no control claims: this is the
        // reference's "somewhere else inside the panel" and saves the edit.
        let title = harness.get_by_label("Profile Slots").rect();
        let press = Pos2::new(title.right() + 30.0, title.center().y);
        harness.hover_at(press);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
        }
        harness.run();

        assert!(
            harness.state().sent.contains(&UiCommand::RenameProfile {
                slot: 0,
                name: stored.clone()
            }),
            "a press on the panel surface must save the open rename"
        );
    }

    #[test]
    fn the_panel_is_anchored_below_the_header_and_inside_the_page_edge() {
        let mut state = baseline_state();
        open_list(&mut state);
        let mut harness = harness(state);
        harness.run();
        harness.run();

        // The drawn panel frame: `PROFILE_PANEL_WIDTH` plus `PANEL_PADDING` on
        // each side plus the 1px stroke egui adds around a frame's content
        // (`Frame::outer_rect`), its right edge `PAGE_PADDING` from the window
        // and its top `PROFILE_PANEL_TOP` below it -- the align_right
        // container with the `PROFILE_PANEL_TOP` spacer plus its own border.
        // The frame's shadow/fill/stroke ride inside a `Shape::Vec`, so the
        // search recurses.
        fn collect(shape: &egui::Shape, wide: &mut Vec<(f32, f32, f32, egui::Color32)>) {
            match shape {
                egui::Shape::Rect(rect) if rect.rect.width() > 300.0 => {
                    wide.push((
                        rect.rect.min.x,
                        rect.rect.min.y,
                        rect.rect.width(),
                        rect.fill,
                    ));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, wide);
                    }
                }
                _ => {}
            }
        }
        let drawn_width = theme::PROFILE_PANEL_WIDTH + 2.0 * (theme::PANEL_PADDING.left + 1.0);
        let expected_min = Pos2::new(
            1040.0 - theme::PAGE_PADDING - drawn_width,
            PROFILE_PANEL_TOP,
        );
        let mut wide = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, &mut wide);
        }
        let found = wide.iter().any(|&(x, y, width, fill)| {
            fill == theme::WHITE
                && (x - expected_min.x).abs() <= 1.5
                && (y - expected_min.y).abs() <= 1.5
                && (width - drawn_width).abs() <= 1.5
        });
        assert!(
            found,
            "the panel frame must sit at the anchored position (expected min {expected_min:?}, width {drawn_width}); wide rects: {wide:?}"
        );
    }

    #[test]
    fn a_language_row_selects_the_language() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        state.profiles = ProfileDialog::Languages;
        let mut harness = harness(state);

        harness.get_by_label("Español").click();
        harness.run();
        assert_eq!(harness.state().state.language, Language::Spanish);
        assert!(matches!(
            harness.state().state.profiles,
            ProfileDialog::Closed
        ));
    }

    #[test]
    fn a_pointer_left_on_a_card_clears_its_hover() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        let mut harness = harness(state);

        harness.get_by_label("Profile slot 1").hover();
        harness.run();
        harness.run();
        assert_eq!(harness.state().state.hovered_slot, Some(0));

        harness.hover_at(Pos2::new(4.0, 4.0));
        harness.run();
        assert_eq!(harness.state().state.hovered_slot, None);
    }

    /// F16: the field is content-sized, so the width it renders with must
    /// already cover the text being typed in the frame the keystroke lands.
    /// The committed name the width is measured from updates one frame later,
    /// so a width measured only from it leaves the typed text overflowing for
    /// one frame; egui's singleline clipping would scroll the text left there
    /// and back as the width catches up.
    #[test]
    fn the_rename_field_takes_the_typed_texts_width_in_the_typing_frame() {
        use egui_kittest::kittest::Queryable;

        let mut state = baseline_state();
        open_list(&mut state);
        let stored = state.stored_profile_name(0).unwrap();
        let mut harness = harness(state);

        harness.get_by_label(&stored).click();
        harness.run();
        harness.run();
        assert!(
            harness.get_by_label("Profile name").is_focused(),
            "the rename field must hold focus before typing"
        );

        let typed = "A name far longer than the stored one";
        harness.get_by_label("Profile name").type_text(typed);
        // One step runs the frame that applies the queued text event. The
        // assertion below reads the rect of that same frame, before the state
        // layer's name update can widen the field on the following frame.
        harness.step();

        let text_width = harness.ctx.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    typed.to_owned(),
                    FontId::proportional(12.0),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
        });
        let rect = harness.get_by_label("Profile name").rect();
        assert!(
            rect.width() + 1.0 >= text_width,
            "the field must take the typed text's width in the typing frame: \
             rect {rect:?}, typed text width {text_width}"
        );
    }

    /// F17: the active draft names only its own four bindings, but an inactive
    /// slot's key must still read as a key name, not as its `SC:xx` scan code.
    /// The fallback goes through the platform resolver the wire names come
    /// from, so the same key reads the same way in either state.
    #[test]
    fn an_inactive_slots_chip_names_a_key_the_active_draft_no_longer_binds() {
        use egui_kittest::kittest::Queryable;

        // The user remapped the active profile's vertical-first key from W
        // (0x11) to Right Shift (0x36); the inactive slots keep W.
        let remapped = PhysicalKey::new(0x36, false);
        let mut saved = Settings::default();
        saved.set_binding(crate::core::LogicalKey::VerticalFirst, remapped);
        let mut snapshot = baseline_snapshot();
        snapshot.saved = saved.clone();
        snapshot.draft = saved;
        snapshot.keys = [
            DisplayKey {
                physical: remapped,
                name: "Right Shift".into(),
            },
            DisplayKey {
                physical: PhysicalKey::new(0x1F, false),
                name: "S".into(),
            },
            DisplayKey {
                physical: PhysicalKey::new(0x1E, false),
                name: "A".into(),
            },
            DisplayKey {
                physical: PhysicalKey::new(0x20, false),
                name: "D".into(),
            },
        ];
        let draft = snapshot.draft.clone();
        let mut state = State {
            snapshot: Some(snapshot),
            draft: Some(draft),
            ..baseline_state()
        };
        open_list(&mut state);
        let mut harness = harness(state);
        harness.run();

        let platform_name = physical_key_name(PhysicalKey::new(0x11, false));
        assert_ne!(
            platform_name, "SC:11",
            "the platform resolver must not fall back to the old scan code"
        );
        assert_eq!(
            harness.query_all_by_label("SC:11").count(),
            0,
            "a key held only by inactive slots must not render its scan code"
        );
        assert!(
            harness.query_all_by_label(&platform_name).count() >= 3,
            "each inactive slot's W chip must carry the platform key name {platform_name:?}"
        );
        assert_eq!(
            harness.query_all_by_label("Right Shift").count(),
            1,
            "the wire name must still win for the active draft's binding"
        );
    }
}
