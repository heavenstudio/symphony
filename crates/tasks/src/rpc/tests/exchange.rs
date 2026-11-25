//! A request on a connection of its own ([`crate::rpc::exchange`]): its response read to the end of
//! the connection, each wait the one it was given, and a socket macOS drained no reason to fail.

use super::{PATIENT, READ_PATIENCE, one_reply, read_request};
use crate::rpc::{exchange, exchanged, parse_response};
use std::io::Read;
use std::time::Duration;

/// A request with a body, as `tasks field` sends one.
const REQUEST: &[u8] =
    b"POST /api/task-field HTTP/1.1\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody";

#[test]
fn an_exchange_sends_its_request_and_reads_the_response_to_the_end_of_its_connection() {
    let (addr, served) = one_reply(b"HTTP/1.1 201 Created\r\nconnection: close\r\n\r\nmade");
    assert_eq!(
        exchange(&addr, REQUEST, PATIENT),
        Ok((201, b"made".to_vec()))
    );
    assert_eq!(served.join().unwrap(), REQUEST);
}

/// A server that takes the request and never answers: the read waits the exchange's wait and no
/// longer, and says so as a socket's own timeout did, `EAGAIN`.
#[test]
fn an_exchange_a_server_never_answers_meets_its_wait_as_eagain() {
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let held = std::thread::spawn(move || {
        let (mut taken, _) = silent.accept().unwrap();
        let request = read_request(&mut taken);
        // Held open, unanswered, until the client lets go of it.
        taken.set_read_timeout(Some(READ_PATIENCE)).unwrap();
        while matches!(taken.read(&mut [0; 4096]), Ok(read) if read > 0) {}
        request
    });
    let said = exchange(&addr, REQUEST, Duration::from_millis(50));
    assert_eq!(
        said,
        Err(format!(
            "the server at {addr} did not answer: {}",
            std::io::Error::from_raw_os_error(libc::EAGAIN)
        ))
    );
    assert_eq!(
        held.join().unwrap(),
        REQUEST,
        "the request went once, whole"
    );
}

/// A TCP socket macOS has drained, as an `lsof` can drain one at its connect's reopening close
/// ([`crate::sys::connect`]): a blocking read of it fails with EBADF, the control, and the
/// exchange on it still gets the server's answer. The server answers only once the exchange waits
/// for it (or has given up), so its read waits on the drained socket.
#[cfg(target_vendor = "apple")]
#[test]
fn an_exchange_on_a_socket_macos_drained_gets_the_servers_answer() {
    use crate::socket::drain;
    use std::io::Write;
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    let (kept, mut peer) = drain::drained::<TcpStream>();
    let mut control = kept.try_clone().unwrap();
    control
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let blocked = control.read(&mut [0; 1]).unwrap_err();
    assert_eq!(
        blocked.raw_os_error(),
        Some(libc::EBADF),
        "drained: {blocked}"
    );
    drop(control);

    let id = Arc::new(AtomicU64::new(0));
    let said = Arc::clone(&id);
    let client = std::thread::spawn(move || {
        said.store(drain::thread_id(), Ordering::SeqCst);
        exchanged(kept, REQUEST, PATIENT)
    });
    peer.set_read_timeout(Some(READ_PATIENCE)).unwrap();
    let request = read_request(&mut peer);
    let backstop = test_time::backstop(Duration::from_secs(10));
    test_time::until_true("the exchange waits for its answer", backstop, || {
        let id = id.load(Ordering::SeqCst);
        client.is_finished() || (id != 0 && drain::waiting_in_the_kernel(id))
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    // A client that already failed has closed its end: what this write meets is no matter.
    let _ = peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    drop(peer);
    let raw = client
        .join()
        .unwrap()
        .expect("the exchange on a drained socket");
    assert_eq!(parse_response(&raw), Ok((200, b"ok".to_vec())));
    assert_eq!(request, REQUEST);
}
