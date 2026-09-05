use std::{
    io,
    os::windows::io::AsRawHandle,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    app::{AppController, AppControllerError, FileSettingsStore},
    core::LogicalKey,
    protocol::{ErrorView, KeySlot, UiCommand, UiEvent, UiSnapshot},
};
use windows::Win32::{Foundation::HANDLE, System::IO::CancelSynchronousIo};

use super::{
    InputService,
    ipc::{IPC_POLL_INTERVAL, NamedPipeServer, PipeConnection, SETTINGS_PIPE_NAME},
    physical_key_name,
};

type Controller = Arc<Mutex<AppController<FileSettingsStore, InputService>>>;
type EventQueue = Sender<UiEvent>;
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
            .is_some_and(|queue| queue.send(event).is_ok())
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
        let _ = controller
            .lock()
            .expect("app controller mutex is not poisoned")
            .close_ui_session();
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
    // threads: drain outbound, Peek-gated inbound read, sleep.
    let (event_sender, event_receiver) = mpsc::channel::<UiEvent>();
    *client
        .lock()
        .expect("settings IPC client mutex is not poisoned") = Some(event_sender.clone());

    loop {
        while let Ok(event) = event_receiver.try_recv() {
            if !send_reply(&mut connection, &event) {
                return;
            }
        }
        if stopping.load(Ordering::Acquire) {
            return;
        }
        match connection.has_pending_data() {
            Ok(false) => thread::sleep(IPC_POLL_INTERVAL),
            Ok(true) => match connection.receive::<UiCommand>() {
                Ok(command) => {
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
                let mut controller = controller
                    .lock()
                    .expect("app controller mutex is not poisoned");
                controller.replace_draft(draft);
                controller.snapshot()
            };
            send_reply(connection, &UiEvent::Snapshot(ui_snapshot(snapshot)))
        }
        UiCommand::Apply => {
            let result = controller
                .lock()
                .expect("app controller mutex is not poisoned")
                .apply();
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
            let result = controller
                .lock()
                .expect("app controller mutex is not poisoned")
                .close_ui_session();
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
    let result = controller
        .lock()
        .expect("app controller mutex is not poisoned")
        .begin_key_capture(LogicalKey::from(slot));
    match result {
        Ok((generation, receiver)) => {
            let worker_controller = Arc::clone(controller);
            let worker_events = events.clone();
            if thread::Builder::new()
                .name("lastkey-ipc-key-capture".into())
                .spawn(move || {
                    let Ok(captured) = receiver.recv() else {
                        return;
                    };
                    let key = crate::protocol::DisplayKey {
                        physical: captured.physical,
                        name: captured.name.clone(),
                    };
                    let accepted = worker_controller
                        .lock()
                        .expect("app controller mutex is not poisoned")
                        .complete_key_capture(generation, captured)
                        .is_some();
                    if accepted {
                        let _ = worker_events.send(UiEvent::KeyCaptured { slot, key });
                    }
                })
                .is_err()
            {
                let _ = controller
                    .lock()
                    .expect("app controller mutex is not poisoned")
                    .cancel_key_capture();
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

fn start_measurement(
    controller: &Controller,
    connection: &mut PipeConnection,
    events: &EventQueue,
) -> bool {
    let result = controller
        .lock()
        .expect("app controller mutex is not poisoned")
        .start_measurement();
    match result {
        Ok((generation, receiver)) => {
            let worker_controller = Arc::clone(controller);
            let worker_events = events.clone();
            if thread::Builder::new()
                .name("lastkey-ipc-measurement".into())
                .spawn(move || {
                    while let Ok(update) = receiver.recv() {
                        let accepted = worker_controller
                            .lock()
                            .expect("app controller mutex is not poisoned")
                            .update_measurement(generation, update);
                        if !accepted {
                            continue;
                        }
                        // The stop snapshot can win the race after acceptance;
                        // re-check before sending so stale events do not revive
                        // a stopped measurement session in the UI.
                        let still_current = worker_controller
                            .lock()
                            .expect("app controller mutex is not poisoned")
                            .is_current_measurement(generation);
                        if still_current
                            && worker_events
                                .send(UiEvent::MeasurementUpdated(update.into()))
                                .is_err()
                        {
                            break;
                        }
                    }
                })
                .is_err()
            {
                let _ = controller
                    .lock()
                    .expect("app controller mutex is not poisoned")
                    .stop_measurement();
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
    let result = command(
        &mut controller
            .lock()
            .expect("app controller mutex is not poisoned"),
    );
    match result {
        Ok(snapshot) => send_reply(connection, &UiEvent::Snapshot(ui_snapshot(snapshot))),
        Err(error) => send_reply(
            connection,
            &UiEvent::RuntimeError(error_view("runtime-command-failed", error)),
        ),
    }
}

fn send_snapshot(controller: &Controller, connection: &mut PipeConnection) -> bool {
    let snapshot = controller
        .lock()
        .expect("app controller mutex is not poisoned")
        .snapshot();
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
