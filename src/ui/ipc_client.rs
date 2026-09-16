//! The settings window's IPC client: one reader thread owns the pipe, outbound
//! commands go back through a channel, and inbound events reach the app through
//! a channel plus a wake callback.
//!
//! Port of `iced-ui/ipc_client.rs`. The Iced `Sipper` stream is replaced by
//! `std::sync::mpsc` plus `wake`: eframe has no async runtime, so the reader
//! calls the wake callback (the app passes `Context::request_repaint`) after
//! every queued event, and the app drains the receiver on the next frame.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    platform::windows::ipc::{
        IPC_SLEEP_POLL_INTERVAL, PipeConnection, SETTINGS_PIPE_NAME, ipc_poll_interval,
    },
    protocol::{UiCommand, UiEvent},
};

#[derive(Clone, Debug)]
pub enum Event {
    Connected(Connection),
    Message(Box<UiEvent>),
    Disconnected(String),
}

#[derive(Clone, Debug)]
pub struct Connection {
    commands: std_mpsc::Sender<UiCommand>,
    awake: Arc<AtomicBool>,
}

impl Connection {
    pub fn send(&self, command: UiCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|error| error.to_string())
    }

    /// Lets the pump sleep while the window is deactivated. The command queue
    /// still wakes it immediately, so a command sent on the way out is not
    /// delayed by the longer wait.
    pub fn set_awake(&self, awake: bool) {
        self.awake.store(awake, Ordering::Relaxed);
    }
}

/// Starts the reader thread and returns the event receiver. `wake` is called
/// after every queued event so the window repaints without polling; the app
/// passes a `Context::request_repaint` closure.
pub fn connect(wake: impl Fn() + Send + Sync + 'static) -> std_mpsc::Receiver<Event> {
    let (sender, receiver) = std_mpsc::channel();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let spawn = thread::Builder::new()
        .name("lastkey-settings-ipc-reader".into())
        .spawn({
            let sender = sender.clone();
            let wake = Arc::clone(&wake);
            move || run_connection_loop(sender, wake)
        });
    if let Err(error) = spawn {
        let _ = sender.send(Event::Disconnected(error.to_string()));
        wake();
    }
    receiver
}

fn run_connection_loop(event_sender: std_mpsc::Sender<Event>, wake: Arc<dyn Fn() + Send + Sync>) {
    // The sole owner of the pipe handle. A pending blocking read on one
    // handle stalls writes on a duplicate handle of the same synchronous
    // pipe, so reads and writes for one session share this thread: wait on
    // the outbound queue, drain outbound, Peek-gated inbound read.
    loop {
        let mut pipe = match PipeConnection::connect(SETTINGS_PIPE_NAME, Duration::from_millis(500))
        {
            Ok(connection) => connection,
            Err(error) => {
                if !publish(
                    &event_sender,
                    &*wake,
                    Event::Disconnected(error.to_string()),
                ) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let (command_sender, command_receiver) = std_mpsc::channel();
        let awake = Arc::new(AtomicBool::new(true));
        if !publish(
            &event_sender,
            &*wake,
            Event::Connected(Connection {
                commands: command_sender,
                awake: Arc::clone(&awake),
            }),
        ) {
            return;
        }
        let mut last_activity = Instant::now();
        'session: loop {
            // Wait on the outbound queue with a timeout so locally queued
            // commands leave immediately; only inbound discovery is bounded
            // by the poll interval.
            let interval = if awake.load(Ordering::Relaxed) {
                ipc_poll_interval(last_activity.elapsed())
            } else {
                IPC_SLEEP_POLL_INTERVAL
            };
            let mut pending = match command_receiver.recv_timeout(interval) {
                Ok(command) => Some(command),
                Err(std_mpsc::RecvTimeoutError::Timeout) => None,
                // The app dropped its Connection handle. Keep polling
                // inbound, mirroring the old try_recv behavior.
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(interval);
                    None
                }
            };
            while let Some(command) = pending {
                last_activity = Instant::now();
                let closing = command == UiCommand::CloseUiSession;
                if pipe.send(&command).is_err() {
                    if !publish(
                        &event_sender,
                        &*wake,
                        Event::Disconnected("failed to send a command to the runtime".into()),
                    ) {
                        return;
                    }
                    break 'session;
                }
                if closing {
                    return;
                }
                pending = command_receiver.try_recv().ok();
            }
            match pipe.has_pending_data() {
                Ok(false) => {}
                Ok(true) => match pipe.receive::<UiEvent>() {
                    Ok(event) => {
                        last_activity = Instant::now();
                        if !publish(&event_sender, &*wake, Event::Message(Box::new(event))) {
                            return;
                        }
                    }
                    Err(error) => {
                        if !publish(
                            &event_sender,
                            &*wake,
                            Event::Disconnected(error.to_string()),
                        ) {
                            return;
                        }
                        break 'session;
                    }
                },
                Err(error) => {
                    if !publish(
                        &event_sender,
                        &*wake,
                        Event::Disconnected(error.to_string()),
                    ) {
                        return;
                    }
                    break 'session;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

/// Queues one event and wakes the window. False once the app dropped the
/// receiver, which tells the reader to stop.
fn publish(
    sender: &std_mpsc::Sender<Event>,
    wake: &(dyn Fn() + Send + Sync),
    event: Event,
) -> bool {
    if sender.send(event).is_ok() {
        wake();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command handle delivers to the channel the reader owns, and the
    /// awake flag is shared with it.
    #[test]
    fn connection_delivers_commands_and_the_awake_flag() {
        let (tx, rx) = std_mpsc::channel();
        let awake = Arc::new(AtomicBool::new(true));
        let connection = Connection {
            commands: tx,
            awake: Arc::clone(&awake),
        };

        connection.send(UiCommand::RequestSnapshot).unwrap();
        assert_eq!(rx.try_recv().unwrap(), UiCommand::RequestSnapshot);

        connection.set_awake(false);
        assert!(!awake.load(Ordering::Relaxed));
        connection.set_awake(true);
        assert!(awake.load(Ordering::Relaxed));
    }

    /// A dead reader surfaces as a send error instead of a silent drop,
    /// which is what the app turns into a disconnect.
    #[test]
    fn send_reports_a_dropped_reader() {
        let (tx, rx) = std_mpsc::channel();
        let connection = Connection {
            commands: tx,
            awake: Arc::new(AtomicBool::new(true)),
        };
        drop(rx);
        assert!(connection.send(UiCommand::RequestSnapshot).is_err());
    }

    /// Every published event wakes the window exactly once.
    #[test]
    fn publish_wakes_after_a_queued_event() {
        let (tx, rx) = std_mpsc::channel();
        let wakes = Arc::new(AtomicBool::new(false));
        let wake = {
            let wakes = Arc::clone(&wakes);
            move || wakes.store(true, Ordering::Relaxed)
        };
        assert!(publish(&tx, &wake, Event::Disconnected("boom".into())));
        assert!(wakes.load(Ordering::Relaxed));
        assert!(matches!(rx.try_recv(), Ok(Event::Disconnected(_))));

        drop(rx);
        assert!(!publish(&tx, &wake, Event::Disconnected("boom".into())));
    }
}
