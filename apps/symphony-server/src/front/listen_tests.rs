use super::{bind_all, listen, listen_all};
use std::cell::Cell;
use std::io::{self, ErrorKind};
use std::rc::Rc;

fn hosts() -> [String; 2] {
    ["127.0.0.1".into(), "::1".into()]
}

/// A bound resource whose ownership is visible even if another process acquires the same port.
#[derive(Debug)]
struct Observed {
    port: u16,
    live: Rc<Cell<usize>>,
}

impl Observed {
    fn new(port: u16, live: &Rc<Cell<usize>>) -> Self {
        live.set(live.get() + 1);
        Self {
            port,
            live: live.clone(),
        }
    }
}

impl Drop for Observed {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}

#[tokio::test]
async fn a_complete_set_accepts_connections_on_every_address() {
    let (port, listeners) = listen_all(&hosts(), 0, listen).unwrap();
    assert_ne!(port, 0);
    assert_eq!(listeners.len(), 2);
    for listener in &listeners {
        let address = listener.local_addr().unwrap();
        assert_eq!(address.port(), port);
        tokio::net::TcpStream::connect(address).await.unwrap();
    }
}

#[tokio::test]
async fn an_invalid_address_keeps_the_native_error_and_stops_after_one_attempt() {
    let mut calls = 0;
    let error = listen_all(&["127.0.0.1".into(), "invalid".into()], 0, |host, port| {
        calls += 1;
        listen(host, port)
    })
    .unwrap_err();
    assert_eq!(calls, 2);
    assert!(error.to_string().contains("invalid is not an address"));
}

#[test]
fn a_picked_port_taken_on_a_sibling_address_is_released_and_retried() {
    let live = Rc::new(Cell::new(0));
    let mut picks = 0;
    let mut calls = Vec::new();
    let (port, listeners) = bind_all(
        &hosts(),
        0,
        |host, port| {
            calls.push((host.to_owned(), port));
            if port == 0 {
                assert_eq!(
                    live.get(),
                    0,
                    "abandoned attempts are closed before retrying"
                );
                picks += 1;
                Ok(Observed::new(41000 + picks, &live))
            } else if picks == 1 {
                Err(io::Error::from(ErrorKind::AddrInUse))
            } else {
                Ok(Observed::new(port, &live))
            }
        },
        |listener| Ok(listener.port),
    )
    .unwrap();
    assert_eq!(picks, 2);
    assert_eq!(port, 41002);
    assert_eq!(
        calls,
        [
            ("127.0.0.1".into(), 0),
            ("::1".into(), 41001),
            ("127.0.0.1".into(), 0),
            ("::1".into(), 41002),
        ]
    );
    assert_eq!(listeners.len(), 2);
    assert_eq!(
        live.get(),
        2,
        "the successful set remains owned by the caller"
    );
    drop(listeners);
    assert_eq!(live.get(), 0);
}

#[test]
fn repeated_sibling_collisions_exhaust_a_bounded_number_of_attempts() {
    let live = Rc::new(Cell::new(0));
    let mut picks = 0;
    let mut calls = 0;
    // Two successful resources before the failure prove every partial listener is dropped.
    let hosts = ["127.0.0.1".into(), "127.0.0.2".into(), "::1".into()];
    let error = bind_all(
        &hosts,
        0,
        |host, port| {
            calls += 1;
            if port == 0 {
                assert_eq!(live.get(), 0, "previous attempt still owns listeners");
                picks += 1;
            }
            if host == "::1" {
                Err(io::Error::from(ErrorKind::AddrInUse))
            } else {
                Ok(Observed::new(41000, &live))
            }
        },
        |listener| Ok(listener.port),
    )
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
    assert_eq!(picks, 16);
    assert_eq!(calls, 48);
    assert!(error.to_string().contains("::1 port 41000"));
    assert!(error.to_string().contains("exhausted 16 attempts"));
    assert_eq!(
        live.get(),
        0,
        "the final failed attempt released both listeners"
    );
}

#[test]
fn explicit_ports_and_non_collision_errors_are_not_retried() {
    for (requested, kind) in [
        (41000, ErrorKind::AddrInUse),
        (0, ErrorKind::PermissionDenied),
    ] {
        let live = Rc::new(Cell::new(0));
        let mut calls = Vec::new();
        let hosts = ["127.0.0.1".into(), "127.0.0.2".into(), "::1".into()];
        let error = bind_all(
            &hosts,
            requested,
            |host, port| {
                calls.push((host.to_owned(), port));
                if host == "::1" {
                    Err(io::Error::from(kind))
                } else {
                    Ok(Observed::new(41000, &live))
                }
            },
            |listener| Ok(listener.port),
        )
        .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(
            calls,
            [
                ("127.0.0.1".into(), requested),
                ("127.0.0.2".into(), 41000),
                ("::1".into(), 41000),
            ]
        );
        assert!(
            error
                .to_string()
                .contains("could not listen on ::1 port 41000")
        );
        assert!(!error.to_string().contains("exhausted"));
        assert_eq!(live.get(), 0, "a terminal error released both listeners");
    }
}

#[test]
fn ipv6_is_optional_only_for_non_collision_errors_on_every_interface() {
    for kind in [ErrorKind::AddrNotAvailable, ErrorKind::AddrInUse] {
        let live = Rc::new(Cell::new(0));
        let mut picks = 0;
        let mut calls = 0;
        let result = bind_all(
            &["0.0.0.0".into(), "::".into()],
            0,
            |host, port| {
                calls += 1;
                if port == 0 {
                    assert_eq!(live.get(), 0, "a retry retained its preceding listeners");
                    picks += 1;
                }
                if host.contains(':') {
                    Err(io::Error::from(kind))
                } else {
                    Ok(Observed::new(41000, &live))
                }
            },
            |listener| Ok(listener.port),
        );
        if kind == ErrorKind::AddrInUse {
            assert_eq!(result.unwrap_err().kind(), kind);
            assert_eq!(picks, 16);
            assert_eq!(calls, 32);
            assert_eq!(live.get(), 0);
        } else {
            let (port, listeners) = result.unwrap();
            // Outside Linux the set also binds the two loopback addresses before the wildcards.
            let ipv4_addresses = if cfg!(target_os = "linux") { 1 } else { 2 };
            assert_eq!(port, 41000);
            assert_eq!(picks, 1);
            assert_eq!(calls, ipv4_addresses * 2);
            assert_eq!(listeners.len(), ipv4_addresses);
            assert_eq!(live.get(), ipv4_addresses);
            drop(listeners);
            assert_eq!(live.get(), 0);
        }
    }
}

#[test]
fn a_first_address_failure_is_not_retried() {
    let mut calls = 0;
    let error = bind_all(
        &hosts(),
        0,
        |_, _| {
            calls += 1;
            Err::<Observed, _>(io::Error::from(ErrorKind::AddrInUse))
        },
        |listener| Ok(listener.port),
    )
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
    assert_eq!(calls, 1);
    assert!(!error.to_string().contains("exhausted"));
}

#[test]
fn a_port_lookup_error_releases_the_partial_set_without_retrying() {
    let live = Rc::new(Cell::new(0));
    let mut calls = 0;
    let error = bind_all(
        &hosts(),
        0,
        |_, _| {
            calls += 1;
            Ok(Observed::new(41000, &live))
        },
        |listener| {
            if live.get() == 2 {
                Err(io::Error::from(ErrorKind::AddrInUse))
            } else {
                Ok(listener.port)
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
    assert_eq!(calls, 2);
    assert_eq!(live.get(), 0);
}
