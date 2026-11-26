# Signing in through a service in front

A sign-in service that stands in front of the server can tell it who each request is from. It
does this with one signed header, which the server verifies and nothing more. The service signs
people in (OAuth 2 with Google, GitHub or a company's own sign-in) and keeps what that needs: the
client secret, the session and the sign-out. The server holds no password, secret, session,
cookie or redirect, and draws no sign-in page. Who the person is comes from the header. What
they may do is the board's, as for anyone named ([TASKS](../workflow/TASKS.md#people)). The
decisions are [the concept's](../product/analysis/users-and-identity.md#how-a-provider-plugs-in).

This page is the header's format, a contract kept stable across releases. It says enough for
another program to sign one.

## What works today, and what waits

The server checks the header on every request: reads and writes, on the one server and on a server
of its own. A request whose header verifies is taken as the person whose sign-in it names. The
Symphony's own sign-in service signs it for a visitor Google or GitHub signed in
([SIGN_IN_SERVICE](SIGN_IN_SERVICE.md)); any other service that signs it as below fits as well. A
hosted adapter's writes (`symphony_server::hosted`) do not go through this header.

## Which services fit

The server holds one public key, named in its own settings, and fetches no key from anywhere.

**Fits:** any service that holds the matching private key and signs this header, as below, on
each request it passes on. For example:
- the Symphony's own, `symphony-sign-in` ([SIGN_IN_SERVICE](SIGN_IN_SERVICE.md));
- a forward-auth service behind nginx `auth_request`, Caddy `forward_auth` or Traefik
  `ForwardAuth`, which signs the header and returns it for the proxy to copy onto the request;
- a small program of your own in front.

**Does not fit as it comes:** a proxy that signs an assertion of its own. Each signs with ES256 or
RS256, takes its keys from a URL, and writes an audience of its own form:
- Pomerium (`X-Pomerium-Jwt-Assertion`);
- Cloudflare Access (`Cf-Access-Jwt-Assertion`);
- Google IAP (`x-goog-iap-jwt-assertion`);
- AWS ALB (`x-amzn-oidc-data`).

`oauth2-proxy`'s `X-Forwarded-User` and `X-Forwarded-Email` are not signed at all. To use one of
these, put a signer of this header between it and the server.

## Setting it up

The machine folder's `sign-in.toml` names the service's public key and this server's origin:

```toml
key = "ed25519:a569c6c8618182a28768b82a82d102d02dfb74a55738e4ce352f147d65b61f23"
origin = "https://board.example.com"
```

- **`key`** is the public half of the service's Ed25519 key: `ed25519:` and 64 lowercase hex
  digits. A weak key is refused.
- **`origin`** is where browsers reach the server through the service: `https://` or `http://`, a
  lowercase host, and a port or none, with nothing after it. It is written as a browser sends it:
  the scheme's own port (`443` for `https://`, `80` for `http://`) is left out, never named.

The machine folder is `~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony`
elsewhere, or `SYMPHONY_MACHINE_DIR` when set ([the server guide](../../apps/symphony-server/README.md#projects)).
The file must be this account's, and no other account may write it. It may not be a link. A
project's `symphony.toml` never names the key.

The file is read on each request that carries the header. A key changed or removed holds from the
next request, with no restart. A file that does not read is said in the server's log by its path
and the rule it breaks, never a value. Until it reads, every signed request is refused.

A key pair, with OpenSSL 3 or later:

```sh
openssl genpkey -algorithm ed25519 -out sign-in.key
openssl pkey -in sign-in.key -pubout -outform DER | tail -c 32 | xxd -p -c 64
```

The second line prints the 64 hex digits that `key` takes. `sign-in.key` stays with the service
alone.

**The server must be reachable only through the service.** A request with no header is judged by
its address, as without the service: from this machine it is this machine's person, from the
tailnet by its Tailscale login ([the server guide](../../apps/symphony-server/README.md#who-a-request-is-from)).
So a client that reaches the server directly, around the service, is not stopped by this header.
Bind the server where only the service reaches it, or put both on one machine with nothing else
listening on the server's port. The service signs every request it passes on: one passed on
unsigned from this machine, naming this machine in its `Host`, is this machine's person.

A header is a bearer credential while it holds, so the service never shows it to the browser.

## The header

```
Symphony-Signed-In: HEADER.CLAIMS.SIGNATURE
```

- **One header**, at most 4096 bytes of visible ASCII. Two are refused.
- **A JWT** (RFC 7519) in JWS compact form (RFC 7515): three parts joined by `.`. Each part is
  base64url with no padding, and no bit is set past its last byte.

### Its header

A JSON object: `{"alg":"EdDSA","typ":"JWT"}`.

| Member | Rule |
| --- | --- |
| `alg` | `EdDSA` (Ed25519, RFC 8037). Required. |
| `typ` | `JWT`. May be left out. |
| `kid` | Any string. May be left out, and is not read. |

Any other member is refused: `crit`, `jwk`, `jku`, `x5c`, `x5u` and the rest.

### Its claims

A JSON object. A claim of this table given twice is refused.

| Claim | Holds | Rule |
| --- | --- | --- |
| `iss` | the service's name: the SERVICE of a profile's `SERVICE:SUBJECT` sign-in | 1 to 64 of `a`–`z`, `0`–`9`, `.`, `_` and `-`, starting with a letter or a digit; never `tailscale` |
| `sub` | the service's subject for the person | a string, not empty |
| `name` | how the service shows them | a string; may be left out |
| `aud` | the server it was made for | a string, exactly `origin` in `sign-in.toml` |
| `iat` | when it was signed | integer seconds since 1970 |
| `exp` | until when it holds | integer seconds, after `iat` and at most 900 after it |
| `nbf` | when it starts to hold | integer seconds; may be left out |

Any other claim is ignored. A `groups`, `roles` or `email` claim is never read, and nothing in the
header gives a power.

`tailscale` is the machine's own Tailscale daemon's word alone, which the server asks itself.

### Fresh

The server's clock decides. A header is refused:
- at or after its `exp`;
- when its `iat` or its `nbf` is more than 30 seconds ahead of the server's clock;
- when it holds longer than 900 seconds from its `iat`.

A service signs a header for each request, or keeps one for no more than those 15 minutes.

### Its signature

Ed25519 (RFC 8032) over the ASCII bytes of `HEADER.CLAIMS`: the first two parts as sent, with the
dot between them. It is 64 bytes. The check is the strict one: a non-canonical signature, or one
under a weak key, is refused.

### Signing one

From a shell, with the key made above:

```sh
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
now=$(date +%s)
header=$(printf '%s' '{"alg":"EdDSA","typ":"JWT"}' | b64url)
claims=$(printf '{"iss":"google","sub":"%s","aud":"https://board.example.com","iat":%d,"exp":%d}' \
  "$SUBJECT" "$now" $((now + 300)) | b64url)
printf '%s.%s' "$header" "$claims" > input
signature=$(openssl pkeyutl -sign -inkey sign-in.key -rawin -in input | b64url)
printf 'Symphony-Signed-In: %s.%s.%s\n' "$header" "$claims" "$signature"
```

Any JWT library that signs EdDSA makes the same header.

## Who it is

The server reads `iss` and `sub` as a sign-in, `iss:sub`. It looks for that sign-in among the
profiles in the machine folder, as it does for a Tailscale login's `tailscale:LOGIN`
([TASKS](../workflow/TASKS.md#people)).
- **The profile holding it is the person.** On each board, they have what that board gives them.
- **A board that gives them nothing is not there for them.** It is in no list they see, and its
  pages answer as a project the server does not hold. A server of its own refuses them.
- **A sign-in no profile holds is nobody's.** It sees no board.
- **Case:** a sign-in is matched ignoring ASCII case, as every sign-in is. A service whose
  subjects can differ by case alone does not fit.

A signed request is never this machine's person, whatever address it comes from. It may name the
service's origin in its `Host`. A browser's write, or its socket for the feed, must name exactly
that origin in its `Origin`. A request with no header may not name the origin in its `Host`,
unless that host is a name of this machine anyway.

## What is refused

A request whose header does not verify is answered `403` before any route, with one body whatever
the reason:

```
Forbidden: this request's signed sign-in is not taken
```

These are refused, and nothing is read or written:
- a header that is not signed, has been changed, or was signed by another key;
- one that is stale, not yet valid, or held too long;
- one made for another origin;
- one naming no service a profile can hold, or `tailscale`;
- two headers, or one oversized or malformed;
- any header where `sign-in.toml` is missing or does not read.

So a proxy whose key was removed refuses its visitors rather than making them this machine's
person. The header, its claims and the key are never logged.

## What it fixes for later releases

These cannot change without breaking every service that signs:
- the header's name and its JWS compact form;
- what is signed (`HEADER.CLAIMS`);
- the meaning of `iss`, `sub`, `name`, `aud`, `iat`, `exp` and `nbf`;
- `iss:sub` read as a profile's sign-in;
- `aud` as the origin's string;
- integer seconds.

These can be added without breaking a signer:
- another `alg`;
- several keys, each tried in turn, so a key can be rotated with no `kid`;
- an `aud` given as an array.
