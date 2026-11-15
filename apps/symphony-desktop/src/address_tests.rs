use super::*;

#[test]
fn plain_http_to_loopback_the_tailnet_and_its_names_is_a_server_and_its_origin_alone_is_kept() {
    for (pasted, origin) in [
        ("http://127.0.0.1:8765", "http://127.0.0.1:8765"),
        ("http://127.9.9.9:1/", "http://127.9.9.9:1"),
        (
            "  http://localhost:8765/symphony/tasks?x=1#top \n",
            "http://localhost:8765",
        ),
        ("http://[::1]:8765/", "http://[::1]:8765"),
        ("http://100.64.0.1:8765", "http://100.64.0.1:8765"),
        ("http://100.127.255.254:8765", "http://100.127.255.254:8765"),
        (
            "http://[fd7a:115c:a1e0::1]:8765",
            "http://[fd7a:115c:a1e0::1]:8765",
        ),
        (
            "http://[fd7a:115c:a1e0:ffff::9]:8765",
            "http://[fd7a:115c:a1e0:ffff::9]:8765",
        ),
        (
            "http://Studio.Tail1234.TS.NET:8765/symphony/",
            "http://studio.tail1234.ts.net:8765",
        ),
        ("http://studio.ts.net:8765", "http://studio.ts.net:8765"),
        (
            "http://studio.tail1234.ts.net.:8765",
            "http://studio.tail1234.ts.net.:8765",
        ),
        (
            "http://studio.tail1234.ts.net",
            "http://studio.tail1234.ts.net",
        ),
    ] {
        let server = Server::parse(pasted).unwrap_or_else(|why| panic!("{pasted}: {why}"));
        assert_eq!(server.to_string(), origin, "{pasted}");
        assert_eq!(server.page("/"), format!("{origin}/"), "{pasted}");
    }
}

#[test]
fn any_other_address_is_refused_saying_why() {
    for (pasted, refused) in [
        ("https://127.0.0.1:8765", Refused::Scheme("https".into())),
        (
            "https://studio.tail1234.ts.net",
            Refused::Scheme("https".into()),
        ),
        ("file:///etc/passwd", Refused::Scheme("file".into())),
        ("ftp://127.0.0.1/", Refused::Scheme("ftp".into())),
        ("javascript:alert(1)", Refused::Scheme("javascript".into())),
        ("http://owner:secret@127.0.0.1:8765", Refused::Credentials),
        ("http://owner@studio.ts.net", Refused::Credentials),
        (
            "http://192.168.1.20:8765",
            Refused::Host("192.168.1.20".into()),
        ),
        ("http://10.0.0.1:8765", Refused::Host("10.0.0.1".into())),
        // Just outside 100.64.0.0/10, on each side.
        (
            "http://100.63.255.255:8765",
            Refused::Host("100.63.255.255".into()),
        ),
        (
            "http://100.128.0.0:8765",
            Refused::Host("100.128.0.0".into()),
        ),
        // Just outside fd7a:115c:a1e0::/48.
        (
            "http://[fd7a:115c:a1e1::1]:8765",
            Refused::Host("[fd7a:115c:a1e1::1]".into()),
        ),
        ("http://[fe80::1]:8765", Refused::Host("[fe80::1]".into())),
        (
            "http://example.com:8765",
            Refused::Host("example.com".into()),
        ),
        ("http://ts.net:8765", Refused::Host("ts.net".into())),
        ("http://.ts.net:8765", Refused::Host(".ts.net".into())),
        (
            "http://studio.ts.net.example.com",
            Refused::Host("studio.ts.net.example.com".into()),
        ),
        ("http://studiots.net", Refused::Host("studiots.net".into())),
        (
            "http://localhost.example.com",
            Refused::Host("localhost.example.com".into()),
        ),
    ] {
        assert_eq!(Server::parse(pasted), Err(refused), "{pasted}");
    }
    for unreadable in [
        "",
        "studio.ts.net:8765",
        "127.0.0.1:8765",
        "http://",
        "not a url",
    ] {
        match Server::parse(unreadable) {
            Err(Refused::Unreadable(_) | Refused::Scheme(_) | Refused::Host(_)) => {}
            other => panic!("{unreadable:?} read as {other:?}"),
        }
    }
}

#[test]
fn a_refusal_says_what_is_followed_instead() {
    let host = Server::parse("http://example.com").unwrap_err().to_string();
    assert!(
        host.starts_with("example.com is neither this Mac nor on the tailnet")
            && host.contains("100.64.0.0/10")
            && host.contains("*.ts.net"),
        "{host}"
    );
    let scheme = Server::parse("https://127.0.0.1").unwrap_err().to_string();
    assert!(scheme.contains("not https://"), "{scheme}");
    let unreadable = Server::parse("studio").unwrap_err().to_string();
    assert!(
        unreadable.ends_with("write it as http://HOST:PORT"),
        "{unreadable}"
    );
}

#[test]
fn this_mac_is_named_so_a_tailnet_name_by_its_first_label_and_an_address_by_itself() {
    for (pasted, name, loopback) in [
        ("http://127.0.0.1:8765", "This Mac", true),
        ("http://[::1]:8765", "This Mac", true),
        ("http://localhost:8765", "This Mac", true),
        ("http://studio.tail1234.ts.net:8765", "studio", false),
        ("http://100.101.102.103:8765", "100.101.102.103", false),
        (
            "http://[fd7a:115c:a1e0::5]:8765",
            "[fd7a:115c:a1e0::5]",
            false,
        ),
    ] {
        let server = Server::parse(pasted).unwrap();
        assert_eq!(server.default_name(), name, "{pasted}");
        assert_eq!(server.is_loopback(), loopback, "{pasted}");
    }
}

#[test]
fn a_server_connects_to_its_host_and_port_and_names_its_pages_under_its_origin() {
    let named = Server::parse("http://studio.tail1234.ts.net/x").unwrap();
    assert_eq!(named.connect_to(), ("studio.tail1234.ts.net".into(), 80));
    assert_eq!(named.host(), "studio.tail1234.ts.net");
    let v6 = Server::parse("http://[fd7a:115c:a1e0::5]:8765").unwrap();
    assert_eq!(v6.connect_to(), ("fd7a:115c:a1e0::5".into(), 8765));
    assert_eq!(v6.host(), "[fd7a:115c:a1e0::5]");
    let local = Server::parse("http://127.0.0.1:8765").unwrap();
    assert_eq!(
        local.page("/symphony/docs/a.md#card"),
        "http://127.0.0.1:8765/symphony/docs/a.md#card"
    );
    assert_eq!(
        local.origin(),
        Url::parse("http://127.0.0.1:8765/x").unwrap().origin()
    );
}

#[test]
fn a_saved_address_is_read_back_by_the_same_rule() {
    let server = Server::parse("http://studio.tail1234.ts.net:8765/tasks").unwrap();
    let text = serde_json::to_string(&server).unwrap();
    assert_eq!(text, r#""http://studio.tail1234.ts.net:8765""#);
    assert_eq!(serde_json::from_str::<Server>(&text).unwrap(), server);
    let refused = serde_json::from_str::<Server>(r#""http://example.com""#).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("neither this Mac nor on the tailnet"),
        "{refused}"
    );
}
