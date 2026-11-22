//! A server the app may follow and show, and the rule that admits one: plain HTTP to this Mac, to
//! the tailnet's addresses and to its `*.ts.net` names, and nothing else. The servers have no login
//! and speak plain HTTP, which only loopback and the tailnet's tunnel keep private; a hosted server's
//! HTTPS and login are not here yet (docs/product/analysis/desktop-client.md, Trust and access).

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// The tailnet's IPv4 range, `100.64.0.0/10`: its first address and the length of its prefix.
const TAILNET_V4: (Ipv4Addr, u32) = (Ipv4Addr::new(100, 64, 0, 0), 10);
/// The tailnet's IPv6 range, `fd7a:115c:a1e0::/48`.
const TAILNET_V6: (Ipv6Addr, u32) = (Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0, 0), 48);
/// The tailnet's names end so: its DNS names a machine `NAME.TAILNET.ts.net`.
const TAILNET_NAMES: &str = ".ts.net";

/// A server's origin, `http://HOST:PORT`: what its pages, its list and its feeds are read from.
/// Made only by [`Server::parse`], so every one the app holds passed the rule.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Server {
    /// Its origin as a URL with the path `/`: the scheme is `http` and the host one the rule admits.
    url: Url,
}

/// Why an address is not a server the app may follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// It is no URL at all.
    Unreadable(String),
    /// Its scheme is not `http`.
    Scheme(String),
    /// It names a user or a password, which no server here reads.
    Credentials,
    /// Its host is neither loopback, nor on the tailnet, nor a tailnet name.
    Host(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Unreadable(why) => {
                write!(f, "not an address ({why}): write it as http://HOST:PORT")
            }
            Refused::Scheme(scheme) => write!(
                f,
                "a server is reached over http://, not {scheme}://: the servers speak plain HTTP, kept private by loopback or the tailnet"
            ),
            Refused::Credentials => write!(f, "a server's address names no user or password"),
            Refused::Host(host) => write!(
                f,
                "{host} is neither this Mac nor on the tailnet: only loopback, 100.64.0.0/10, fd7a:115c:a1e0::/48 and *.ts.net names are followed over plain HTTP"
            ),
        }
    }
}

impl Server {
    /// The server at `text`, a URL pasted as it is: its origin alone is kept, so a page's address
    /// names its server (`http://studio.tail1234.ts.net:8765/symphony/tasks` is
    /// `http://studio.tail1234.ts.net:8765`).
    pub fn parse(text: &str) -> Result<Server, Refused> {
        let url =
            Url::parse(text.trim()).map_err(|error| Refused::Unreadable(error.to_string()))?;
        if url.scheme() != "http" {
            return Err(Refused::Scheme(url.scheme().to_owned()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Refused::Credentials);
        }
        let admitted = match url.host() {
            Some(Host::Ipv4(address)) => address.is_loopback() || within_v4(address, TAILNET_V4),
            Some(Host::Ipv6(address)) => address.is_loopback() || within_v6(address, TAILNET_V6),
            Some(Host::Domain(name)) => {
                let name = name.strip_suffix('.').unwrap_or(name);
                name == "localhost"
                    || name
                        .strip_suffix(TAILNET_NAMES)
                        .is_some_and(|machine| !machine.is_empty() && !machine.ends_with('.'))
            }
            None => false,
        };
        if !admitted {
            return Err(Refused::Host(url.host_str().unwrap_or_default().to_owned()));
        }
        let mut origin = url;
        origin.set_path("/");
        origin.set_query(None);
        origin.set_fragment(None);
        Ok(Server { url: origin })
    }

    /// Its origin, as a page's own is compared with it.
    pub fn origin(&self) -> url::Origin {
        self.url.origin()
    }

    /// The address of `path` on this server: `/` is its project list, `/<id>/` a project's home.
    /// A path is the server's word (a card's link, a project's id, a task's slug), so one that
    /// would lead off this origin (`//elsewhere/`, `/\elsewhere/`, an address of its own, another
    /// scheme) gives the project list instead.
    pub fn page(&self, path: &str) -> String {
        match self.url.join(path) {
            Ok(page) if page.origin() == self.url.origin() => page.into(),
            _ => self.url.to_string(),
        }
    }

    /// Its host as the request's `Host` names it, `[…]` around an IPv6 address.
    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    /// Its port, 80 when the address names none.
    pub fn port(&self) -> u16 {
        self.url.port_or_known_default().unwrap_or(80)
    }

    /// Whether it is this Mac, reached by loopback.
    pub fn is_loopback(&self) -> bool {
        match self.url.host() {
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            Some(Host::Domain(name)) => name.trim_end_matches('.') == "localhost",
            None => false,
        }
    }

    /// The name a server is listed by until it is renamed: "This Mac" for loopback, else the first
    /// label of its tailnet name (`studio` for `studio.tail1234.ts.net`), else its address.
    pub fn default_name(&self) -> String {
        if self.is_loopback() {
            return THIS_MAC.to_owned();
        }
        match self.url.host() {
            Some(Host::Domain(name)) => name.split('.').next().unwrap_or(name).to_owned(),
            _ => self.host().to_owned(),
        }
    }

    /// The host and port to connect to: the address itself, or the name the system resolves.
    pub(crate) fn connect_to(&self) -> (String, u16) {
        let host = match self.url.host() {
            Some(Host::Ipv6(address)) => address.to_string(),
            _ => self.host().to_owned(),
        };
        (host, self.port())
    }
}

/// The name this Mac's own server is listed by.
pub const THIS_MAC: &str = "This Mac";

impl fmt::Display for Server {
    /// Its origin, `http://HOST:PORT`, the port left out when it is 80.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url.origin().ascii_serialization())
    }
}

impl serde::Serialize for Server {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Server {
    /// A saved address read by the same rule as a pasted one, so a list file edited by hand admits
    /// no server the app would refuse.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Server, D::Error> {
        let text = String::deserialize(deserializer)?;
        Server::parse(&text).map_err(serde::de::Error::custom)
    }
}

fn within_v4(address: Ipv4Addr, (first, prefix): (Ipv4Addr, u32)) -> bool {
    let mask = u32::MAX << (32 - prefix);
    u32::from(address) & mask == u32::from(first) & mask
}

fn within_v6(address: Ipv6Addr, (first, prefix): (Ipv6Addr, u32)) -> bool {
    let mask = u128::MAX << (128 - prefix);
    u128::from(address) & mask == u128::from(first) & mask
}

#[cfg(test)]
#[path = "address_tests.rs"]
mod tests;
