use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle},
    thread,
    time::{Duration, Instant},
};

use serde::{Serialize, de::DeserializeOwned};
use windows::{
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_SEM_TIMEOUT,
            GetLastError, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
        },
    },
    core::HSTRING,
};

use crate::protocol::{MAX_FRAME_SIZE, ProtocolError, decode, encode};

pub const SETTINGS_PIPE_NAME: &str = r"\\.\pipe\LastKey.Settings.v1";
const PIPE_SECURITY_SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

/// Active poll interval for the single-threaded IPC pump loops.
///
/// A pending blocking read on one handle stalls writes on a duplicate
/// handle of the same synchronous pipe instance, so each session pumps its
/// pipe from exactly one thread: wait on the outbound queue with a timeout,
/// drain outbound, Peek-gated inbound read. A few milliseconds of settings-UI
/// latency is irrelevant here; latency-sensitive input never crosses this
/// pipe.
pub const IPC_POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Idle poll interval once a session has been quiet for [`IPC_IDLE_AFTER`].
/// Fifty milliseconds is imperceptible in a settings dialog and cuts idle
/// wakeups by more than tenfold.
pub const IPC_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Silence after which a session pump backs off to [`IPC_IDLE_POLL_INTERVAL`].
pub const IPC_IDLE_AFTER: Duration = Duration::from_millis(250);

/// Returns the pump wait for the time since the last sent or received
/// message. Any traffic resets the session to the active interval, so the
/// step is invisible to the user.
pub fn ipc_poll_interval(idle_for: Duration) -> Duration {
    if idle_for >= IPC_IDLE_AFTER {
        IPC_IDLE_POLL_INTERVAL
    } else {
        IPC_POLL_INTERVAL
    }
}

pub struct NamedPipeServer {
    name: HSTRING,
    pending: Option<File>,
}

impl NamedPipeServer {
    pub fn new(name: &str) -> io::Result<Self> {
        validate_pipe_name(name)?;
        let name = HSTRING::from(name);
        let pending = Some(create_pipe_instance(&name)?);
        Ok(Self { name, pending })
    }

    pub fn accept(&mut self) -> io::Result<PipeConnection> {
        let file = match self.pending.take() {
            Some(file) => file,
            None => create_pipe_instance(&self.name)?,
        };
        let handle = windows::Win32::Foundation::HANDLE(file.as_raw_handle());
        let connected = unsafe { ConnectNamedPipe(handle, None) }.is_ok()
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            return Err(io::Error::last_os_error());
        }
        Ok(PipeConnection { file })
    }
}

