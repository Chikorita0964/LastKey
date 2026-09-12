use std::{
    io,
    os::windows::io::AsRawHandle,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    app::{AppController, AppControllerError, CapturedKey, FileSettingsStore, MeasurementUpdate},
    core::{LogicalKey, MonitorEvent},
    protocol::{DisplayKey, ErrorView, KeySlot, UiCommand, UiEvent, UiSnapshot},
};
use windows::Win32::{
    Foundation::{HANDLE, LPARAM, WPARAM},
    System::{IO::CancelSynchronousIo, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP},
};

pub const FILTER_STATUS_MESSAGE: u32 = WM_APP + 3;

use super::{
    InputService,
    ipc::{NamedPipeServer, PipeConnection, SETTINGS_PIPE_NAME, ipc_poll_interval},
    physical_key_name,
};

type Controller = Arc<Mutex<AppController<FileSettingsStore, InputService>>>;
/// Locks the controller. Poisoning means an earlier handler panicked while
/// holding it, so the session state is unknown and continuing to serve a UI
/// from it would publish garbage; failing fast is the only safe answer.
fn locked(
    controller: &Controller,
) -> MutexGuard<'_, AppController<FileSettingsStore, InputService>> {
    controller
        .lock()
        .expect("app controller mutex is not poisoned")
}
/// Pump-bound events. Capture completions travel through the same queue as
/// outbound replies so the pump validates, applies, and answers them in one
/// defined sequence instead of racing a later Revert.
enum ServerEvent {
    FilterChanged,
    MonitorUpdated {
        generation: u64,
        event: Box<MonitorEvent>,
    },
    Out(Box<UiEvent>),
    KeyCaptureDone {
        generation: u64,
        slot: KeySlot,
        captured: CapturedKey,
    },
    MeasurementUpdated {
        generation: u64,
        update: Box<MeasurementUpdate>,
    },
}
type EventQueue = Sender<ServerEvent>;
type SharedQueue = Arc<Mutex<Option<EventQueue>>>;

pub struct UiServer {
    stopping: Arc<AtomicBool>,
    client: Arc<Mutex<Option<EventQueue>>>,
    thread: Option<JoinHandle<()>>,
}

impl UiServer {
    pub fn start(controller: Controller) -> io::Result<Self> {
        let server = NamedPipeServer::new(SETTINGS_PIPE_NAME)?;
        // The runtime creates this server on its tray message-loop thread.
        let main_thread = unsafe { GetCurrentThreadId() };
        let stopping = Arc::new(AtomicBool::new(false));
        let stopping_for_thread = Arc::clone(&stopping);
        let client = Arc::new(Mutex::new(None));
        let client_for_thread = Arc::clone(&client);
        let thread = thread::Builder::new()
            .name("lastkey-ui-server".into())
            .spawn(move || {
                run(
                    server,
                    controller,
                    stopping_for_thread,
                    client_for_thread,
                    main_thread,
                )
            })?;
        Ok(Self {
            stopping,
            client,
            thread: Some(thread),
        })
    }

    pub fn request_focus(&self, view: crate::protocol::UiView) -> bool {
        self.send_event(UiEvent::FocusRequested(view))
    }

    pub fn notify_filter_changed(&self) -> bool {
        self.client
            .lock()
            .expect("settings IPC client mutex is not poisoned")
            .as_ref()
            .is_some_and(|queue| queue.send(ServerEvent::FilterChanged).is_ok())
    }

    pub fn notify_shutdown(&self) -> bool {
        self.send_event(UiEvent::RuntimeShuttingDown)
    }

    fn send_event(&self, event: UiEvent) -> bool {
        self.client
            .lock()
            .expect("settings IPC client mutex is not poisoned")
            .as_ref()
            .is_some_and(|queue| queue.send(ServerEvent::Out(Box::new(event))).is_ok())
    }
}

