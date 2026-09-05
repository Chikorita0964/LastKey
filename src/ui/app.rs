use iced::{
    Center, Element, Fill, Length, Size, Subscription, Task, Theme,
    widget::{button, checkbox, column, container, row, rule, scrollable, slider, text},
    window,
};

use crate::{
    protocol::{KeySlot, MeasurementSnapshot, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::Settings,
};

use super::{
    ipc_client::{self, Connection, Event},
    theme,
};

pub fn run() -> iced::Result {
    let initial_view = requested_view();
    iced::application(SettingsApp::new, SettingsApp::update, SettingsApp::view)
        .title(SettingsApp::title)
        .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        .window_size(window_size(initial_view))
        .run()
}

const SETTINGS_WINDOW_SIZE: Size = Size::new(420.0, 720.0);
const MEASUREMENT_WINDOW_SIZE: Size = Size::new(780.0, 620.0);

struct SettingsApp {
    connection: Option<Connection>,
    snapshot: Option<UiSnapshot>,
    draft: Option<Settings>,
    current_view: UiView,
    status: String,
    error: Option<String>,
}

#[derive(Clone, Debug)]
enum Message {
    Ipc(Event),
    RequestSnapshot,
    ShowSettings,
    ShowMeasurement,
    Capture(KeySlot),
    TransitionDelayToggled(bool),
    TransitionMinimumChanged(f32),
    TransitionMaximumChanged(f32),
    PreserveOverlapToggled(bool),
    PreservationRateChanged(f32),
    PreservedMinimumChanged(f32),
    PreservedMaximumChanged(f32),
    Apply,
    Revert,
    RestoreMappingDefaults,
    RestoreAllDefaults,
    ToggleMeasurement,
}

impl SettingsApp {
    fn new() -> Self {
        let current_view = requested_view();
        Self {
            connection: None,
            snapshot: None,
            draft: None,
            current_view,
            status: "Connecting to the LastKey runtime...".into(),
            error: None,
        }
    }

    fn title(&self) -> String {
        match self.current_view {
            UiView::Settings => "LastKey Settings".into(),
            UiView::Measurement => "LastKey Input Timing Results".into(),
        }
    }

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(ipc_client::connect).map(Message::Ipc)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ipc(Event::Connected(connection)) => {
                self.connection = Some(connection);
                self.status = "Connected to the LastKey runtime.".into();
                self.error = None;
                self.send(UiCommand::RequestSnapshot);
            }
            Message::Ipc(Event::Message(event)) => return self.handle_event(*event),
            Message::Ipc(Event::Disconnected(error)) => {
                self.connection = None;
                self.status = "The LastKey runtime is disconnected.".into();
                self.error = Some(error);
            }
            Message::RequestSnapshot => self.send(UiCommand::RequestSnapshot),
            Message::ShowSettings => return self.show_view(UiView::Settings, false),
            Message::ShowMeasurement => return self.show_view(UiView::Measurement, false),
            Message::Capture(slot) => {
                // Timing edits live only in the local draft until Apply.
                // Push them first so the Snapshot answering the capture
                // does not overwrite them with the stale server draft.
                self.sync_draft();
                self.send(UiCommand::BeginKeyCapture(slot));
            }
            Message::TransitionDelayToggled(enabled) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.socd_transition_delay_enabled = enabled;
                }
            }
            Message::TransitionMinimumChanged(milliseconds) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.socd_transition_min_micros = millis_to_micros(milliseconds);
                }
            }
            Message::TransitionMaximumChanged(milliseconds) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.socd_transition_max_micros = millis_to_micros(milliseconds);
                }
            }
            Message::PreserveOverlapToggled(enabled) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.preserve_overlap = enabled;
                }
            }
            Message::PreservationRateChanged(rate) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.overlap_preservation_rate = rate.round().clamp(1.0, 100.0) as u8;
                }
            }
            Message::PreservedMinimumChanged(milliseconds) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.preserved_overlap_min_micros = millis_to_micros(milliseconds);
                }
            }
            Message::PreservedMaximumChanged(milliseconds) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.preserved_overlap_max_micros = millis_to_micros(milliseconds);
                }
            }
            Message::Apply => {
                if let Some(draft) = self.draft.clone() {
                    self.send(UiCommand::UpdateDraft(draft));
                    self.send(UiCommand::Apply);
                    self.status = "Applying settings...".into();
                }
            }
            Message::Revert => self.send(UiCommand::Revert),
            Message::RestoreMappingDefaults => {
                // The controller only resets bindings, but the answering
                // Snapshot would still discard unsent timing edits.
                self.sync_draft();
                self.send(UiCommand::RestoreMappingDefaults);
            }
            Message::RestoreAllDefaults => self.send(UiCommand::RestoreAllDefaults),
            Message::ToggleMeasurement => {
                let active = self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.measurement_active);
                self.sync_draft();
                self.send(if active {
                    UiCommand::StopMeasurement
                } else {
                    UiCommand::StartMeasurement
                });
            }
        }
        Task::none()
    }

    fn handle_event(&mut self, event: UiEvent) -> Task<Message> {
        match event {
            UiEvent::Snapshot(snapshot) => {
                self.set_snapshot(snapshot);
                self.status = "Settings are synchronized with the runtime.".into();
                self.error = None;
            }
            UiEvent::ApplySucceeded(snapshot) => {
                self.set_snapshot(snapshot);
                self.status = "Settings applied.".into();
                self.error = None;
            }
            UiEvent::KeyCaptured { slot, key } => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.bindings[key_slot_index(slot)] = key.physical;
                }
                if let Some(snapshot) = self.snapshot.as_mut() {
                    snapshot.draft.bindings[key_slot_index(slot)] = key.physical;
                    snapshot.keys[key_slot_index(slot)] = key;
                    snapshot.capture_slot = None;
                }
                self.status = "Mapping changed. Select Apply when ready.".into();
            }
            UiEvent::MeasurementUpdated(update) => {
                if let Some(snapshot) = self.snapshot.as_mut() {
                    snapshot.measurement = Some(update);
                    snapshot.measurement_active = true;
                }
            }
            UiEvent::ValidationFailed(error) | UiEvent::RuntimeError(error) => {
                self.error = Some(error.message);
            }
            UiEvent::FocusRequested(view) => {
                return self.show_view(view, true);
            }
            UiEvent::RuntimeShuttingDown => {
                self.connection = None;
                self.status = "The LastKey runtime is shutting down.".into();
                return window::latest().and_then(window::close);
            }
        }
        Task::none()
    }

    fn set_snapshot(&mut self, snapshot: UiSnapshot) {
        self.draft = Some(snapshot.draft.clone());
        self.snapshot = Some(snapshot);
    }

    fn show_view(&mut self, view: UiView, focus: bool) -> Task<Message> {
        self.current_view = view;
        window::latest().and_then(move |id| {
            let mut tasks = vec![window::resize(id, window_size(view))];
            if focus {
                tasks.push(window::set_mode(id, window::Mode::Windowed));
                tasks.push(window::gain_focus(id));
            }
            Task::batch(tasks)
        })
    }

    fn sync_draft(&mut self) {
        if let Some(draft) = self.draft.clone() {
            self.send(UiCommand::UpdateDraft(draft));
        }
    }

    fn send(&mut self, command: UiCommand) {
        let result = self
            .connection
            .as_ref()
            .ok_or_else(|| "The LastKey runtime is disconnected.".to_string())
            .and_then(|connection| connection.send(command));
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let header = column![
            row![
                text("LastKey").size(24).width(Fill),
                button("Settings").on_press(Message::ShowSettings),
                button("Measurement").on_press(Message::ShowMeasurement),
            ]
            .spacing(theme::ROW_GAP)
            .align_y(Center),
            text(&self.status).size(12).color(theme::MUTED_TEXT),
        ]
        .spacing(6);

        let body: Element<_> = match self.current_view {
            UiView::Settings => self.settings_view(),
            UiView::Measurement => self.measurement_view(),
        };
        let mut page = column![header, rule::horizontal(1), body]
            .spacing(theme::SECTION_GAP)
            .padding(theme::PAGE_PADDING)
            .height(Fill);
        if let Some(error) = &self.error {
            page = page.push(text(error).color(theme::ERROR_TEXT));
        }
        container(page).height(Fill).width(Fill).into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let (Some(snapshot), Some(draft)) = (&self.snapshot, &self.draft) else {
            return disconnected_view();
        };
        let timing = &draft.timing;
        let transition_enabled = timing.socd_transition_delay_enabled;
        let preserve_enabled = transition_enabled && timing.preserve_overlap;

        let mappings = column![
            text("Key mappings").size(22),
            key_row("Vertical first", KeySlot::VerticalFirst, snapshot),
            key_row("Vertical second", KeySlot::VerticalSecond, snapshot),
            key_row("Horizontal first", KeySlot::HorizontalFirst, snapshot),
            key_row("Horizontal second", KeySlot::HorizontalSecond, snapshot),
            button("Restore mapping defaults").on_press(Message::RestoreMappingDefaults),
        ]
        .spacing(theme::ROW_GAP);

        let transition = column![
            text("Input timing").size(22),
            checkbox(transition_enabled)
                .label("Enable SOCD transition delay")
                .on_toggle(Message::TransitionDelayToggled),
            timing_slider(
                "Transition minimum",
                timing.socd_transition_min_micros,
                transition_enabled,
                Message::TransitionMinimumChanged,
            ),
            timing_slider(
                "Transition maximum",
                timing.socd_transition_max_micros,
                transition_enabled,
                Message::TransitionMaximumChanged,
            ),
            checkbox(timing.preserve_overlap)
                .label("Preserve physical overlap")
                .on_toggle_maybe(transition_enabled.then_some(Message::PreserveOverlapToggled)),
            value_slider(
                "Preservation probability",
                1.0..=100.0,
                f32::from(timing.overlap_preservation_rate),
                preserve_enabled,
                "%",
                Message::PreservationRateChanged,
            ),
            timing_slider(
                "Preserved overlap minimum",
                timing.preserved_overlap_min_micros,
                preserve_enabled,
                Message::PreservedMinimumChanged,
            ),
            timing_slider(
                "Preserved overlap maximum",
                timing.preserved_overlap_max_micros,
                preserve_enabled,
                Message::PreservedMaximumChanged,
            ),
        ]
        .spacing(theme::ROW_GAP);

        let actions = row![
            button("Apply").on_press(Message::Apply),
            button("Revert").on_press(Message::Revert),
            button("Restore all defaults").on_press(Message::RestoreAllDefaults),
        ]
        .spacing(theme::ROW_GAP);

        scrollable(
            column![
                mappings,
                rule::horizontal(1),
                transition,
                rule::horizontal(1),
                actions
            ]
            .spacing(theme::SECTION_GAP),
        )
        .height(Fill)
        .into()
    }

    fn measurement_view(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return disconnected_view();
        };
        let button_label = if snapshot.measurement_active {
            "Stop measurement"
        } else {
            "Start measurement"
        };
        let mut content = column![
            text("Input timing measurement").size(22),
            text("Measurement observes configured physical key pairs and excludes LastKey output.")
                .color(theme::MUTED_TEXT),
            button(button_label).on_press(Message::ToggleMeasurement),
        ]
        .spacing(theme::SECTION_GAP);

        if let Some(measurement) = snapshot.measurement {
            content = content.push(measurement_results(measurement));
        } else {
            content = content.push(text("No measurement results yet."));
        }
        scrollable(content).height(Fill).into()
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            let _ = connection.send(UiCommand::CloseUiSession);
        }
    }
}

