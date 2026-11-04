//! Failure evidence only. At connect the client's own new socket is read on its own number, the
//! stream taken and given back, no copy of it made or closed; at a failure the number is read
//! through a borrowed copy, never taking ownership of a number that may no longer be the client's.

use std::fs::File;
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};

// EBADF has this value on the supported macOS and Linux hosts.
pub(super) fn bad_descriptor(error: &io::Error) -> bool {
    error.raw_os_error() == Some(9)
}

#[derive(Debug, PartialEq, Eq)]
enum Observation {
    Open {
        device: u64,
        inode: u64,
        socket: bool,
        local: Result<SocketAddr, String>,
        peer: Result<SocketAddr, String>,
    },
    Closed,
    Unknown(String),
}

fn inspect(fd: BorrowedFd<'_>) -> Observation {
    // Cloning borrows the original and gives File exactly one newly owned descriptor. Its
    // metadata is fstat on that duplicate, observing the original's file description at clone.
    match fd.try_clone_to_owned() {
        Ok(copy) => observed(File::from(copy)).0,
        Err(error) if bad_descriptor(&error) => Observation::Closed,
        Err(error) => Observation::Unknown(error.to_string()),
    }
}

/// What `file`'s descriptor holds, and the descriptor back as a stream.
fn observed(file: File) -> (Observation, TcpStream) {
    let metadata = file.metadata();
    // Endpoint reads must observe that same held description, not an original fd which another
    // component might replace between the stat and getsockname/getpeername calls.
    let socket = TcpStream::from(OwnedFd::from(file));
    let observation = match metadata {
        Ok(meta) => Observation::Open {
            device: meta.dev(),
            inode: meta.ino(),
            socket: meta.file_type().is_socket(),
            local: socket.local_addr().map_err(|error| error.to_string()),
            peer: socket.peer_addr().map_err(|error| error.to_string()),
        },
        Err(error) => Observation::Unknown(error.to_string()),
    };
    (observation, socket)
}

#[derive(Debug)]
pub(super) struct Descriptor {
    number: RawFd,
    connected: Observation,
}

impl Descriptor {
    /// `stream`, the client's own new socket, read on its own number and given back: a copy closed
    /// here is one more close that a process reading it could turn into a drain.
    pub(super) fn connected(stream: TcpStream) -> (Self, TcpStream) {
        let number = stream.as_raw_fd();
        let (connected, stream) = observed(File::from(OwnedFd::from(stream)));
        (Self { number, connected }, stream)
    }

    pub(super) fn failure(&self, stream: &TcpStream) -> String {
        self.describe(inspect(stream.as_fd()))
    }

    /// Whether `stream`'s number still holds the socket it held at connect, as read now: the same
    /// ends, said as such; else the evidence of what it holds.
    pub(super) fn recheck(&self, stream: &TcpStream) -> Result<String, String> {
        self.judge(inspect(stream.as_fd()))
    }

    /// The same socket: open, a socket, the same stat, and both ends read and unchanged. A peer
    /// that no longer reads as one, or any end that cannot be read, is not proof of it.
    fn judge(&self, current: Observation) -> Result<String, String> {
        match (&self.connected, &current) {
            (
                Observation::Open {
                    device,
                    inode,
                    socket: true,
                    local: Ok(local),
                    peer: Ok(peer),
                },
                Observation::Open {
                    device: now_device,
                    inode: now_inode,
                    socket: true,
                    local: Ok(now_local),
                    peer: Ok(now_peer),
                },
            ) if (device, inode, local, peer) == (now_device, now_inode, now_local, now_peer) => {
                Ok(format!(
                    "descriptor fd={} still holds its socket, both ends as at connect: local {local}, peer {peer}",
                    self.number
                ))
            }
            _ => Err(self.describe(current)),
        }
    }

    fn describe(&self, current: Observation) -> String {
        let state = match (&self.connected, &current) {
            (_, Observation::Closed) => "closed-by-someone",
            (_, Observation::Open { socket: false, .. }) => "replaced",
            (
                Observation::Open {
                    device: before_dev,
                    inode: before_ino,
                    local: before_local,
                    peer: before_peer,
                    ..
                },
                Observation::Open {
                    device: now_dev,
                    inode: now_ino,
                    local: now_local,
                    peer: now_peer,
                    ..
                },
            ) if (before_dev, before_ino) != (now_dev, now_ino)
                || differs(before_local, now_local)
                || differs(before_peer, now_peer) =>
            {
                "replaced"
            }
            (
                Observation::Open {
                    local: before_local,
                    peer: before_peer,
                    ..
                },
                Observation::Open {
                    local: now_local,
                    peer: now_peer,
                    ..
                },
            ) if [before_local, before_peer, now_local, now_peer]
                .iter()
                .any(|address| address.is_err()) =>
            {
                "identity-unknown"
            }
            (Observation::Open { inode: 1.., .. }, Observation::Open { .. })
                if self.connected == current =>
            {
                "still-open-same-identity"
            }
            // macOS TCP fstat reports device=inode=0 for distinct sockets. Matching endpoint
            // tuples do not exclude a close/reconnect, so never turn those zeros into identity.
            (Observation::Open { .. }, Observation::Open { .. }) => "open-identity-unproven",
            _ => "identity-unknown",
        };
        format!(
            "descriptor fd={} state={state}, at-connect={:?}, at-failure={current:?} (point-in-time observations)",
            self.number, self.connected
        )
    }
}

