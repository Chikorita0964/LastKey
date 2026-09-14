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

/// Pump wait for a settings window that has lost focus. It draws no frames
/// then, so it only needs to drain the pipe often enough that the runtime's
/// writes never block — the filter engine runs in that other process and must
/// not be slowed by a deactivated dialog.
pub const IPC_SLEEP_POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    use std::{
        fs::OpenOptions,
        io::{self, Write},
        process,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use crate::protocol::{MAX_FRAME_SIZE, UiCommand, encode};

    use super::{
        IPC_IDLE_POLL_INTERVAL, IPC_POLL_INTERVAL, NamedPipeServer, PipeConnection,
        ipc_poll_interval,
    };

    /// Pipe names are machine-global, so a test instance is keyed by the
    /// process and the test's own thread to survive parallel runs and a real
    /// LastKey running on the same machine.
    fn test_pipe_name(label: &str) -> String {
        format!(
            r"\\.\pipe\LastKey.Test.{}.{}.{}",
            label,
            process::id(),
            thread::current().name().unwrap_or("unnamed")
        )
    }

    /// Frames a payload by hand so a test can put bytes on the pipe that
    /// `encode` would never produce.
    fn raw_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::from(
            u32::try_from(payload.len())
                .expect("test payload length fits in u32")
                .to_le_bytes(),
        );
        frame.extend_from_slice(payload);
        frame
    }

    /// Opens the pipe the way a settings process would, but as a plain file so
    /// the test can write bytes `PipeConnection::send` would reject.
    fn raw_client(name: &str) -> std::fs::File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
            .expect("client opens the pipe")
    }

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

    /// `receive` consumes exactly the framed length before it tries to decode,
    /// so one bad payload costs one message rather than the whole session. A
    /// reader that decoded first and resynchronized afterwards would leave the
    /// stream off a frame boundary and every later frame would be garbage.
    #[test]
    fn a_malformed_payload_does_not_desynchronize_the_stream() {
        let name = test_pipe_name("Malformed");
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let server_thread = thread::spawn(move || {
            let mut connection = server.accept().expect("client connects");
            let error = connection
                .receive::<UiCommand>()
                .expect_err("a malformed payload is rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(
                connection
                    .receive::<UiCommand>()
                    .expect("the following frame still decodes"),
                UiCommand::Apply
            );
        });

        let mut client = raw_client(&name);
        client
            .write_all(&raw_frame(b"{ not json"))
            .expect("garbage is sent");
        client
            .write_all(&encode(&UiCommand::Apply).expect("command encodes"))
            .expect("a valid frame follows");
        client.flush().expect("client flushes");
        server_thread.join().expect("server thread finishes");
    }

    /// The length prefix is checked before the body is read, so a hostile or
    /// corrupt prefix cannot make the pump reserve memory or wait for bytes
    /// that will never arrive. This test hangs rather than fails if that order
    /// is ever reversed, which is the failure worth noticing.
    #[test]
    fn an_oversized_length_prefix_is_rejected_without_reading_a_body() {
        let name = test_pipe_name("Oversized");
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let server_thread = thread::spawn(move || {
            let mut connection = server.accept().expect("client connects");
            let error = connection
                .receive::<UiCommand>()
                .expect_err("an oversized frame is rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("too large"), "{error}");
        });

        let mut client = raw_client(&name);
        let length = u32::try_from(MAX_FRAME_SIZE + 1)
            .expect("test maximum fits in u32")
            .to_le_bytes();
        client.write_all(&length).expect("prefix is sent");
        client.flush().expect("client flushes");
        server_thread.join().expect("server thread finishes");
    }

    /// A settings process killed mid-write leaves a partial frame. The read has
    /// to end in an error the pump can treat as end of session; blocking here
    /// would strand the accept loop and leak the session until the runtime exits.
    #[test]
    fn a_client_that_dies_mid_frame_ends_the_read_instead_of_blocking() {
        let name = test_pipe_name("Truncated");
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let (accepted, is_accepted) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            let mut connection = server.accept().expect("client connects");
            // The session has to exist before the client dies, or the failure
            // under test never happens: ConnectNamedPipe rejects a pipe whose
            // client closed first, which is a different path entirely.
            accepted.send(()).expect("test harness is listening");
            let error = connection
                .receive::<UiCommand>()
                .expect_err("a half-written frame cannot decode");
            // Whether the closed handle surfaces as an EOF or a broken pipe
            // depends on how much of the body was already buffered; the pump
            // ends the session either way.
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
                ),
                "unexpected error kind {:?}: {error}",
                error.kind()
            );
        });

        {
            let mut client = raw_client(&name);
            is_accepted
                .recv_timeout(Duration::from_secs(5))
                .expect("the server accepts the session");
            let frame = encode(&UiCommand::Apply).expect("command encodes");
            client
                .write_all(&frame[..frame.len() - 1])
                .expect("a partial frame is sent");
            client.flush().expect("client flushes");
        }
        server_thread.join().expect("server thread finishes");
    }

    /// The pump never blocks in a read it has not peeked first, so the peek is
    /// the only place a dead client can be noticed. It has to report the closed
    /// handle as an error rather than as "no data", which would idle forever.
    #[test]
    fn a_disconnected_client_is_reported_by_the_peek_the_pump_gates_on() {
        let name = test_pipe_name("Disconnect");
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let (accepted, is_accepted) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            let connection = server.accept().expect("client connects");
            // The client must outlive the accept: a pipe whose client closed
            // first fails ConnectNamedPipe outright, which is a different path.
            accepted.send(()).expect("test harness is listening");
            let deadline = Instant::now() + Duration::from_secs(5);
            while connection.has_pending_data().is_ok() {
                assert!(
                    Instant::now() < deadline,
                    "the peek reported an idle pipe instead of the closed client"
                );
                thread::sleep(Duration::from_millis(10));
            }
        });

        let client = raw_client(&name);
        is_accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("the server accepts the session");
        drop(client);
        server_thread.join().expect("server thread finishes");
    }

    /// `UiServer::drop` raises its stop flag and then connects to its own pipe,
    /// because a thread parked in `ConnectNamedPipe` never observes the flag.
    /// This covers that release path; the `CancelSynchronousIo` call beside it
    /// needs the real server thread handle and stays outside this test.
    #[test]
    fn a_parked_accept_is_released_by_a_self_connect() {
        let name = test_pipe_name("AcceptRelease");
        let mut server = NamedPipeServer::new(&name).expect("server is configured");
        let (parked, is_parked) = mpsc::channel();
        let accept_thread = thread::spawn(move || {
            parked.send(()).expect("test harness is listening");
            server
                .accept()
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        is_parked
            .recv_timeout(Duration::from_secs(5))
            .expect("the accept thread starts");

        let _self_connect = PipeConnection::connect(&name, Duration::from_millis(250))
            .expect("the shutdown self-connect reaches the pipe");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !accept_thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            accept_thread.is_finished(),
            "accept stayed parked after the self-connect"
        );
        accept_thread
            .join()
            .expect("accept thread finishes")
            .expect("accept returns the self-connected session");
    }
}
