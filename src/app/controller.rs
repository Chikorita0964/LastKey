use std::sync::mpsc::Receiver;

use crate::{core::LogicalKey, settings::Settings};

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
        self.state.draft = Settings::default();
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
        let next = self.state.draft.clone();
        next.validate()
            .map_err(AppControllerError::InvalidSettings)?;

        let previous = self.state.saved.clone();
        self.store
            .save(&next)
            .map_err(AppControllerError::Persistence)?;

        if let Err(runtime_error) = self.runtime.apply(next.clone()) {
            return match self.store.save(&previous) {
                Ok(()) => Err(AppControllerError::Runtime(runtime_error)),
                Err(rollback) => Err(AppControllerError::RuntimeWithRollbackFailure {
                    runtime: runtime_error,
                    rollback,
                }),
            };
        }

        self.state.saved = next.clone();
        self.state.active = next.clone();
        self.state.draft = next;
        self.state.invalidate_capture();
        self.state.invalidate_measurement();
        Ok(self.snapshot())
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

    pub fn close_ui_session(&mut self) -> Result<(), AppControllerError> {
        let capture_result = self.runtime.cancel_key_capture();
        let measurement_result = if self.state.measurement_active {
            self.runtime.stop_measurement().map(|_| ())
        } else {
            Ok(())
        };
        self.state.invalidate_capture();
        self.state.invalidate_measurement();

        capture_result
            .and(measurement_result)
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
        core::{LogicalKey, PhysicalKey},
        settings::Settings,
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
        apply_results: Rc<RefCell<VecDeque<Result<(), String>>>>,
        capture_sender: Rc<RefCell<Option<Sender<CapturedKey>>>>,
        measurement_sender: Rc<RefCell<Option<Sender<MeasurementUpdate>>>>,
        final_measurement: Rc<RefCell<Option<MeasurementUpdate>>>,
        capture_cancellations: Rc<Cell<u32>>,
        measurement_stops: Rc<Cell<u32>>,
    }

    impl RuntimeService for MockRuntime {
        fn apply(&self, settings: Settings) -> Result<(), String> {
            self.applied.borrow_mut().push(settings);
            self.apply_results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(()))
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
    }

    fn controller() -> (
        AppController<MockStore, MockRuntime>,
        MockStore,
        MockRuntime,
    ) {
        let store = MockStore::default();
        let runtime = MockRuntime::default();
        (
            AppController::new(Settings::default(), store.clone(), runtime.clone()),
            store,
            runtime,
        )
    }

    fn changed_settings() -> Settings {
        let mut settings = Settings::default();
        settings.timing.socd_transition_delay_enabled = true;
        settings
    }

    #[test]
    fn apply_persists_activates_and_publishes_the_authoritative_snapshot() {
        let (mut controller, store, runtime) = controller();
        let changed = changed_settings();
        controller.replace_draft(changed.clone());

        let snapshot = controller.apply().expect("apply succeeds");

        assert_eq!(snapshot.saved, changed);
        assert_eq!(snapshot.active, changed);
        assert_eq!(snapshot.draft, changed);
        assert_eq!(&*store.saves.borrow(), std::slice::from_ref(&changed));
        assert_eq!(&*runtime.applied.borrow(), &[changed]);
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
        assert_eq!(snapshot.active, Settings::default());
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
        assert_eq!(snapshot.active, Settings::default());
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
    fn closing_the_ui_session_cancels_transient_runtime_work_only() {
        let (mut controller, _store, runtime) = controller();
        controller
            .begin_key_capture(LogicalKey::VerticalSecond)
            .expect("capture starts");
        controller.start_measurement().expect("measurement starts");

        controller.close_ui_session().expect("session closes");

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.active, Settings::default());
        assert!(!snapshot.measurement_active);
        assert_eq!(runtime.capture_cancellations.get(), 1);
        assert_eq!(runtime.measurement_stops.get(), 1);
    }
}
