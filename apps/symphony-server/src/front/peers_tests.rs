//! The gate a peer passes and the Host `/api/front` holds to, as a project's routes and the one
//! server's both judge them ([`peer_allowed`], [`loopback_refused`]), and what a project's state
//! is made with ([`Shared::one_server`], [`Shared::beside`]).

use super::*;

/// A confirmed tailnet peer, the one address the gate's `tailscale whois` knows.
const PEER: &str = "100.101.102.103";

/// A project's state behind a gate whose `tailscale whois` answers `whois`.
fn behind(
    whois: impl Fn(&str) -> Result<Option<String>, String> + Send + Sync + 'static,
) -> Arc<Shared> {
    let gate = Gate::new(Arc::new(whois), Arc::new(Instant::now));
    Shared::new(1, 1, Path::new("/nowhere"), None, gate)
}

/// A response's status and its body as text.
async fn answered(response: Response) -> (StatusCode, String) {
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// This machine is let in without a `tailscale whois`: loopback at once, and an IPv4-mapped
/// loopback by the gate, which unmaps it.
#[tokio::test]
async fn this_machine_is_let_in_without_asking_tailscale() {
    let shared = behind(|ip| panic!("tailscale was asked of {ip}"));
    for ip in ["127.0.0.1", "127.9.9.9", "::1", "::ffff:127.0.0.1"] {
        assert!(peer_allowed(&shared, ip.parse().unwrap()).await, "{ip}");
    }
}

/// A tailnet address is let in when `tailscale whois` names a node of the tailnet for it, and
/// refused when it says the address is no peer; an address outside the tailnet's ranges is refused
/// unasked.
#[tokio::test]
async fn a_tailnet_peer_is_let_in_only_when_tailscale_confirms_it() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let shared = behind({
        let asked = asked.clone();
        move |ip| {
            asked.lock().unwrap().push(ip.to_string());
            Ok((ip == PEER).then(|| r#"{"Node": {"Name": "phone"}}"#.to_string()))
        }
    });
    assert!(peer_allowed(&shared, PEER.parse().unwrap()).await);
    for ip in [
        "100.64.0.9",
        "fd7a:115c:a1e0::9",
        "192.168.1.20",
        "10.0.0.2",
        "::ffff:192.168.1.20",
        "0.0.0.0",
    ] {
        assert!(!peer_allowed(&shared, ip.parse().unwrap()).await, "{ip}");
    }
    assert_eq!(
        *asked.lock().unwrap(),
        [PEER, "100.64.0.9", "fd7a:115c:a1e0::9"],
        "tailscale is asked of the tailnet's addresses alone"
    );
}

/// A gate that panics while it judges a peer refuses it, and the server answers on.
#[tokio::test]
async fn a_peer_whose_gate_panics_is_refused() {
    let shared = behind(|ip| panic!("the gate's whois of {ip} panics"));
    assert!(!peer_allowed(&shared, PEER.parse().unwrap()).await);
    assert!(peer_allowed(&shared, "127.0.0.1".parse().unwrap()).await);
}

#[tokio::test]
async fn a_refused_peer_is_told_who_the_symphony_answers() {
    assert_eq!(
        answered(peer_refused()).await,
        (
            StatusCode::FORBIDDEN,
            "Forbidden: the Symphony answers only this machine and its tailnet\n".to_string()
        )
    );
}

/// `/api/front` answers a Host naming loopback, and refuses another name, none, and one that is no
/// text, each with the same answer.
#[tokio::test]
async fn the_front_route_refuses_every_host_but_loopbacks_name() {
    let with = |host: Option<&[u8]>| {
        let mut headers = HeaderMap::new();
        if let Some(host) = host {
            headers.insert(
                header::HOST,
                axum::http::HeaderValue::from_bytes(host).unwrap(),
            );
        }
        headers
    };
    for host in ["127.0.0.1:1", "localhost", "LOCALHOST:8765", "[::1]:1"] {
        assert!(
            loopback_refused(&with(Some(host.as_bytes()))).is_none(),
            "{host}"
        );
    }
    let refused = [
        Some(&b"evil.example:1"[..]),
        Some(&b"127.0.0.1.evil.example"[..]),
        Some(&b""[..]),
        Some(&b"localhost\xff"[..]),
        None,
    ];
    for host in refused {
        let response = loopback_refused(&with(host)).unwrap_or_else(|| panic!("{host:?}"));
        assert_eq!(
            answered(response).await,
            (
                StatusCode::FORBIDDEN,
                "Forbidden: the Host header does not name this machine's loopback\n".to_string()
            ),
            "{host:?}"
        );
    }
}

/// The server given its install folder is the machine's one server, and so is each project's
/// state beside it; a server of its own is not, nor is a project beside it.
#[test]
fn the_one_server_is_the_one_given_its_install_folder() {
    let root = Path::new("/nowhere");
    let one = Shared::new(
        1,
        1,
        root,
        Some(Path::new("/install")),
        crate::testing::open_gate(),
    );
    let private = Shared::new(1, 1, root, None, crate::testing::open_gate());
    assert!(one.one_server());
    assert!(!private.one_server());
    assert!(one.beside(Path::new("/other"), "/other").one_server());
    assert!(!private.beside(Path::new("/other"), "/other").one_server());
}

/// A project beside another is served by the same process and port with the same client, and
/// its recorders keep the same gate: its root, base and hub are its own.
#[test]
fn a_project_beside_another_shares_its_server_and_client_and_keeps_its_own_root() {
    let first = Shared::new(
        7,
        8,
        Path::new("/first"),
        Some(Path::new("/install")),
        crate::testing::open_gate(),
    );
    let second = first.beside(Path::new("/second"), "/second");
    assert_eq!((second.pid, second.port), (7, 8));
    assert_eq!(
        (&second.ui_dist, second.installed_ui, second.embedded_ui),
        (&first.ui_dist, first.installed_ui, first.embedded_ui)
    );
    assert!(Arc::ptr_eq(&second.recording, &first.recording));
    assert_eq!(
        (second.root.as_path(), second.base.as_str()),
        (Path::new("/second"), "/second")
    );
    assert_eq!(
        (first.root.as_path(), first.base.as_str()),
        (Path::new("/first"), "")
    );
    assert!(
        second.api.get().is_none(),
        "its hub is its own, started later"
    );
}