fn create_pipe_instance(name: &HSTRING) -> io::Result<File> {
    let security_descriptor = LocalSecurityDescriptor::new(PIPE_SECURITY_SDDL)?;
    let security_attributes = security_descriptor.attributes();
    let handle = unsafe {
        CreateNamedPipeW(
            name,
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            Some(&security_attributes),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

pub struct PipeConnection {
    file: File,
}

impl PipeConnection {
    pub fn connect(name: &str, timeout: Duration) -> io::Result<Self> {
        validate_pipe_name(name)?;
        let started = Instant::now();
        loop {
            match OpenOptions::new().read(true).write(true).open(name) {
                Ok(file) => return Ok(Self { file }),
                Err(error) if started.elapsed() < timeout && is_retryable_connect_error(&error) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn send<T: Serialize>(&mut self, message: &T) -> io::Result<()> {
        let frame = encode(message).map_err(protocol_io_error)?;
        self.file.write_all(&frame)?;
        self.file.flush()
    }

    /// Returns true when at least one byte is available without blocking.
    ///
    /// This gates `receive` so a pump thread never sits in a blocking read
    /// while it still has outbound messages to write. Any error (including a
    /// broken pipe) means the session is over.
    pub fn has_pending_data(&self) -> io::Result<bool> {
        let handle = windows::Win32::Foundation::HANDLE(self.file.as_raw_handle());
        let mut available = 0_u32;
        unsafe { PeekNamedPipe(handle, None, 0, None, Some(&mut available), None) }
            .map_err(|_| io::Error::last_os_error())?;
        Ok(available > 0)
    }

    pub fn receive<T: DeserializeOwned>(&mut self) -> io::Result<T> {
        let mut length_bytes = [0_u8; 4];
        self.file.read_exact(&mut length_bytes)?;
        let payload_length = u32::from_le_bytes(length_bytes) as usize;
        if payload_length > MAX_FRAME_SIZE {
            return Err(protocol_io_error(ProtocolError::FrameTooLarge(
                payload_length,
            )));
        }
        let mut frame = Vec::with_capacity(4 + payload_length);
        frame.extend_from_slice(&length_bytes);
        frame.resize(4 + payload_length, 0);
        self.file.read_exact(&mut frame[4..])?;
        decode(&frame).map_err(protocol_io_error)
    }
}

struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl LocalSecurityDescriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error.to_string()))?;
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

fn is_retryable_connect_error(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    [ERROR_FILE_NOT_FOUND, ERROR_SEM_TIMEOUT, ERROR_PIPE_BUSY]
        .into_iter()
        .any(|retryable| code == retryable.0 as i32)
}

fn validate_pipe_name(name: &str) -> io::Result<()> {
    if name.starts_with(r"\\.\pipe\") && name.len() > r"\\.\pipe\".len() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "named pipe path must start with \\\\.\\pipe\\",
        ))
    }
}

fn protocol_io_error(error: ProtocolError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use std::{process, sync::mpsc, thread, time::Duration};

    use crate::protocol::UiCommand;

    use super::{
        IPC_IDLE_POLL_INTERVAL, IPC_POLL_INTERVAL, NamedPipeServer, PipeConnection,
        ipc_poll_interval,
    };

    #[test]
    fn named_pipe_transports_protocol_messages_in_both_directions() {
        let name = format!(
            r"\\.\pipe\LastKey.Test.{}.{}",
            process::id(),
            thread::current().name().unwrap_or("unnamed")
        );
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let server_thread = thread::spawn(move || {
            let mut connection = server.accept().expect("client connects");
            let command: UiCommand = connection.receive().expect("command arrives");
            assert_eq!(command, UiCommand::RequestSnapshot);
            connection.send(&UiCommand::Apply).expect("reply is sent");
        });

        let mut client = PipeConnection::connect(&name, Duration::from_secs(2))
            .expect("client connects to server");
        client
            .send(&UiCommand::RequestSnapshot)
            .expect("command is sent");
        assert_eq!(
            client.receive::<UiCommand>().expect("reply arrives"),
            UiCommand::Apply
        );
        server_thread.join().expect("server thread finishes");
    }

    #[test]
    fn pump_interval_backs_off_only_after_sustained_silence() {
        assert_eq!(ipc_poll_interval(Duration::ZERO), IPC_POLL_INTERVAL);
        assert_eq!(
            ipc_poll_interval(Duration::from_millis(249)),
            IPC_POLL_INTERVAL
        );
        assert_eq!(
            ipc_poll_interval(Duration::from_millis(250)),
            IPC_IDLE_POLL_INTERVAL
        );
        assert_eq!(
            ipc_poll_interval(Duration::from_secs(60)),
            IPC_IDLE_POLL_INTERVAL
        );
    }

    #[test]
    fn peek_reports_pending_frames_without_consuming_them() {
        let name = format!(
            r"\\.\pipe\LastKey.Test.Peek.{}.{}",
            process::id(),
            thread::current().name().unwrap_or("unnamed")
        );
        let (replied_sender, replied_receiver) = mpsc::channel();
        let (done_sender, done_receiver) = mpsc::channel();
        let server_name = name.clone();
        let server_thread = thread::spawn(move || {
            let mut server = NamedPipeServer::new(&server_name).expect("server is configured");
            let mut connection = server.accept().expect("client connects");
            let command: UiCommand = connection.receive().expect("command arrives");
            assert_eq!(command, UiCommand::RequestSnapshot);
            connection.send(&UiCommand::Apply).expect("reply is sent");
            replied_sender.send(()).expect("test harness is listening");
            done_receiver.recv().expect("test harness finishes first");
        });

        let mut client = PipeConnection::connect(&name, Duration::from_secs(2))
            .expect("client connects to server");
        assert!(
            !client
                .has_pending_data()
                .expect("idle pipe reports no data")
        );
        client
            .send(&UiCommand::RequestSnapshot)
            .expect("command is sent");
        replied_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("server replies");
        assert!(client.has_pending_data().expect("reply is visible"));
        assert_eq!(
            client.receive::<UiCommand>().expect("reply arrives"),
            UiCommand::Apply
        );
        assert!(
            !client
                .has_pending_data()
                .expect("consumed pipe reports no data")
        );
        done_sender.send(()).expect("server is waiting");
        server_thread.join().expect("server thread finishes");
    }
}
