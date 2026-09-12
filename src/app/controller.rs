use std::sync::mpsc::Receiver;

use crate::{
    core::{LogicalKey, MonitorEvent},
    settings::Settings,
};

use super::{
    AppControllerError, AppSnapshot, CapturedKey, MeasurementUpdate, RuntimeService, SettingsStore,
    state::AppState,
};

pub struct AppController<S, R> {
    state: AppState,
    store: S,
    runtime: R,
}

impl<S, R> AppController<S, R>
where
    S: SettingsStore,
    R: RuntimeService,
{
    pub fn new(settings: Settings, store: S, runtime: R) -> Self {
        Self {
            state: AppState::new(settings),
            store,
            runtime,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.state.snapshot()
    }

    pub fn replace_draft(&mut self, draft: Settings) {
        self.state.draft = draft;
    }

    /// Loading is an explicit activation transaction, independent of an unapplied draft.
    pub fn load_profile(&mut self, index: u8) -> Result<AppSnapshot, AppControllerError> {
        let next = self
            .state
            .saved
            .select_profile(index)
            .map_err(AppControllerError::InvalidSettings)?;
        self.stop_monitor()?;
        let previous_draft = std::mem::replace(&mut self.state.draft, next);
        let result = self.apply();
        if result.is_err() {
            self.state.draft = previous_draft;
        }
        result
    }

    /// Profile names are metadata: persist them without activating an unrelated draft.
    pub fn rename_profile(
        &mut self,
        index: u8,
        name: String,
    ) -> Result<AppSnapshot, AppControllerError> {
        let mut next = self.state.saved.clone();
        let mut bank = next.profile_bank();
        let slot =
            bank.slots
                .get_mut(usize::from(index))
                .ok_or(AppControllerError::InvalidSettings(
                    crate::settings::SettingsError::InvalidProfile,
                ))?;
        slot.name = name.trim().into();
        next.profiles = Some(Box::new(bank));
        next.validate()
            .map_err(AppControllerError::InvalidSettings)?;
        self.store
            .save(&next)
            .map_err(AppControllerError::Persistence)?;
        self.state.draft.profiles = next.profiles.clone();
        self.state.saved = next;
        Ok(self.snapshot())
    }

    pub fn revert(&mut self) -> Result<AppSnapshot, AppControllerError> {
        self.runtime
            .cancel_key_capture()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_capture();
        self.state.draft = self.state.saved.clone();
        Ok(self.snapshot())
    }

    pub fn restore_all_defaults(&mut self) -> Result<AppSnapshot, AppControllerError> {
        self.runtime
            .cancel_key_capture()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_capture();
        self.state.draft = Settings {
            profiles: self.state.draft.profiles.clone(),
            ..Settings::default()
        };
        Ok(self.snapshot())
    }

    pub fn restore_mapping_defaults(&mut self) -> Result<AppSnapshot, AppControllerError> {
        self.runtime
            .cancel_key_capture()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_capture();
        self.state.draft.bindings = Settings::default().bindings;
        Ok(self.snapshot())
    }

    pub fn apply(&mut self) -> Result<AppSnapshot, AppControllerError> {
        let mut next = self.state.draft.clone();
        next.sync_active_profile()
            .map_err(AppControllerError::InvalidSettings)?;
        next.validate()
            .map_err(AppControllerError::InvalidSettings)?;

        let previous = self.state.saved.clone();
        self.store
            .save(&next)
            .map_err(AppControllerError::Persistence)?;

        if let Err(runtime_error) = self.runtime.apply(next.clone()) {
            return self.reconcile_apply_outcome(next, previous, runtime_error);
        }

        self.state.saved = next.clone();
        self.state.draft = next;
        self.state.invalidate_capture();
        self.state.invalidate_measurement();
        Ok(self.snapshot())
    }

    /// Confirms whether a failed Apply still activated before deciding
    /// between rollback and adoption. The fence queues behind the late Apply,
    /// so its answer is authoritative about the outcome.
    fn reconcile_apply_outcome(
        &mut self,
        next: Settings,
        previous: Settings,
        runtime_error: String,
    ) -> Result<AppSnapshot, AppControllerError> {
        match self.runtime.active_settings() {
            // Late activation won: the candidate file is already on disk.
            Ok(active) if active == next => {
                self.state.saved = next.clone();
                self.state.draft = next;
                self.state.invalidate_capture();
                self.state.invalidate_measurement();
                Ok(self.snapshot())
            }
            // The engine never activated: restore the previous file.
            Ok(_) => match self.store.save(&previous) {
                Ok(()) => Err(AppControllerError::Runtime(runtime_error)),
                Err(rollback) => Err(AppControllerError::RuntimeWithRollbackFailure {
                    runtime: runtime_error,
                    rollback,
                }),
            },
            // The engine state is unknowable: restore the file but say so.
            Err(fence_error) => match self.store.save(&previous) {
                Ok(()) => Err(AppControllerError::RuntimeUnconfirmed {
                    runtime: runtime_error,
                    fence: fence_error,
                }),
                Err(rollback) => Err(AppControllerError::RuntimeWithRollbackFailure {
                    runtime: runtime_error,
                    rollback: format!("{rollback}; confirmation also failed: {fence_error}"),
                }),
            },
        }
    }

    pub fn begin_key_capture(
        &mut self,
        slot: LogicalKey,
    ) -> Result<(u64, Receiver<CapturedKey>), AppControllerError> {
        let receiver = self
            .runtime
            .begin_key_capture()
            .map_err(AppControllerError::Runtime)?;
        self.state.capture_generation = self.state.capture_generation.wrapping_add(1);
        self.state.capture_slot = Some(slot);
        Ok((self.state.capture_generation, receiver))
    }

    pub fn complete_key_capture(
        &mut self,
        generation: u64,
        captured: CapturedKey,
    ) -> Option<AppSnapshot> {
        if self.state.capture_generation != generation {
            return None;
        }
        let slot = self.state.capture_slot.take()?;
        self.state.draft.set_binding(slot, captured.physical);
        Some(self.snapshot())
    }

    pub fn cancel_key_capture(&mut self) -> Result<(), AppControllerError> {
        self.runtime
            .cancel_key_capture()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_capture();
        Ok(())
    }

    pub fn start_measurement(
        &mut self,
    ) -> Result<(u64, Receiver<MeasurementUpdate>), AppControllerError> {
        let receiver = self
            .runtime
            .start_measurement()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_capture();
        self.state.measurement_generation = self.state.measurement_generation.wrapping_add(1);
        self.state.measurement_active = true;
        self.state.measurement = None;
        Ok((self.state.measurement_generation, receiver))
    }

    pub fn update_measurement(&mut self, generation: u64, update: MeasurementUpdate) -> bool {
        if !self.is_current_measurement(generation) {
            return false;
        }
        self.state.measurement = Some(update);
        true
    }

    pub fn is_current_measurement(&self, generation: u64) -> bool {
        self.state.measurement_active && self.state.measurement_generation == generation
    }

    pub fn stop_measurement(&mut self) -> Result<AppSnapshot, AppControllerError> {
        let final_update = self
            .runtime
            .stop_measurement()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_measurement();
        if final_update.is_some() {
            self.state.measurement = final_update;
        }
        Ok(self.snapshot())
    }

    /// Clears a stopped session. Restarting a live session uses start_measurement.
    pub fn clear_measurement(&mut self) -> Result<AppSnapshot, AppControllerError> {
        self.stop_measurement()?;
        self.state.measurement = None;
        Ok(self.snapshot())
    }

    pub fn set_filter_enabled(&mut self, enabled: bool) -> Result<(), AppControllerError> {
        self.runtime
            .set_filter_enabled(enabled)
            .map_err(AppControllerError::Runtime)
    }

    pub fn filter_enabled(&self) -> Result<bool, AppControllerError> {
        self.runtime
            .filter_enabled()
            .map_err(AppControllerError::Runtime)
    }

    pub fn start_monitor(&mut self) -> Result<(u64, Receiver<MonitorEvent>), AppControllerError> {
        let receiver = self
            .runtime
            .start_monitor()
            .map_err(AppControllerError::Runtime)?;
        self.state.monitor_generation = self.state.monitor_generation.wrapping_add(1);
        self.state.monitor_active = true;
        Ok((self.state.monitor_generation, receiver))
    }

    /// Unlike `update_measurement`, the monitor retains nothing: events stream
    /// straight to the consumer, so the pump only asks whether the session it
    /// received them for is still the current one.
    pub fn is_current_monitor(&self, generation: u64) -> bool {
        self.state.monitor_active && self.state.monitor_generation == generation
    }

    pub fn stop_monitor(&mut self) -> Result<(), AppControllerError> {
        self.runtime
            .stop_monitor()
            .map_err(AppControllerError::Runtime)?;
        self.state.invalidate_monitor();
        Ok(())
    }

    pub fn close_ui_session(&mut self) -> Result<(), AppControllerError> {
        let capture_result = self.runtime.cancel_key_capture();
        // Stop unconditionally: a start whose acknowledgement timed out may
        // still have armed the engine afterwards, leaving measurement_active
        // false here. Stopping an inactive session is a no-op.
        let measurement_result = self.runtime.stop_measurement().map(|_| ());
        // Same rule for the monitor: an orphaned tap would stream into a
        // session that no longer exists.
        let monitor_result = self.runtime.stop_monitor();
        self.state.invalidate_capture();
        self.state.invalidate_measurement();
        self.state.invalidate_monitor();

        capture_result
            .and(measurement_result)
            .and(monitor_result)
            .map_err(AppControllerError::Runtime)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
        sync::mpsc::{self, Receiver, Sender},
    };

    use crate::{
        app::{CapturedKey, MeasurementUpdate, RuntimeService, SettingsStore},
        core::{LogicalKey, MonitorEvent, PhysicalKey},
        settings::{Settings, SocdMode},
    };

    use super::{AppController, AppControllerError};

    #[derive(Clone, Default)]
    struct MockStore {
        saves: Rc<RefCell<Vec<Settings>>>,
        results: Rc<RefCell<VecDeque<Result<(), String>>>>,
    }

    impl SettingsStore for MockStore {
        fn save(&self, settings: &Settings) -> Result<(), String> {
            self.saves.borrow_mut().push(settings.clone());
            self.results.borrow_mut().pop_front().unwrap_or(Ok(()))
        }
    }

    #[derive(Clone, Default)]
    struct MockRuntime {
        applied: Rc<RefCell<Vec<Settings>>>,
        active: Rc<RefCell<Settings>>,
        apply_results: Rc<RefCell<VecDeque<Result<(), String>>>>,
        active_results: Rc<RefCell<VecDeque<Result<Settings, String>>>>,
        capture_sender: Rc<RefCell<Option<Sender<CapturedKey>>>>,
        measurement_sender: Rc<RefCell<Option<Sender<MeasurementUpdate>>>>,
        final_measurement: Rc<RefCell<Option<MeasurementUpdate>>>,
        capture_cancellations: Rc<Cell<u32>>,
        measurement_stops: Rc<Cell<u32>>,
        filter: Rc<Cell<bool>>,
        monitor_sender: Rc<RefCell<Option<Sender<MonitorEvent>>>>,
        monitor_stops: Rc<Cell<u32>>,
    }

    impl RuntimeService for MockRuntime {
        fn apply(&self, settings: Settings) -> Result<(), String> {
            self.applied.borrow_mut().push(settings.clone());
            let result = self
                .apply_results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(()));
            if result.is_ok() {
                *self.active.borrow_mut() = settings;
            }
            result
        }

        fn active_settings(&self) -> Result<Settings, String> {
            self.active_results
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(self.active.borrow().clone()))
        }

        fn begin_key_capture(&self) -> Result<Receiver<CapturedKey>, String> {
            let (sender, receiver) = mpsc::channel();
            *self.capture_sender.borrow_mut() = Some(sender);
            Ok(receiver)
        }

        fn cancel_key_capture(&self) -> Result<(), String> {
            self.capture_cancellations
                .set(self.capture_cancellations.get() + 1);
            self.capture_sender.borrow_mut().take();
            Ok(())
        }

        fn start_measurement(&self) -> Result<Receiver<MeasurementUpdate>, String> {
            let (sender, receiver) = mpsc::channel();
            *self.measurement_sender.borrow_mut() = Some(sender);
            Ok(receiver)
        }

        fn stop_measurement(&self) -> Result<Option<MeasurementUpdate>, String> {
            self.measurement_stops.set(self.measurement_stops.get() + 1);
            self.measurement_sender.borrow_mut().take();
            Ok(self.final_measurement.borrow_mut().take())
        }

        fn set_filter_enabled(&self, enabled: bool) -> Result<(), String> {
            self.filter.set(enabled);
            Ok(())
        }

        fn filter_enabled(&self) -> Result<bool, String> {
            Ok(self.filter.get())
        }

        fn start_monitor(&self) -> Result<Receiver<MonitorEvent>, String> {
            let (sender, receiver) = mpsc::channel();
            *self.monitor_sender.borrow_mut() = Some(sender);
            Ok(receiver)
        }

        fn stop_monitor(&self) -> Result<(), String> {
            self.monitor_stops.set(self.monitor_stops.get() + 1);
            self.monitor_sender.borrow_mut().take();
            Ok(())
        }
    }

    fn controller() -> (
        AppController<MockStore, MockRuntime>,
        MockStore,
        MockRuntime,
    ) {
        let store = MockStore::default();
        let runtime = MockRuntime::default();
        // Mirror the engine default: the filter starts on.
        runtime.filter.set(true);
        (
            AppController::new(Settings::default(), store.clone(), runtime.clone()),
            store,
            runtime,
        )
    }

    fn changed_settings() -> Settings {
        let mut settings = Settings::default();
        settings.timing.mode = SocdMode::PressDelay;
        settings
    }

    #[test]
    fn apply_persists_activates_and_publishes_the_authoritative_snapshot() {
        let (mut controller, store, runtime) = controller();
        let changed = changed_settings();
        controller.replace_draft(changed.clone());

        let snapshot = controller.apply().expect("apply succeeds");

        assert_eq!(snapshot.saved, changed);
        assert_eq!(snapshot.draft, changed);
        assert_eq!(&*store.saves.borrow(), std::slice::from_ref(&changed));
        assert_eq!(&*runtime.applied.borrow(), std::slice::from_ref(&changed));
        assert_eq!(*runtime.active.borrow(), changed);
    }

    #[test]
    fn validation_failure_does_not_persist_or_activate() {
        let (mut controller, store, runtime) = controller();
        let mut invalid = Settings::default();
        invalid.bindings[1] = invalid.bindings[0];
        controller.replace_draft(invalid);

        assert!(matches!(
            controller.apply(),
            Err(AppControllerError::InvalidSettings(_))
        ));
        assert!(store.saves.borrow().is_empty());
        assert!(runtime.applied.borrow().is_empty());
    }

    #[test]
    fn persistence_failure_leaves_saved_and_active_settings_unchanged() {
        let (mut controller, store, runtime) = controller();
        store
            .results
            .borrow_mut()
            .push_back(Err("disk full".into()));
        controller.replace_draft(changed_settings());

        assert!(matches!(
            controller.apply(),
            Err(AppControllerError::Persistence(error)) if error == "disk full"
        ));
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.saved, Settings::default());
        assert_eq!(*runtime.active.borrow(), Settings::default());
        assert!(runtime.applied.borrow().is_empty());
    }

    #[test]
    fn activation_failure_restores_the_previous_persisted_settings() {
        let (mut controller, store, runtime) = controller();
        runtime
            .apply_results
            .borrow_mut()
            .push_back(Err("service stopped".into()));
        let changed = changed_settings();
        controller.replace_draft(changed.clone());

        assert!(matches!(
            controller.apply(),
            Err(AppControllerError::Runtime(error)) if error == "service stopped"
        ));
        assert_eq!(&*store.saves.borrow(), &[changed, Settings::default()]);
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.saved, Settings::default());
        assert_eq!(*runtime.active.borrow(), Settings::default());
    }

    #[test]
    fn late_activation_after_timeout_is_adopted_not_rolled_back() {
        let (mut controller, store, runtime) = controller();
        runtime
            .apply_results
            .borrow_mut()
            .push_back(Err("service timeout".into()));
        let changed = changed_settings();
        controller.replace_draft(changed.clone());
        // The engine activated despite the lost acknowledgement.
        *runtime.active.borrow_mut() = changed.clone();

        let snapshot = controller.apply().expect("late activation is adopted");

        assert_eq!(snapshot.saved, changed);
        assert_eq!(*runtime.active.borrow(), changed);
        // No rollback save: the candidate file already on disk is correct.
        assert_eq!(&*store.saves.borrow(), std::slice::from_ref(&changed));
    }

    #[test]
    fn unconfirmable_activation_reports_uncertainty_after_rollback() {
        let (mut controller, store, runtime) = controller();
        runtime
            .apply_results
            .borrow_mut()
            .push_back(Err("service timeout".into()));
        runtime
            .active_results
            .borrow_mut()
            .push_back(Err("service stopped".into()));
        controller.replace_draft(changed_settings());

        assert!(matches!(
            controller.apply(),
            Err(AppControllerError::RuntimeUnconfirmed { .. })
        ));
        assert_eq!(store.saves.borrow().len(), 2);
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.saved, Settings::default());
        assert_eq!(*runtime.active.borrow(), Settings::default());
    }

    #[test]
    fn revert_cancels_capture_and_restores_the_saved_draft() {
        let (mut controller, _store, runtime) = controller();
        controller.replace_draft(changed_settings());
        let (generation, _receiver) = controller
            .begin_key_capture(LogicalKey::VerticalFirst)
            .expect("capture starts");

        let snapshot = controller.revert().expect("revert succeeds");

        assert_eq!(snapshot.draft, Settings::default());
        assert_eq!(runtime.capture_cancellations.get(), 1);
        assert!(
            controller
                .complete_key_capture(
                    generation,
                    CapturedKey {
                        physical: PhysicalKey::new(0x2C, false),
                        name: "Z".into(),
                    }
                )
                .is_none()
        );
    }

    #[test]
    fn restoring_all_defaults_resets_the_complete_draft() {
        let (mut controller, _store, _runtime) = controller();
        let mut changed = changed_settings();
        changed.bindings.rotate_left(1);
        controller.replace_draft(changed);

        let snapshot = controller
            .restore_all_defaults()
            .expect("defaults are restored");

        assert_eq!(snapshot.draft, Settings::default());
    }

    #[test]
    fn restoring_mapping_defaults_preserves_draft_timing() {
        let (mut controller, _store, _runtime) = controller();
        let mut changed = changed_settings();
        changed.bindings.rotate_left(1);
        let timing = changed.timing.clone();
        controller.replace_draft(changed);

        let snapshot = controller
            .restore_mapping_defaults()
            .expect("mapping defaults are restored");

        assert_eq!(snapshot.draft.bindings, Settings::default().bindings);
        assert_eq!(snapshot.draft.timing, timing);
    }

    #[test]
    fn key_capture_updates_only_the_requested_draft_binding() {
        let (mut controller, _store, _runtime) = controller();
        let (generation, _receiver) = controller
            .begin_key_capture(LogicalKey::HorizontalFirst)
            .expect("capture starts");
        let physical = PhysicalKey::new(0x2C, false);

        let snapshot = controller
            .complete_key_capture(
                generation,
                CapturedKey {
                    physical,
                    name: "Z".into(),
                },
            )
            .expect("capture is current");

        assert_eq!(
            snapshot.draft.binding(LogicalKey::HorizontalFirst),
            physical
        );
        assert_eq!(
            snapshot.draft.binding(LogicalKey::HorizontalSecond),
            Settings::default().binding(LogicalKey::HorizontalSecond)
        );
    }

    #[test]
    fn measurement_updates_are_generation_checked_and_final_results_remain_visible() {
        let (mut controller, _store, runtime) = controller();
        let (generation, _receiver) = controller.start_measurement().expect("measurement starts");
        let live = MeasurementUpdate {
            observed_event_count: 4,
            ..MeasurementUpdate::default()
        };
        assert!(controller.update_measurement(generation, live));

        let final_update = MeasurementUpdate {
            observed_event_count: 6,
            ..MeasurementUpdate::default()
        };
        *runtime.final_measurement.borrow_mut() = Some(final_update);
        let snapshot = controller.stop_measurement().expect("measurement stops");

        assert!(!snapshot.measurement_active);
        assert_eq!(snapshot.measurement, Some(final_update));
        assert!(!controller.update_measurement(generation, live));
    }

    #[test]
    fn stopped_measurement_generation_is_no_longer_current() {
        let (mut controller, _store, _runtime) = controller();
        let (generation, _receiver) = controller.start_measurement().expect("measurement starts");
        assert!(controller.is_current_measurement(generation));

        controller.stop_measurement().expect("measurement stops");
        assert!(!controller.is_current_measurement(generation));
    }

    #[test]
    fn filter_toggle_round_trips_through_the_runtime() {
        let (mut controller, _store, runtime) = controller();
        assert!(controller.filter_enabled().expect("filter reads"));

        controller
            .set_filter_enabled(false)
            .expect("filter disables");
        assert!(!controller.filter_enabled().expect("filter reads"));
        assert!(!runtime.filter.get());

        controller.set_filter_enabled(true).expect("filter enables");
        assert!(controller.filter_enabled().expect("filter reads"));
    }

    #[test]
    fn stopped_monitor_generation_is_no_longer_current() {
        let (mut controller, _store, runtime) = controller();
        let (generation, _receiver) = controller.start_monitor().expect("monitor starts");
        assert!(controller.is_current_monitor(generation));
        assert!(runtime.monitor_sender.borrow().is_some());

        controller.stop_monitor().expect("monitor stops");
        assert!(!controller.is_current_monitor(generation));
        assert_eq!(runtime.monitor_stops.get(), 1);
    }

    #[test]
    fn closing_the_ui_session_cancels_transient_runtime_work_only() {
        let (mut controller, _store, runtime) = controller();
        controller
            .begin_key_capture(LogicalKey::VerticalSecond)
            .expect("capture starts");
        controller.start_measurement().expect("measurement starts");

        controller.close_ui_session().expect("session closes");

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.saved, Settings::default());
        assert!(!snapshot.measurement_active);
        assert_eq!(runtime.capture_cancellations.get(), 1);
        assert_eq!(runtime.measurement_stops.get(), 1);
        assert_eq!(runtime.monitor_stops.get(), 1);
    }

    #[test]
    fn closing_an_idle_session_still_stops_runtime_measurement() {
        let (mut controller, _store, runtime) = controller();

        controller.close_ui_session().expect("session closes");

        // A start whose acknowledgement timed out may still have armed the
        // engine afterwards; the flag cannot be trusted here.
        assert_eq!(runtime.measurement_stops.get(), 1);
        assert!(!controller.snapshot().measurement_active);
    }

    #[test]
    fn profile_load_activates_and_failed_load_preserves_the_unapplied_draft() {
        let (mut controller, store, runtime) = controller();
        let loaded = controller.load_profile(2).expect("profile activates");
        assert_eq!(*runtime.active.borrow(), loaded.saved);
        assert_eq!(store.saves.borrow().last(), Some(&loaded.saved));
        let mut edited = loaded.draft.clone();
        edited.timing.socd_transition_min_micros = 3_000;
        controller.replace_draft(edited.clone());
        runtime
            .apply_results
            .borrow_mut()
            .push_back(Err("activation failed".into()));
        assert!(controller.load_profile(3).is_err());
        assert_eq!(controller.snapshot().draft, edited);
        assert_eq!(controller.snapshot().saved, loaded.saved);
        assert_eq!(*runtime.active.borrow(), loaded.saved);
        assert_eq!(store.saves.borrow().last(), Some(&loaded.saved));
    }

    #[test]
    fn rename_never_activates_draft_and_apply_saves_only_the_active_profile() {
        let (mut controller, _, runtime) = controller();
        let loaded = controller.load_profile(1).expect("profile activates");
        let mut edited = loaded.draft.clone();
        edited.timing.socd_transition_max_micros = 8_000;
        controller.replace_draft(edited);
        let renamed = controller
            .rename_profile(1, "  Custom  ".into())
            .expect("rename persists");
        assert_eq!(*runtime.active.borrow(), loaded.saved);
        assert_eq!(renamed.draft.timing.socd_transition_max_micros, 8_000);
        let applied = controller.apply().expect("draft applies");
        let bank = applied.saved.profile_bank();
        assert_eq!(bank.slots[1].name, "Custom");
        assert_eq!(bank.slots[1].timing, applied.saved.timing);
        assert_eq!(bank.slots[0], loaded.saved.profile_bank().slots[0]);
        assert_eq!(
            controller.restore_all_defaults().unwrap().draft.profiles,
            applied.saved.profiles
        );
    }
}
