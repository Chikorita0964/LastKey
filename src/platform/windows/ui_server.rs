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
    core::LogicalKey,
    protocol::{DisplayKey, ErrorView, KeySlot, UiCommand, UiEvent, UiSnapshot},
};
use windows::Win32::{Foundation::HANDLE, System::IO::CancelSynchronousIo};

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
        let stopping = Arc::new(AtomicBool::new(false));
        let stopping_for_thread = Arc::clone(&stopping);
        let client = Arc::new(Mutex::new(None));
        let client_for_thread = Arc::clone(&client);
        let thread = thread::Builder::new()
            .name("lastkey-ui-server".into())
            .spawn(move || run(server, controller, stopping_for_thread, client_for_thread))?;
        Ok(Self {
            stopping,
            client,
            thread: Some(thread),
        })
    }

    pub fn request_focus(&self, view: crate::protocol::UiView) -> bool {
        self.send_event(UiEvent::FocusRequested(view))
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
        serve_connection(connection, &controller, &client, &stopping);
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
                    if !dispatch(command, controller, &mut connection, &event_sender) {
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
) -> bool {
    match command {
        UiCommand::RequestSnapshot => send_snapshot(controller, connection),
        UiCommand::UpdateDraft(draft) => {
            let snapshot = {
                let mut controller = locked(controller);
                controller.replace_draft(draft);
                controller.snapshot()
            };
            send_reply(connection, &UiEvent::Snapshot(ui_snapshot(snapshot)))
        }
        UiCommand::Apply => {
            let result = locked(controller).apply();
            match result {
                Ok(snapshot) => {
                    send_reply(connection, &UiEvent::ApplySucceeded(ui_snapshot(snapshot)))
                }
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
        Ok(snapshot) => send_reply(connection, &UiEvent::Snapshot(ui_snapshot(snapshot))),
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("runtime-command-failed", error)),
        ),
    }
}

fn send_snapshot(controller: &Controller, connection: &mut PipeConnection) -> bool {
    let snapshot = locked(controller).snapshot();
    send_reply(connection, &UiEvent::Snapshot(ui_snapshot(snapshot)))
}

fn ui_snapshot(snapshot: crate::app::AppSnapshot) -> UiSnapshot {
    let names = snapshot.draft.bindings.map(physical_key_name);
    UiSnapshot::from_app(snapshot, names)
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
