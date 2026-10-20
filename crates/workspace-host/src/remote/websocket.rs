//! A paired device's connection on `/v1`: the challenge, then the host protocol, one JSON
//! message per text frame, until either side closes it.
use super::{Identity, Shared, ToActor, listener::ServerSocket};
use crate::socket::{Outbox, Peer, Session};
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    sync::Arc,
    time::Instant,
};
use tungstenite::{
    Message,
    protocol::{CloseFrame, frame::coding::CloseCode},
};
use workspace_core::{Challenge, ChallengeAnswer, ChallengeVerdict, UNPAIRED, pairing};

/// How long the I/O thread sleeps with nothing to do before it looks again.
const IDLE_POLL_MS: i32 = 15_000;

pub(super) fn send(socket: &mut ServerSocket, value: &impl Serialize) -> Result<()> {
    socket.send(Message::text(serde_json::to_string(value)?))?;
    Ok(())
}

/// The next text message, as `T`.
pub(super) fn receive<T: DeserializeOwned>(socket: &mut ServerSocket) -> Result<T> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Close(_) => bail!("The device closed the connection"),
            _ => {}
        }
    }
}

pub(super) fn serve(
    mut socket: ServerSocket,
    connection: u64,
    identity: &Identity,
    shared: &Arc<Shared>,
) -> Result<()> {
    let challenge = workspace_core::new_credential()?.into_bytes();
    let host_id = identity.key.id();
    send(
        &mut socket,
        &Challenge {
            challenge: BASE64.encode(&challenge),
            host_id: host_id.clone(),
        },
    )?;
    let answer: ChallengeAnswer = receive(&mut socket)?;
    // Claimed before the actor checks it, so a removal meanwhile still closes this connection.
    shared.claim(connection, &answer.device_id);
    let level = pairing::challenge_message(&challenge, &host_id, &answer.device_id)
        .ok()
        .zip(BASE64.decode(&answer.signature).ok())
        .and_then(|(signed, signature)| {
            let (reply, verdict) = async_channel::bounded(1);
            shared
                .requests
                .tell(ToActor::Authenticate {
                    device: answer.device_id.clone(),
                    signed,
                    signature,
                    reply,
                })
                .ok()?;
            verdict.recv_blocking().ok().flatten()
        });
    let Some(level) = level else {
        send(&mut socket, &ChallengeVerdict::Unpaired)?;
        let _ = socket.close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: UNPAIRED.into(),
        }));
        let _ = socket.flush();
        return Ok(());
    };
    send(&mut socket, &ChallengeVerdict::Accepted { level })?;
    let served = pump(socket, answer.device_id, shared);
    let _ = shared.requests.tell(ToActor::Changed);
    served
}

/// Messages for the device: queued for the I/O thread, which a byte on a pipe wakes.
struct Frames {
    queue: async_channel::Sender<Vec<u8>>,
    wake: File,
}
impl Outbox for Frames {
    fn send(&self, message: Vec<u8>) -> Result<()> {
        self.queue
            .send_blocking(message)
            .map_err(|_| anyhow::anyhow!("The device's connection closed"))?;
        let _ = (&self.wake).write(&[1]);
        Ok(())
    }
}

/// Reads requests and writes replies and events on one thread, with the socket non-blocking,
/// until the device or the host closes the connection.
fn pump(mut socket: ServerSocket, device: String, shared: &Arc<Shared>) -> Result<()> {
    let (wake_read, wake_write) = pipe()?;
    let (queue, queued) = async_channel::bounded(64);
    let outbox = Arc::new(Frames {
        queue,
        wake: wake_write,
    });
    let mut session = Session::new(
        shared.requests.clone(),
        Peer::Device { id: device },
        shared.binary.clone(),
        outbox,
        shared.replying.clone(),
    );
    socket.get_mut().sock.set_nonblocking(true)?;
    let served = (|| -> Result<()> {
        let mut waiting: Option<Message> = None;
        loop {
            loop {
                match socket.read() {
                    Ok(Message::Text(text)) => session.receive(text.as_bytes(), Instant::now())?,
                    Ok(Message::Close(_)) => return Ok(()),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(
                        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed,
                    ) => return Ok(()),
                    // A device that goes away without closing TLS, or one the host shut down.
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                        ) =>
                    {
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            while let Some(message) = waiting.take().or_else(|| queued.try_recv().ok().map(text)) {
                match socket.write(message) {
                    Ok(()) => {}
                    Err(tungstenite::Error::WriteBufferFull(message)) => {
                        waiting = Some(*message);
                        break;
                    }
                    Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let blocked = match socket.flush() {
                Ok(()) => false,
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => true,
                Err(error) => return Err(error.into()),
            };
            if waiting.is_some() && !blocked {
                continue;
            }
            wait(socket.get_ref().sock.as_raw_fd(), blocked, &wake_read);
        }
    })();
    queued.close();
    served
}

fn text(message: Vec<u8>) -> Message {
    Message::text(String::from_utf8_lossy(&message).into_owned())
}

/// Waits until the socket can be read, or written when `blocked`, or the pipe wakes it.
fn wait(socket: i32, blocked: bool, wake: &File) {
    let mut events = libc::POLLIN;
    if blocked {
        events |= libc::POLLOUT;
    }
    let mut fds = [
        libc::pollfd {
            fd: socket,
            events,
            revents: 0,
        },
        libc::pollfd {
            fd: wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: two valid `pollfd`s.
    unsafe { libc::poll(fds.as_mut_ptr(), 2, IDLE_POLL_MS) };
    let mut drained = [0u8; 64];
    while matches!((&*wake).read(&mut drained), Ok(n) if n > 0) {}
}

/// A pipe whose read end does not block.
fn pipe() -> Result<(File, File)> {
    let mut ends = [0; 2];
    // SAFETY: `ends` has room for both descriptors.
    anyhow::ensure!(
        unsafe { libc::pipe(ends.as_mut_ptr()) } == 0,
        std::io::Error::last_os_error()
    );
    for end in ends {
        // SAFETY: descriptors pipe just opened.
        unsafe {
            libc::fcntl(end, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(end, libc::F_SETFL, libc::O_NONBLOCK);
        }
    }
    // SAFETY: each descriptor is owned by exactly one `File`.
    Ok(unsafe { (File::from_raw_fd(ends[0]), File::from_raw_fd(ends[1])) })
}
