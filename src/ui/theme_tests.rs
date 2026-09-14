//! Slot-card tint tests.
//!
//! The values below are the reference's own rendered pixels, not its class
//! names. A browser blends alpha in sRGB while iced blends in linear space, so
//! handing iced an alpha channel lands visibly paler than the reference draws
//! (`#CED1F6` where the reference shows `#EBEEFD`, measured). `slot_tint`
//! flattens the composite to opaque sRGB instead, and these pin that.
//!
//! Every expectation was read off a screenshot of the running reference at the
//! panel's card-fill and card-edge coordinates. Channel tolerance is one 8-bit
//! step, because the browser keeps more precision between two stacked composites
//! than this model does -- the model rounds to a byte at each step. The error it
//! guards against is thirty steps, not one.
//!
//! Four entries are derived rather than measured and are marked as such: the
//! reference was never captured with the Immediate card hovered or the Random
//! Mix card loaded, and the name box's hover inks follow conclusions about the
//! reference's DOM rather than pixels read off it (one `group-hover` class, so
//! the name and the pencil move together; the box's ink sets `currentColor`, so
//! the two must land on the same colour in both states).

use super::theme::{SlotState, slot_ink, slot_ink_hovering, slot_mode_ink, slot_tint};
use crate::settings::SocdMode;

/// Splits a colour into its 8-bit channels, the way a screen shows it.
fn channels(color: iced::Color) -> [u8; 3] {
    let channel = |value: f32| (value * 255.0).round() as u8;
    [channel(color.r), channel(color.g), channel(color.b)]
}

/// Parses `#RRGGBB` into its 8-bit channels.
fn parse(hex: &str) -> [u8; 3] {
    let byte = |i: usize| u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).expect("hex digits");
    [byte(0), byte(1), byte(2)]
}

/// Asserts a colour's channels match, one 8-bit step per channel.
fn assert_close(color: iced::Color, expected: &str, what: &str) {
    let actual = channels(color);
    let want = parse(expected);
    for i in 0..3 {
        assert!(
            actual[i].abs_diff(want[i]) <= 1,
            "{what}: got {actual:?} want {expected} ({want:?})"
        );
    }
}

#[test]
fn immediate_tints_match_the_reference() {
    let active = slot_tint(SocdMode::Immediate, SlotState::Active);
    assert_close(active.fill, "#EBEDFC", "Immediate active fill");
    assert_close(active.border, "#93A1F2", "Immediate active border");

    let idle = slot_tint(SocdMode::Immediate, SlotState::Idle);
    assert_close(idle.fill, "#F8F8FE", "Immediate idle fill");
    assert_close(idle.border, "#DCE0FB", "Immediate idle border");
}

#[test]
fn random_mix_tints_match_the_reference() {
    let hovered = slot_tint(SocdMode::RandomMix, SlotState::Hovered);
    assert_close(hovered.fill, "#F0EDFD", "Random Mix hovered fill");
    assert_close(hovered.border, "#BBAEF7", "Random Mix hovered border");

    let idle = slot_tint(SocdMode::RandomMix, SlotState::Idle);
    assert_close(idle.fill, "#F9F8FE", "Random Mix idle fill");
    assert_close(idle.border, "#E5E0FC", "Random Mix idle border");

    // Derived: the active card was never captured, but it shares Immediate's
    // alphas on its own accent, and that pair was measured.
    let active = slot_tint(SocdMode::RandomMix, SlotState::Active);
    assert_close(active.fill, "#F0EDFD", "Random Mix active fill (derived)");
    assert_close(
        active.border,
        "#AF9FF6",
        "Random Mix active border (derived)",
    );
}

#[test]
fn press_delay_tints_match_the_reference() {
    let active = slot_tint(SocdMode::PressDelay, SlotState::Active);
    assert_close(active.fill, "#E9EEFF", "Press Delay active fill");
    assert_close(active.border, "#7C86FF", "Press Delay active border");

    let hovered = slot_tint(SocdMode::PressDelay, SlotState::Hovered);
    assert_close(hovered.fill, "#F1F5FF", "Press Delay hovered fill");
    assert_close(hovered.border, "#A3B3FF", "Press Delay hovered border");

    let idle = slot_tint(SocdMode::PressDelay, SlotState::Idle);
    assert_close(idle.fill, "#FAFBFF", "Press Delay idle fill");
    assert_close(idle.border, "#E0E6FF", "Press Delay idle border");
}

#[test]
fn release_delay_tints_match_the_reference() {
    let active = slot_tint(SocdMode::ReleaseDelay, SlotState::Active);
    assert_close(active.fill, "#F2F0FE", "Release Delay active fill");
    assert_close(active.border, "#A684FF", "Release Delay active border");

    let hovered = slot_tint(SocdMode::ReleaseDelay, SlotState::Hovered);
    assert_close(hovered.fill, "#F7F5FF", "Release Delay hovered fill");
    assert_close(hovered.border, "#C4B3FF", "Release Delay hovered border");

    let idle = slot_tint(SocdMode::ReleaseDelay, SlotState::Idle);
    assert_close(idle.fill, "#FCFBFF", "Release Delay idle fill");
    assert_close(idle.border, "#ECE8FF", "Release Delay idle border");
}

