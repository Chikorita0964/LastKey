//! Semantic UI tests: the shipped settings controls, driven by accessible
//! label rather than by coordinate.
//!
//! Every interaction here goes through `egui_kittest`'s `get_by_label`, so a
//! control that loses its accessible name fails the test instead of silently
//! drifting. The flows cover the migration's list: rebind a key, change a
//! mode, edit a timing value, toggle the filter, and rename a profile.
//!
//! `src/ui2/app.rs` is a private module, so the shell below composes the same
//! public cards the page does (`app::settings_cards`): the header, the
//! mapping/timing card row, the amber dirty badge the action bar mounts, and
//! the profile overlay. The fakes sit at the process boundary: the shell runs
//! `state::update` and records the `UiCommand`s a real connection would send,
//! and the rebind flow injects the runtime's `KeyCaptured` event the way the
//! IPC reader would.
//!
//! `cargo test` compiles this file with default features, where `src/lib.rs`
//! keeps `ui2` behind `windows + egui-ui`; the crate-level cfg leaves the file
//! empty in that configuration.

#![cfg(all(windows, feature = "egui-ui"))]

use egui::{Align, Layout, Ui, Vec2, accesskit::Role};
use egui_kittest::{Harness, kittest::Queryable};
use lastkey::{
    core::PhysicalKey,
    protocol::{DisplayKey, KeySlot, UiCommand, UiEvent, UiSnapshot},
    settings::{Settings, SocdMode},
    ui2::{
        header, mapping,
        message::{IpcEvent, Message, TimingField},
        profiles,
        state::{Effect, ProfileDialog, State, TimingInputs, update},
        theme, timing,
        timing::PreviewMount,
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

/// The harness's stand-in for the app loop. It draws the page's public cards,
/// feeds every message through `state::update`, records the commands a real
/// connection would send, and executes the one view-side effect the field
/// needs (the rename focus request).
struct PageShell {
    state: State,
    sent: Vec<UiCommand>,
}

impl PageShell {
    /// Runs one message through the state layer, records the commands a real
    /// connection would send, and returns the effects for the caller. The two
    /// capture commands are mirrored back into the snapshot the way the
    /// runtime answers them, so the view sees an armed capture.
    fn dispatch(&mut self, message: Message) -> Vec<Effect> {
        let effects = update(&mut self.state, message);
        for effect in &effects {
            if let Effect::Send(command) = effect {
                match command {
                    UiCommand::BeginKeyCapture(slot) => {
                        if let Some(snapshot) = self.state.snapshot.as_mut() {
                            snapshot.capture_slot = Some(*slot);
                        }
                    }
                    UiCommand::CancelKeyCapture => {
                        if let Some(snapshot) = self.state.snapshot.as_mut() {
                            snapshot.capture_slot = None;
                        }
                    }
                    _ => {}
                }
                self.sent.push(command.clone());
            }
        }
        effects
    }

    fn frame(&mut self, ui: &mut Ui) {
        let mut messages = Vec::new();
        messages.extend(header::header(ui, &self.state));
        messages.extend(self.cards(ui));
        // The action bar mounts the badge below the cards, as `actions_bar`
        // does; the badge itself stays T6's control.
        header::dirty_badge(ui, &self.state);
        messages.extend(profiles::profile_overlay(ui, &self.state));
        for message in messages {
            for effect in self.dispatch(message) {
                // The app loop defers this to the frame the field mounts.
                if let Effect::FocusProfileName = effect {
                    profiles::focus_profile_name(ui);
                }
            }
        }
    }

    /// The mapping/timing row exactly as `app::settings_cards` lays it out.
    fn cards(&self, ui: &mut Ui) -> Vec<Message> {
        let mut messages = Vec::new();
        if self.state.snapshot.is_none() || self.state.draft.is_none() {
            return messages;
        }
        let gap = theme::SECTION_GAP;
        let width = ((ui.available_width() - gap) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                messages.extend(mapping::key_mappings_card(ui, &self.state));
            });
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                if let Some(draft) = &self.state.draft {
                    let _ = timing::timing_card(
                        ui,
                        &draft.timing,
                        &self.state.inputs,
                        &self.state.editing,
                        self.state.language,
                        Some(PreviewMount {
                            preview: &self.state.preview,
                            awake: self.state.focused,
                            clock_mounted: matches!(self.state.profiles, ProfileDialog::Closed),
                        }),
                        &mut messages,
                    );
                }
            });
        });
        messages
    }
}

fn harness(state: State) -> Harness<'static, PageShell> {
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(1040.0, 1600.0))
        .build_ui_state(
            |ui, shell: &mut PageShell| shell.frame(ui),
            PageShell {
                state,
                sent: Vec::new(),
            },
        )
}

/// A draft edit through the UI, so the badge and mode-dependent controls can
/// be observed against real state. The mode buttons are reached by role and
/// label because their painted inner label is a node of its own.
fn switch_to_press_delay(harness: &mut Harness<'_, PageShell>) {
    harness
        .get_by_role_and_label(Role::Button, "Press Delay")
        .click();
    harness.run();
}