fn key_row<'a>(label: &'a str, slot: KeySlot, snapshot: &'a UiSnapshot) -> Element<'a, Message> {
    let key = &snapshot.keys[key_slot_index(slot)];
    let capture_label = if snapshot.capture_slot == Some(slot) {
        "Press a key…"
    } else {
        "Capture"
    };
    row![
        button(capture_label).on_press(Message::Capture(slot)),
        text(label).width(Fill),
        text(&key.name).width(Length::Fixed(120.0)),
    ]
    .spacing(theme::ROW_GAP)
    .align_y(Center)
    .width(Fill)
    .into()
}

fn timing_slider<'a>(
    label: &'a str,
    micros: u32,
    enabled: bool,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    value_slider(
        label,
        0.0..=1_000.0,
        micros as f32 / 1_000.0,
        enabled,
        " ms",
        on_change,
    )
}

fn value_slider<'a>(
    label: &'a str,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    enabled: bool,
    suffix: &'a str,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    let control: Element<'_, Message> = if enabled {
        slider(range, value, on_change).step(0.1).into()
    } else {
        text("Disabled").color(theme::MUTED_TEXT).width(Fill).into()
    };
    row![
        text(label).width(Length::Fixed(210.0)),
        container(control).width(Fill),
        text(format!("{value:.1}{suffix}")).width(Length::Fixed(90.0)),
    ]
    .spacing(theme::ROW_GAP)
    .align_y(Center)
    .into()
}