#[test]
fn every_card_colour_is_opaque() {
    // The whole point of the model: iced's own alpha blending is linear and
    // would land off the reference, so nothing may reach it with an alpha.
    for mode in SocdMode::ALL {
        for state in [SlotState::Active, SlotState::Hovered, SlotState::Idle] {
            let tint = slot_tint(mode, state);
            assert_eq!(tint.fill.a, 1.0, "{mode:?}/{state:?} fill must be opaque");
            assert_eq!(
                tint.border.a, 1.0,
                "{mode:?}/{state:?} border must be opaque"
            );
        }
    }
}

#[test]
fn idle_ink_is_the_reference_opacity_step() {
    // The reference dims an idle card's *contents*, not just its wash, so the
    // name it draws is slate-900 at `opacity-75`.
    assert_close(slot_ink(SlotState::Active).name, "#0F172B", "active name");
    assert_close(slot_ink(SlotState::Idle).name, "#4B5160", "idle name");
    assert_close(
        slot_ink(SlotState::Active).chip_edge,
        "#CAD5E2",
        "active chip edge",
    );
    // Hover is a full-strength card, so it takes the undimmed ink.
    assert_eq!(
        channels(slot_ink(SlotState::Hovered).name),
        channels(slot_ink(SlotState::Active).name)
    );
}

#[test]
fn name_box_hover_moves_the_name_and_the_pencil_together() {
    // The box is the only part of a card whose ink is not a function of the
    // card's state: `group-hover/slot:text-indigo-600` recolours the name, and
    // the pencil draws in `currentColor` from that same declaration, so both
    // land on indigo-600. One class at full strength also means the hover
    // *replaces* the idle dim rather than stacking on it -- an idle card under
    // the pointer draws the same indigo a loaded one would.
    for state in [SlotState::Idle, SlotState::Hovered, SlotState::Active] {
        let resting = slot_ink(state);
        let hovered = slot_ink_hovering(state, true);

        assert_close(hovered.name, "#4F39F6", "hovered name");
        assert_close(hovered.pencil, "#4F39F6", "hovered pencil");
        assert_ne!(
            channels(hovered.name),
            channels(resting.name),
            "{state:?}: the hover has to be visible against the rest ink"
        );
        assert_eq!(
            channels(hovered.name),
            channels(hovered.pencil),
            "{state:?}: the pencil follows the name's colour on hover"
        );
        // The box's own ink is all that moves; the chips and the divider are
        // outside it and keep the card's state ink.
        assert_eq!(channels(hovered.chip_label), channels(resting.chip_label));
        assert_eq!(channels(hovered.chip_edge), channels(resting.chip_edge));
        assert_eq!(channels(hovered.hairline), channels(resting.hairline));
    }

    // At rest the two inks differ, which is why the box cannot be styled
    // through its button's single `text_color`.
    assert_ne!(
        channels(slot_ink(SlotState::Active).name),
        channels(slot_ink(SlotState::Active).pencil)
    );
    // Not hovering is exactly the resting table.
    for state in [SlotState::Idle, SlotState::Hovered, SlotState::Active] {
        assert_eq!(
            channels(slot_ink_hovering(state, false).name),
            channels(slot_ink(state).name),
            "{state:?}: `slot_ink` is `slot_ink_hovering` with no hover"
        );
    }
}

#[test]
fn mode_label_ink_matches_the_reference_table() {
    // The two delay labels reach one ramp step darker than the cards they sit
    // on; the other two label in their card's own colour.
    assert_close(
        slot_mode_ink(SocdMode::Immediate, SlotState::Active),
        "#3A55E8",
        "Immediate label",
    );
    assert_close(
        slot_mode_ink(SocdMode::PressDelay, SlotState::Active),
        "#432DD7",
        "Press Delay label",
    );
    assert_close(
        slot_mode_ink(SocdMode::RandomMix, SlotState::Active),
        "#6D51EE",
        "Random Mix label",
    );
    assert_close(
        slot_mode_ink(SocdMode::ReleaseDelay, SlotState::Active),
        "#7008E7",
        "Release Delay label",
    );
    // An idle card dims its label with everything else.
    assert_close(
        slot_mode_ink(SocdMode::PressDelay, SlotState::Idle),
        "#7262E1",
        "idle Press Delay label",
    );
}

#[test]
fn hover_changes_the_card_but_not_an_active_one() {
    // The reference's active class carries no hover variant, so `slot_state`
    // never reports `Hovered` for the loaded slot. Its tints still have to stay
    // distinguishable, or this test would pass while the hover was invisible.
    //
    // The pair is compared, not each channel: Immediate and Random Mix take the
    // same `bg-…/10` wash in both states and deepen only the edge, so their fills
    // are equal by design and only the border moves.
    for mode in SocdMode::ALL {
        let active = slot_tint(mode, SlotState::Active);
        let hovered = slot_tint(mode, SlotState::Hovered);
        assert_ne!(
            (channels(active.fill), channels(active.border)),
            (channels(hovered.fill), channels(hovered.border)),
            "{mode:?} must draw a different card when hovered than when loaded"
        );
    }
}