impl Drop for UiServer {
    fn drop(&mut self) {
        let _ = self.notify_shutdown();
        self.stopping.store(true, Ordering::Release);
        if let Some(queue) = self
            .client
            .lock()
            .expect("settings IPC client mutex is not poisoned")
            .take()
        {
            drop(queue);
        }
        if let Some(thread) = self.thread.take() {
            let thread_handle = HANDLE(thread.as_raw_handle());
            let _ = unsafe { CancelSynchronousIo(thread_handle) };
            let _ = PipeConnection::connect(SETTINGS_PIPE_NAME, Duration::from_millis(250));
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                let _ = unsafe { CancelSynchronousIo(thread_handle) };
                thread::sleep(Duration::from_millis(25));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

fn run(
    mut server: NamedPipeServer,
    controller: Controller,
    stopping: Arc<AtomicBool>,
    client: SharedQueue,
    main_thread: u32,
) {
    while !stopping.load(Ordering::Acquire) {
        let Ok(connection) = server.accept() else {
            if !stopping.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(100));
            }
            continue;
        };
        if stopping.load(Ordering::Acquire) {
            break;
        }
        serve_connection(connection, &controller, &client, &stopping, main_thread);
        *client
            .lock()
            .expect("settings IPC client mutex is not poisoned") = None;
        let _ = locked(&controller).close_ui_session();
    }
}

fn serve_connection(
    mut connection: PipeConnection,
    controller: &Controller,
    client: &SharedQueue,
    stopping: &Arc<AtomicBool>,
    main_thread: u32,
) {
    // Every pipe syscall for this session runs on this thread. A pending
    // blocking read on one handle stalls writes on a duplicate handle of
    // the same synchronous pipe, so the session is never split across
    // threads: wait on the outbound queue, drain outbound, Peek-gated
    // inbound read.
    let (event_sender, event_receiver) = mpsc::channel::<ServerEvent>();
    *client
        .lock()
        .expect("settings IPC client mutex is not poisoned") = Some(event_sender.clone());
    let mut last_activity = Instant::now();

    loop {
        // Wait on the outbound queue with a timeout so worker events leave
        // immediately; only inbound discovery is bounded by the poll interval.
        let interval = ipc_poll_interval(last_activity.elapsed());
        let mut pending = match event_receiver.recv_timeout(interval) {
            Ok(event) => Some(event),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            // The queue sender is owned by this session and outlives it, so
            // disconnection cannot happen here; sleep to preserve the shape.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                thread::sleep(interval);
                None
            }
        };
        while let Some(event) = pending {
            last_activity = Instant::now();
            match event {
                ServerEvent::FilterChanged => {
                    // Query at dispatch, so queued tray notifications cannot revert a newer UI toggle.
                    let result = locked(controller).filter_enabled();
                    let event = match result {
                        Ok(enabled) => UiEvent::FilterChanged(enabled),
                        Err(error) => {
                            UiEvent::RuntimeError(error_view("filter-state-failed", error))
                        }
                    };
                    if !send_reply(&mut connection, &event) {
                        return;
                    }
                }
                ServerEvent::MonitorUpdated { generation, event } => {
                    let accepted = locked(controller).is_current_monitor(generation);
                    if accepted
                        && !send_reply(&mut connection, &UiEvent::MonitorUpdated((*event).into()))
                    {
                        return;
                    }
                }
                ServerEvent::Out(event) => {
                    if !send_reply(&mut connection, &event) {
                        return;
                    }
                }
                ServerEvent::KeyCaptureDone {
                    generation,
                    slot,
                    captured,
                } => {
                    if !complete_key_capture(
                        controller,
                        &mut connection,
                        generation,
                        slot,
                        captured,
                    ) {
                        return;
                    }
                }
                ServerEvent::MeasurementUpdated { generation, update } => {
                    if !accept_measurement_update(controller, &mut connection, generation, *update)
                    {
                        return;
                    }
                }
            }
            pending = event_receiver.try_recv().ok();
        }
        if stopping.load(Ordering::Acquire) {
            return;
        }
        match connection.has_pending_data() {
            Ok(false) => {}
            Ok(true) => match connection.receive::<UiCommand>() {
                Ok(command) => {
                    last_activity = Instant::now();
                    if !dispatch(
                        command,
                        controller,
                        &mut connection,
                        &event_sender,
                        main_thread,
                    ) {
                        return;
                    }
                }
                Err(_) => return,
            },
            Err(_) => return,
        }
    }
}