fn measurement_results(measurement: MeasurementSnapshot) -> Element<'static, Message> {
    let overlap_frequency = percentage(measurement.overlap_count, measurement.sample_count);
    let near_frequency = percentage(
        measurement.near_simultaneous_count,
        measurement.sample_count,
    );
    column![
        text(format!(
            "{} physical key edges · {} valid paired samples",
            measurement.observed_event_count, measurement.sample_count
        ))
        .size(16),
        text(format!(
            "Physical overlap: {overlap_frequency} · Near-simultaneous (<1 ms): {near_frequency}"
        ))
        .color(theme::MUTED_TEXT),
        statistics_header(),
        statistics_row(
            "Neutral transition",
            measurement.transition_count,
            duration(measurement.transition_median_micros),
            duration(measurement.transition_p10_micros),
            duration(measurement.transition_p90_micros),
            duration(measurement.transition_min_micros),
            duration(measurement.transition_max_micros),
        ),
        statistics_row(
            "Physical overlap",
            measurement.overlap_count,
            duration(measurement.overlap_median_micros),
            duration(measurement.overlap_p10_micros),
            duration(measurement.overlap_p90_micros),
            duration(measurement.overlap_min_micros),
            duration(measurement.overlap_max_micros),
        ),
        statistics_row(
            "Near-simultaneous",
            measurement.near_simultaneous_count,
            "<1 ms".into(),
            "—".into(),
            "—".into(),
            "—".into(),
            "—".into(),
        ),
        rule::horizontal(1),
        text("Suggested SOCD settings").size(18),
        text(format!(
            "SOCD Transition Delay: {}",
            timing_range(measurement.recommended_transition)
        )),
        text(format!(
            "Preserved Overlap Duration: {}",
            timing_range(measurement.recommended_overlap)
        )),
        text(
            "Suggestions use P10 through P50 after excluding near-simultaneous samples. Both configured axes and directions are combined."
        )
        .color(theme::MUTED_TEXT),
    ]
    .spacing(theme::ROW_GAP)
    .into()
}