#[test]
fn the_page_controls_are_reachable_by_label() {
    let mut harness = harness(baseline_state());
    harness.run();

    // Header, keycaps, and modes: each of these panics when its label is gone.
    for label in [
        "Profile slots",
        "Language",
        "Engine on/off",
        "UP keycap: W",
        "DOWN keycap: S",
        "LEFT keycap: A",
        "RIGHT keycap: D",
    ] {
        harness.get_by_label(label);
    }
    for mode in ["Immediate", "Press Delay", "Release Delay", "Random Mix"] {
        harness.get_by_role_and_label(Role::Button, mode);
    }

    // A mode change re-renders the conditional groups: the value boxes arrive,
    // the preview leaves, and the edit marks the draft dirty.
    switch_to_press_delay(&mut harness);
    harness.get_by_label("Transition Minimum");
    harness.get_by_label("Transition Maximum");
    assert!(
        harness.query_by_label("Play preview").is_none(),
        "the preview belongs to Immediate mode only"
    );
    harness.get_by_label("Unsaved Draft Changes");
}

#[test]
fn rebinding_a_key_is_driven_by_the_keycap_label() {
    let mut harness = harness(baseline_state());

    harness.get_by_label("UP keycap: W").click();
    harness.run();
    harness.get_by_label("UP keycap: rebinding");
    assert!(
        harness
            .state()
            .sent
            .contains(&UiCommand::BeginKeyCapture(KeySlot::VerticalFirst)),
        "the keycap click must ask the runtime to capture"
    );

    // The runtime captures E; the event enters through the same seam the IPC
    // reader writes to.
    let _ = harness
        .state_mut()
        .dispatch(Message::Ipc(IpcEvent::Message(Box::new(
            UiEvent::KeyCaptured {
                slot: KeySlot::VerticalFirst,
                key: DisplayKey {
                    physical: PhysicalKey::new(0x12, false),
                    name: "E".into(),
                },
            },
        ))));
    harness.run();

    harness.get_by_label("UP keycap: E");
    assert_eq!(
        harness.state().state.draft.as_ref().unwrap().bindings[0],
        PhysicalKey::new(0x12, false),
        "the captured key must land in the draft binding"
    );
    assert_eq!(
        harness
            .state()
            .state
            .snapshot
            .as_ref()
            .unwrap()
            .capture_slot,
        None,
        "the capture slot must clear once the key arrives"
    );
}

#[test]
fn switching_the_mode_by_label_swaps_the_conditional_controls() {
    let mut harness = harness(baseline_state());
    harness.run();

    harness.get_by_label("Play preview");
    assert_eq!(
        harness.state().state.draft.as_ref().unwrap().timing.mode,
        SocdMode::Immediate
    );

    switch_to_press_delay(&mut harness);
    assert_eq!(
        harness.state().state.draft.as_ref().unwrap().timing.mode,
        SocdMode::PressDelay
    );
    harness.get_by_label("New Key Press Delay duration range");
    assert!(
        harness
            .query_by_label("Previous Key Release Delay duration range")
            .is_none(),
        "the release-delay group belongs to Release Delay and Random Mix"
    );

    harness
        .get_by_role_and_label(Role::Button, "Release Delay")
        .click();
    harness.run();
    harness.get_by_label("Previous Key Release Delay duration range");
    assert!(
        harness
            .query_by_label("New Key Press Delay duration range")
            .is_none(),
        "the press-delay group must leave with its mode"
    );
}

#[test]
fn editing_a_timing_value_by_label_commits_the_typed_number() {
    let mut harness = harness(baseline_state());
    switch_to_press_delay(&mut harness);

    // The first click selects the whole value; typing replaces it.
    harness.get_by_label("Transition Minimum").click();
    harness.run();
    harness.get_by_label("Transition Minimum").type_text("15.5");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(
        harness
            .state()
            .state
            .draft
            .as_ref()
            .unwrap()
            .timing
            .socd_transition_min_micros,
        15_500,
        "Enter must commit the typed value to the draft"
    );
    assert_eq!(harness.state().state.inputs.transition_minimum, "15.5");
    assert!(harness.state().state.editing[TimingField::TransitionMinimum.index()]);
    assert_eq!(
        harness
            .get_by_label("Transition Minimum")
            .value()
            .as_deref(),
        Some("15.5"),
        "the box must read the committed value back by label"
    );
}

#[test]
fn toggling_the_filter_by_label_sends_the_command() {
    let mut harness = harness(baseline_state());

    harness.get_by_label("Engine on/off").click();
    harness.run();

    assert!(
        harness
            .state()
            .sent
            .contains(&UiCommand::SetFilterEnabled(false)),
        "the header control must send the toggle through the state layer"
    );
    assert_eq!(harness.state().state.pending_filter, Some(false));
}

#[test]
fn renaming_a_profile_by_label_commits_the_typed_name() {
    let mut harness = harness(baseline_state());

    harness.get_by_label("Profile slots").click();
    harness.run();
    let stored = harness.state().state.stored_profile_name(0).unwrap();

    harness.get_by_label(&stored).click();
    harness.run();
    // The field mounts on the next frame, consumes the focus request, and
    // holds focus with its value selected from the frame after that.
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
            name: "Renamed".into(),
        }),
        "Enter must commit the typed name"
    );
    harness.get_by_label("Renamed");
}