fn dispatch(
    command: UiCommand,
    controller: &Controller,
    connection: &mut PipeConnection,
    events: &EventQueue,
    main_thread: u32,
) -> bool {
    match command {
        UiCommand::RequestSnapshot => send_snapshot(controller, connection),
        UiCommand::LoadProfile(index) => {
            if let Err(error) = locked(controller).stop_monitor() {
                return send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("profile-load-failed", error)),
                );
            }
            if !send_reply(connection, &UiEvent::MonitorStateChanged(false)) {
                return false;
            }
            let result = locked(controller).load_profile(index);
            match result {
                Ok(snapshot) => {
                    let enabled = match locked(controller).filter_enabled() {
                        Ok(enabled) => enabled,
                        Err(error) => {
                            return send_reply(
                                connection,
                                &UiEvent::RuntimeError(error_view("profile-load-failed", error)),
                            );
                        }
                    };
                    let names = snapshot.draft.bindings.map(physical_key_name);
                    send_reply(
                        connection,
                        &UiEvent::ProfileLoaded(UiSnapshot::from_app(snapshot, names, enabled)),
                    )
                }
                Err(error) => send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("profile-load-failed", error)),
                ),
            }
        }
        UiCommand::RenameProfile { slot, name } => {
            let result = locked(controller).rename_profile(slot, name);
            match result {
                Ok(snapshot) => send_app_snapshot(controller, connection, snapshot, false),
                Err(error) => send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("profile-rename-failed", error)),
                ),
            }
        }
        UiCommand::SetFilterEnabled(enabled) => {
            let result = {
                let mut controller = locked(controller);
                controller
                    .set_filter_enabled(enabled)
                    .and_then(|()| controller.filter_enabled())
            };
            match result {
                Ok(confirmed) => {
                    // SAFETY: scalar thread-message payload, no borrowed pointers cross threads.
                    let posted = unsafe {
                        PostThreadMessageW(main_thread, FILTER_STATUS_MESSAGE, WPARAM(0), LPARAM(0))
                    };
                    if let Err(error) = posted {
                        return send_reply(
                            connection,
                            &UiEvent::RuntimeError(ErrorView {
                                code: "filter-failed".into(),
                                message: format!(
                                    "Filter changed, but the tray could not be notified: {error}"
                                ),
                                recoverable: true,
                            }),
                        );
                    }
                    send_reply(connection, &UiEvent::FilterChanged(confirmed))
                }
                Err(error) => send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("filter-failed", error)),
                ),
            }
        }
        UiCommand::StartMonitor => start_monitor(controller, connection, events),
        UiCommand::StopMonitor => match locked(controller).stop_monitor() {
            Ok(()) => send_reply(connection, &UiEvent::MonitorStateChanged(false)),
            Err(error) => send_reply(
                connection,
                &UiEvent::RuntimeError(error_view("monitor-stop-failed", error)),
            ),
        },
        UiCommand::CancelKeyCapture => {
            controller_snapshot_command(controller, connection, |controller| {
                controller.cancel_key_capture()?;
                Ok(controller.snapshot())
            })
        }
        UiCommand::ResetMeasurement => {
            let active = locked(controller).snapshot().measurement_active;
            if active {
                start_measurement(controller, connection, events)
            } else {
                controller_snapshot_command(controller, connection, |controller| {
                    controller.clear_measurement()
                })
            }
        }
        UiCommand::UpdateDraft(draft) => {
            let snapshot = {
                let mut controller = locked(controller);
                controller.replace_draft(draft);
                controller.snapshot()
            };
            send_app_snapshot(controller, connection, snapshot, false)
        }
        UiCommand::Apply => {
            if let Err(error) = locked(controller).stop_monitor() {
                return send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("apply-failed", error)),
                );
            }
            let result = locked(controller).apply();
            if !send_reply(connection, &UiEvent::MonitorStateChanged(false)) {
                return false;
            }
            match result {
                Ok(snapshot) => send_app_snapshot(controller, connection, snapshot, true),
                Err(error @ AppControllerError::InvalidSettings(_)) => send_reply(
                    connection,
                    &UiEvent::ValidationFailed(error_view("invalid-settings", error)),
                ),
                Err(error) => send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("apply-failed", error)),
                ),
            }
        }
        UiCommand::Revert => {
            controller_snapshot_command(controller, connection, |controller| controller.revert())
        }
        UiCommand::RestoreMappingDefaults => {
            controller_snapshot_command(controller, connection, |controller| {
                controller.restore_mapping_defaults()
            })
        }
        UiCommand::RestoreAllDefaults => {
            controller_snapshot_command(controller, connection, |controller| {
                controller.restore_all_defaults()
            })
        }
        UiCommand::BeginKeyCapture(slot) => begin_key_capture(slot, controller, connection, events),
        UiCommand::StartMeasurement => start_measurement(controller, connection, events),
        UiCommand::StopMeasurement => {
            controller_snapshot_command(controller, connection, |controller| {
                controller.stop_measurement()
            })
        }
        UiCommand::CloseUiSession => {
            let result = locked(controller).close_ui_session();
            if let Err(error) = result {
                let _ = send_reply(
                    connection,
                    &UiEvent::RuntimeError(error_view("close-session-failed", error)),
                );
            }
            false
        }
    }
}

