//! Native access boundary: admit only confirmed peers. Private whois unit
//! tests cover raw stdout and errors without HTTP or machine Tailscale state.

#[path = "after_access/fixture.rs"]
mod fixture;

#[path = "common/bash_env.rs"]
mod bash_env;

use fixture::{Fixture, child_case};
use symphony_server::gate::Gate;

#[test]
fn bash_env_does_not_corrupt_access_lookup() {
    bash_env::run(
        "bash_env_does_not_corrupt_access_lookup",
        native_access_uses_command_stdout_and_refuses_unconfirmed_peers,
    );
}

#[test]
fn native_access_uses_command_stdout_and_refuses_unconfirmed_peers() {
    const ADDRESS: &str = "100.84.246.59";
    const PEER: &[u8] = b" \n{\"Node\": {\"Name\": \"phone.ts.net.\"}}\n\n";
    if let Some(case) = child_case() {
        let expected = match case.as_str() {
            "confirmed" => true,
            "refused" => false,
            other => panic!("unknown access fixture: {other}"),
        };
        // The native caller must unwrap mapped IPv4 before passing the address to whois. The
        // stand-in answers within a backstop only a stall reaches, not the server's 3 s.
        let address = format!("::ffff:{ADDRESS}").parse().unwrap();
        let gate = Gate::tailscale_within(test_time::backstop(std::time::Duration::from_secs(3)));
        assert_eq!(gate.allows(address), expected);
        return;
    }

    for (case, stdout, stderr, status) in [
        ("confirmed", PEER, b"peer not found\n".as_slice(), 0),
        ("refused", b"not json {\n".as_slice(), PEER, 0),
        ("refused", b"".as_slice(), PEER, 0),
        ("refused", b"[]\n".as_slice(), b"".as_slice(), 0),
        ("refused", b"{\"Self\": {}}\n".as_slice(), b"".as_slice(), 0),
        (
            "refused",
            b"{\"Node\": null}\n".as_slice(),
            b"".as_slice(),
            0,
        ),
        ("refused", b"{\"Node\": 5}\n".as_slice(), b"".as_slice(), 0),
        ("refused", PEER, b"tailscaled is down\n".as_slice(), 1),
        ("refused", PEER, b"peer not found\n".as_slice(), 1),
        ("refused", PEER, b"peer unreachable\n".as_slice(), 1),
    ] {
        let fixture = Fixture::new(stdout, stderr, status);
        fixture.run(
            "native_access_uses_command_stdout_and_refuses_unconfirmed_peers",
            case,
        );
        fixture.assert_args(&["whois", "--json", ADDRESS]);
    }
}