fn statistics_header() -> Element<'static, Message> {
    row![
        text("Input pattern").width(Length::Fixed(135.0)),
        text("Count").width(Length::Fixed(55.0)),
        text("Median").width(Length::Fixed(80.0)),
        text("P10").width(Length::Fixed(80.0)),
        text("P90").width(Length::Fixed(80.0)),
        text("Minimum").width(Length::Fixed(80.0)),
        text("Maximum").width(Length::Fixed(80.0)),
    ]
    .spacing(6)
    .into()
}

fn statistics_row(
    label: &'static str,
    count: u32,
    median: String,
    p10: String,
    p90: String,
    minimum: String,
    maximum: String,
) -> Element<'static, Message> {
    row![
        text(label).width(Length::Fixed(135.0)),
        text(count).width(Length::Fixed(55.0)),
        text(median).width(Length::Fixed(80.0)),
        text(p10).width(Length::Fixed(80.0)),
        text(p90).width(Length::Fixed(80.0)),
        text(minimum).width(Length::Fixed(80.0)),
        text(maximum).width(Length::Fixed(80.0)),
    ]
    .spacing(6)
    .into()
}

fn disconnected_view<'a>() -> Element<'a, Message> {
    column![
        text("The settings UI is waiting for LastKey.exe."),
        button("Request snapshot").on_press(Message::RequestSnapshot),
    ]
    .spacing(theme::ROW_GAP)
    .into()
}

fn key_slot_index(slot: KeySlot) -> usize {
    match slot {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

fn requested_view() -> UiView {
    requested_view_from(std::env::args())
}

fn requested_view_from(arguments: impl IntoIterator<Item = impl AsRef<str>>) -> UiView {
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument.as_ref() != "--view" {
            continue;
        }
        return match arguments.next().as_ref().map(AsRef::as_ref) {
            Some("measurement") => UiView::Measurement,
            _ => UiView::Settings,
        };
    }
    UiView::Settings
}

fn window_size(view: UiView) -> Size {
    match view {
        UiView::Settings => SETTINGS_WINDOW_SIZE,
        UiView::Measurement => MEASUREMENT_WINDOW_SIZE,
    }
}

fn millis_to_micros(milliseconds: f32) -> u32 {
    (milliseconds.clamp(0.0, 1_000.0) * 10.0).round() as u32 * 100
}

fn duration(micros: Option<u64>) -> String {
    micros.map_or_else(
        || "—".into(),
        |value| format!("{:.1} ms", value as f64 / 1_000.0),
    )
}

fn timing_range(range: Option<crate::protocol::TimingRange>) -> String {
    range.map_or_else(
        || "Collect at least 10 samples".into(),
        |range| {
            format!(
                "{:.1}–{:.1} ms",
                range.min_micros as f64 / 1_000.0,
                range.max_micros as f64 / 1_000.0
            )
        },
    )
}

fn percentage(count: u32, total: u32) -> String {
    if total == 0 {
        "—".into()
    } else {
        format!("{:.1}%", f64::from(count) * 100.0 / f64::from(total))
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::UiView;

    use super::{millis_to_micros, requested_view_from};

    #[test]
    fn slider_values_are_rounded_to_tenth_milliseconds() {
        assert_eq!(millis_to_micros(1.94), 1_900);
        assert_eq!(millis_to_micros(1.95), 2_000);
        assert_eq!(millis_to_micros(1_001.0), 1_000_000);
    }

    #[test]
    fn view_argument_requires_the_named_flag_and_value() {
        assert_eq!(
            requested_view_from(["lastkey-settings", "--view", "measurement"]),
            UiView::Measurement
        );
        assert_eq!(
            requested_view_from(["lastkey-settings", "measurement"]),
            UiView::Settings
        );
        assert_eq!(
            requested_view_from(["lastkey-settings", "--view", "unknown"]),
            UiView::Settings
        );
    }
}