fn begin_key_capture(
    slot: KeySlot,
    controller: &Controller,
    connection: &mut PipeConnection,
    events: &EventQueue,
) -> bool {
    let result = locked(controller).begin_key_capture(LogicalKey::from(slot));
    match result {
        Ok((generation, receiver)) => {
            let worker_events = events.clone();
            if thread::Builder::new()
                .name("lastkey-ipc-key-capture".into())
                .spawn(move || {
                    let Ok(captured) = receiver.recv() else {
                        return;
                    };
                    // Validation happens on the pump thread, where this
                    // completion is sequenced against later commands such as
                    // Revert instead of racing them.
                    let _ = worker_events.send(ServerEvent::KeyCaptureDone {
                        generation,
                        slot,
                        captured,
                    });
                })
                .is_err()
            {
                let _ = locked(controller).cancel_key_capture();
                return send_reply(
                    connection,
                    &UiEvent::RuntimeError(ErrorView {
                        code: "capture-worker-failed".into(),
                        message: "The key capture worker could not start.".into(),
                        recoverable: true,
                    }),
                );
            }
            send_snapshot(controller, connection)
        }
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("capture-failed", error)),
        ),
    }
}

/// Validates a capture completion against the current generation, applies it,
/// and answers from the pump thread, so a Revert processed either before or
/// after can neither be undone by it nor leave it unanswered. A stale
/// completion is dropped silently: the answering Snapshot already carries the
/// reverted state. Returns false when the session is over.
fn complete_key_capture(
    controller: &Controller,
    connection: &mut PipeConnection,
    generation: u64,
    slot: KeySlot,
    captured: CapturedKey,
) -> bool {
    let key = DisplayKey {
        physical: captured.physical,
        name: captured.name.clone(),
    };
    let accepted = locked(controller)
        .complete_key_capture(generation, captured)
        .is_some();
    if accepted {
        send_reply(connection, &UiEvent::KeyCaptured { slot, key })
    } else {
        true
    }
}

/// Validates a measurement update against the current generation, applies
/// it, and answers from the pump thread, so an update accepted for one
/// session can never surface in the next. Stale updates drop silently:
/// the Stop/Start snapshots already carry the authoritative state. Returns
/// false when the session is over.
fn accept_measurement_update(
    controller: &Controller,
    connection: &mut PipeConnection,
    generation: u64,
    update: MeasurementUpdate,
) -> bool {
    let accepted = locked(controller).update_measurement(generation, update);
    if accepted {
        send_reply(connection, &UiEvent::MeasurementUpdated(update.into()))
    } else {
        true
    }
}