fn differs(before: &Result<SocketAddr, String>, now: &Result<SocketAddr, String>) -> bool {
    matches!((before, now), (Ok(before), Ok(now)) if before != now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        connect(&listener)
    }

    fn connect(listener: &std::net::TcpListener) -> (TcpStream, TcpStream) {
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        (socket, peer)
    }

    #[test]
    fn evidence_borrows_the_socket_and_compares_each_identity_outcome() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (socket, mut peer) = connect(&listener);
        let number = socket.as_raw_fd();
        let (evidence, mut socket) = Descriptor::connected(socket);
        assert_eq!((evidence.number, socket.as_raw_fd()), (number, number));
        assert!(matches!(evidence.connected, Observation::Open { .. }));
        let same = evidence.failure(&socket);
        assert!(
            same.contains(&format!("fd={} ", socket.as_raw_fd())),
            "{same}"
        );
        #[cfg(target_os = "macos")]
        assert!(same.contains("state=open-identity-unproven"), "{same}");
        #[cfg(not(target_os = "macos"))]
        assert!(same.contains("state=still-open-same-identity"), "{same}");
        assert!(same.contains("at-connect=Open"), "{same}");
        assert!(same.contains("at-failure=Open"), "{same}");
        let (other, _other_peer) = connect(&listener);
        assert_ne!(
            (socket.local_addr().unwrap(), socket.peer_addr().unwrap()),
            (other.local_addr().unwrap(), other.peer_addr().unwrap())
        );
        assert_eq!(
            evidence.recheck(&socket),
            Ok(format!(
                "descriptor fd={number} still holds its socket, both ends as at connect: local {}, peer {}",
                socket.local_addr().unwrap(),
                socket.peer_addr().unwrap()
            ))
        );
        let another = evidence.recheck(&other).unwrap_err();
        assert!(another.contains("state=replaced"), "{another}");
        let replaced = evidence.failure(&other);
        assert!(replaced.contains("state=replaced"), "{replaced}");
        assert!(replaced.contains("local: Ok"), "{replaced}");
        assert!(replaced.contains("peer: Ok"), "{replaced}");
        let file = File::open("/dev/null").unwrap();
        let non_socket = inspect(file.as_fd());
        assert!(matches!(
            non_socket,
            Observation::Open { socket: false, .. }
        ));
        let replaced = evidence.describe(non_socket);
        assert!(replaced.contains("state=replaced"), "{replaced}");
        assert!(replaced.contains("local: Err"), "{replaced}");
        assert!(replaced.contains("peer: Err"), "{replaced}");
        assert!(
            replaced.contains(&std::io::Error::from_raw_os_error(libc::ENOTSOCK).to_string()),
            "{replaced}"
        );
        let closed = evidence.describe(Observation::Closed);
        assert!(closed.contains("state=closed-by-someone"), "{closed}");
        assert!(closed.contains("at-failure=Closed"), "{closed}");
        let unknown = evidence.describe(Observation::Unknown("cannot inspect".into()));
        assert!(unknown.contains("state=identity-unknown"), "{unknown}");
        assert!(unknown.contains("cannot inspect"), "{unknown}");
        drop(evidence);
        peer.write_all(b"a").unwrap();
        let mut byte = [0];
        socket.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"a");
    }

    #[test]
    fn file_type_proves_replacement_even_when_stat_is_uninformative() {
        let (socket, _peer) = pair();
        let (mut evidence, socket) = Descriptor::connected(socket);
        evidence.connected = Observation::Open {
            device: 0,
            inode: 0,
            socket: true,
            local: Ok(socket.local_addr().unwrap()),
            peer: Ok(socket.peer_addr().unwrap()),
        };
        let text = evidence.describe(Observation::Open {
            device: 0,
            inode: 0,
            socket: false,
            local: Err("not a socket".into()),
            peer: Err("not a socket".into()),
        });
        assert!(text.contains("state=replaced"), "{text}");
        assert!(text.contains("socket: false"), "{text}");
        assert!(text.contains("not a socket"), "{text}");
    }

    #[test]
    fn a_listeners_changed_local_address_proves_replacement_despite_its_peer_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (socket, _peer) = connect(&listener);
        let (evidence, socket) = Descriptor::connected(socket);
        assert_ne!(socket.local_addr().unwrap(), listener.local_addr().unwrap());
        let observed = inspect(listener.as_fd());
        assert!(matches!(
            &observed,
            Observation::Open {
                local: Ok(_),
                peer: Err(_),
                ..
            }
        ));
        let text = evidence.judge(observed).unwrap_err();
        assert!(text.contains("state=replaced"), "{text}");
        assert!(
            text.contains(&io::Error::from_raw_os_error(libc::ENOTCONN).to_string()),
            "{text}"
        );

        let mut unavailable = inspect(socket.as_fd());
        let Observation::Open { peer, .. } = &mut unavailable else {
            panic!("socket must be open");
        };
        *peer = Err("peer lookup unavailable".into());
        let text = evidence
            .judge(unavailable)
            .expect_err("an unread end proves nothing");
        assert!(text.contains("state=identity-unknown"), "{text}");
        assert!(text.contains("peer lookup unavailable"), "{text}");
    }
}
