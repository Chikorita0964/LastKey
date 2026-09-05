use std::{sync::mpsc as std_mpsc, thread, time::Duration};

use iced::{
    futures::{channel::mpsc, stream::StreamExt},
    task::{Never, Sipper, sipper},
};

use crate::{
    platform::windows::ipc::{IPC_POLL_INTERVAL, PipeConnection, SETTINGS_PIPE_NAME},
    protocol::{UiCommand, UiEvent},
};

#[derive(Clone, Debug)]
pub enum Event {
    Connected(Connection),
    Message(Box<UiEvent>),
    Disconnected(String),
}

#[derive(Clone, Debug)]
pub struct Connection(std_mpsc::Sender<UiCommand>);

impl Connection {
    pub fn send(&self, command: UiCommand) -> Result<(), String> {
        self.0.send(command).map_err(|error| error.to_string())
    }
}

pub fn connect() -> impl Sipper<Never, Event> {
    sipper(async |mut output| {
        let (event_sender, mut events) = mpsc::unbounded();
        if let Err(error) = thread::Builder::new()
            .name("lastkey-settings-ipc-reader".into())
            .spawn(move || run_connection_loop(event_sender))
        {
            output.send(Event::Disconnected(error.to_string())).await;
        }

        while let Some(event) = events.next().await {
            output.send(event).await;
        }
        std::future::pending::<Never>().await
    })
}

fn run_connection_loop(event_sender: mpsc::UnboundedSender<Event>) {
    // The sole owner of the pipe handle. A pending blocking read on one
    // handle stalls writes on a duplicate handle of the same synchronous
    // pipe, so reads and writes for one session share this thread: drain
    // outbound, Peek-gated inbound read, sleep (see IPC_POLL_INTERVAL).
    loop {
        let mut pipe = match PipeConnection::connect(SETTINGS_PIPE_NAME, Duration::from_millis(500))
        {
            Ok(connection) => connection,
            Err(error) => {
                if !publish(&event_sender, Event::Disconnected(error.to_string())) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let (command_sender, command_receiver) = std_mpsc::channel();
        if !publish(&event_sender, Event::Connected(Connection(command_sender))) {
            return;
        }
        'session: loop {
            while let Ok(command) = command_receiver.try_recv() {
                let closing = command == UiCommand::CloseUiSession;
                if pipe.send(&command).is_err() {
                    if !publish(
                        &event_sender,
                        Event::Disconnected("failed to send a command to the runtime".into()),
                    ) {
                        return;
                    }
                    break 'session;
                }
                if closing {
                    return;
                }
            }
            match pipe.has_pending_data() {
                Ok(false) => thread::sleep(IPC_POLL_INTERVAL),
                Ok(true) => match pipe.receive::<UiEvent>() {
                    Ok(event) => {
                        if !publish(&event_sender, Event::Message(Box::new(event))) {
                            return;
                        }
                    }
                    Err(error) => {
                        if !publish(&event_sender, Event::Disconnected(error.to_string())) {
                            return;
                        }
                        break 'session;
                    }
                },
                Err(error) => {
                    if !publish(&event_sender, Event::Disconnected(error.to_string())) {
                        return;
                    }
                    break 'session;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn publish(sender: &mpsc::UnboundedSender<Event>, event: Event) -> bool {
    sender.unbounded_send(event).is_ok()
}