fn start_measurement(
    controller: &Controller,
    connection: &mut PipeConnection,
    events: &EventQueue,
) -> bool {
    let result = locked(controller).start_measurement();
    match result {
        Ok((generation, receiver)) => {
            let worker_events = events.clone();
            if thread::Builder::new()
                .name("lastkey-ipc-measurement".into())
                .spawn(move || {
                    // Validation happens on the pump thread, where this update
                    // is sequenced against Stop and the next Start instead of
                    // racing them.
                    while let Ok(update) = receiver.recv() {
                        if worker_events
                            .send(ServerEvent::MeasurementUpdated {
                                generation,
                                update: Box::new(update),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .is_err()
            {
                let _ = locked(controller).stop_measurement();
                return send_reply(
                    connection,
                    &UiEvent::RuntimeError(ErrorView {
                        code: "measurement-worker-failed".into(),
                        message: "The measurement worker could not start.".into(),
                        recoverable: true,
                    }),
                );
            }
            send_snapshot(controller, connection)
        }
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("measurement-start-failed", error)),
        ),
    }
}

fn controller_snapshot_command(
    controller: &Controller,
    connection: &mut PipeConnection,
    command: impl FnOnce(
        &mut AppController<FileSettingsStore, InputService>,
    ) -> Result<crate::app::AppSnapshot, AppControllerError>,
) -> bool {
    let result = command(&mut locked(controller));
    match result {
        Ok(snapshot) => send_app_snapshot(controller, connection, snapshot, false),
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("runtime-command-failed", error)),
        ),
    }
}

fn send_snapshot(controller: &Controller, connection: &mut PipeConnection) -> bool {
    let snapshot = locked(controller).snapshot();
    send_app_snapshot(controller, connection, snapshot, false)
}

fn send_app_snapshot(
    controller: &Controller,
    connection: &mut PipeConnection,
    snapshot: crate::app::AppSnapshot,
    applied: bool,
) -> bool {
    let enabled = match locked(controller).filter_enabled() {
        Ok(enabled) => enabled,
        Err(error) => {
            return send_reply(
                connection,
                &UiEvent::RuntimeError(error_view("filter-state-failed", error)),
            );
        }
    };
    let names = snapshot.draft.bindings.map(physical_key_name);
    let snapshot = UiSnapshot::from_app(snapshot, names, enabled);
    send_reply(
        connection,
        &if applied {
            UiEvent::ApplySucceeded(snapshot)
        } else {
            UiEvent::Snapshot(snapshot)
        },
    )
}

fn start_monitor(
    controller: &Controller,
    connection: &mut PipeConnection,
    events: &EventQueue,
) -> bool {
    let result = locked(controller).start_monitor();
    match result {
        Ok((generation, receiver)) => {
            let events = events.clone();
            if thread::Builder::new()
                .name("lastkey-ipc-monitor".into())
                .spawn(move || {
                    while let Ok(event) = receiver.recv() {
                        if events
                            .send(ServerEvent::MonitorUpdated {
                                generation,
                                event: Box::new(event),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .is_err()
            {
                let _ = locked(controller).stop_monitor();
                return send_reply(
                    connection,
                    &UiEvent::RuntimeError(ErrorView {
                        code: "monitor-start-failed".into(),
                        message: "The monitor worker could not start.".into(),
                        recoverable: true,
                    }),
                );
            }
            send_reply(connection, &UiEvent::MonitorStateChanged(true))
        }
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("monitor-start-failed", error)),
        ),
    }
}

fn error_view(code: &str, error: AppControllerError) -> ErrorView {
    ErrorView {
        code: code.into(),
        message: error.to_string(),
        recoverable: true,
    }
}

fn send_reply(connection: &mut PipeConnection, event: &UiEvent) -> bool {
    connection.send(event).is_ok()
}
