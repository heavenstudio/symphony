# The Symphony's own sign-in service

`symphony-sign-in` stands in front of the server and signs each visitor in with Google or GitHub. It
keeps what signing in needs, the client secrets and the session. It passes every request on to the
server with the one signed header the server verifies ([SIGNED_IN](SIGNED_IN.md)), naming the
person the provider signed in. It is a program of its own, apart from the core: nothing links it, and the server
knows it only by its public key. Any other service that signs the same header replaces it
([what the Symphony ships for signing in](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-signing-in)).

Nothing in a project's `symphony.toml` sets it up: whoever runs the server does, once for the
machine.

## What works today, and what waits

- **Google**, by OpenID Connect. A visitor signs in with their Google account, and is the person
  whose profile holds their Google subject, `google:SUBJECT`.
- **GitHub**, by OAuth 2, as an OAuth app. A visitor signs in with their GitHub account, and is
  the person whose profile holds its numeric id, `github:ID`.
- **Either or both.** With one set, a visitor is sent straight to it. With both, a visitor is
  offered the two, as two links.
- **Sessions live in the service's memory.** A restart of the service signs every visitor out,
  and the next page they open sends them to sign in again, which the provider usually does
  straight away.
- **It speaks plain HTTP.** For an `https://` origin, something in front of it ends TLS: Caddy, a
  load balancer, a tunnel ([below](#setting-it-up)).

## What it needs

- **A Google OAuth client** of the type Web application, **a GitHub OAuth app**, or both: each
  one's client id and secret.
- **`curl` on `PATH`**, which it reaches each provider through. macOS has one; most
  Linux systems too.
- **Rust**, to build it: the toolchain this repository's `rust-toolchain.toml` names.
- **For an `https://` origin, a TLS terminator** in front of it, with a certificate for the
  origin's host.

## Setting it up

The machine folder is the server's: `~/Library/Application Support/Symphony` on macOS,
`$XDG_DATA_HOME/symphony` or `~/.local/share/symphony` elsewhere, `SYMPHONY_MACHINE_DIR` when set.
The service keeps its two files there, beside the server's `sign-in.toml`. The example origin is
`https://board.example.com`, with the server on this machine's port 8765.

1. **Build it** in a clone of this repository. It is not on crates.io and no release installs it:

   ```sh
   cargo install --locked --path crates/symphony-sign-in
   ```

2. **Make its key pair.** This writes the private key to the machine folder's
   `sign-in-service.key`, mode 600, and prints the line the server's file takes:

   ```sh
   symphony-sign-in key
   ```

   It refuses to write over a key already there, which `sign-in.toml` may name. To make a new
   pair, remove the old key first and put the new line in `sign-in.toml`.

3. **Name it for the server** in the machine folder's `sign-in.toml`, with the line printed and
   the origin browsers will reach the server at ([SIGNED_IN](SIGNED_IN.md#setting-it-up)):

   ```toml
   key = "ed25519:…"
   origin = "https://board.example.com"
   ```

4. **Make the Google client**, to sign in with Google. In the Google Cloud console, under APIs &
   Services:
   - set up the OAuth consent screen (Google Auth Platform, Branding and Audience);
   - under Clients, create a client of the type Web application;
   - add the authorized redirect URI: the origin, then `/.sign-in/callback`
     (`https://board.example.com/.sign-in/callback`);
   - copy its client id and its client secret.

   The service asks for the scope `openid` alone, so no consent screen scope needs adding. Google
   allows an `http://` redirect URI on `localhost` alone.

   **Make the GitHub OAuth app**, to sign in with GitHub. In GitHub's Settings, under Developer
   settings, OAuth Apps, register a new application:
   - the homepage URL is the origin;
   - the authorization callback URL is the origin, then `/.sign-in/github-callback`
     (`https://board.example.com/.sign-in/github-callback`);
   - generate a client secret, and copy it and the client id.

   The service asks for no scope: what it reads, the account's id, is public. GitHub's consent
   screen asks for nothing more than that.

5. **Write its settings** to the machine folder's `sign-in-service.toml`, the file its owner's
   alone (mode 600), as it holds the client secrets:

   ```toml
   listen = "127.0.0.1:8766"
   upstream = "127.0.0.1:8765"
   origin = "https://board.example.com"

   [google]
   client-id = "1234-….apps.googleusercontent.com"
   client-secret = "GOCSPX-…"

   [github]
   client-id = "Ov23li…"
   client-secret = "…"
   ```

   - **`listen`** is where the service takes requests: an address and a port, plain HTTP.
   - **`upstream`** is the server behind it: an address and a port.
   - **`origin`** is `sign-in.toml`'s, by the same rule.
   - **`[google]`** and **`[github]`** are the providers visitors sign in with: either, or both.
     Each holds its `client-id` and `client-secret`. A file that names neither is refused.

   On macOS:

   ```sh
   folder="$HOME/Library/Application Support/Symphony"
   (umask 077; "${EDITOR:-vi}" "$folder/sign-in-service.toml")
   ```

6. **End TLS in front of it** for an `https://` origin. With Caddy, which gets the certificate
   itself:

   ```
   board.example.com {
       reverse_proxy 127.0.0.1:8766
   }
   ```

   Nothing in front of the service may cache what it passes on: each answer is one signed-in
   visitor's, and the service marks every one `private`.

7. **Run it**, and keep it running with the machine's own service manager (launchd, systemd):

   ```sh
   symphony-sign-in serve
   ```

   A start reads both files and refuses to listen until they read. Where the server's
   `sign-in.toml` is in the same machine folder, it must name this key and this origin, since a
   server holding another key would refuse every request the service signs.

8. **Give each person their sign-in.** A visitor signed in opens `/.sign-in/` on the origin
   (`https://board.example.com/.sign-in/`). It says whom they are signed in as: `google:` and
   their Google subject, or `github:` and their GitHub account's id, each a number. They send that
   to the person at the machine, who gives it to their profile and a board's powers
   ([TASKS](../workflow/TASKS.md#people)):

   ```sh
   just tasks-people add ana --display "Ana Lima" --signin google:110248495921238986420 --power speak --as HANDLE
   ```

   A person who signs in with both is given both sign-ins.

   Until then the server takes them as nobody's, and they see no board.

**The server must be reachable only through the service.** A request that reaches the server
around it is judged by its address, as without the service ([SIGNED_IN](SIGNED_IN.md#setting-it-up)).
The machine's one server listens on loopback and its tailnet addresses alone, so nothing outside
the machine and the tailnet reaches it. A tailnet peer still reaches it directly, as its own
Tailscale login. Run the service on the server's machine, or on a host that alone reaches the
server.

## What it does with a request

**Its own paths are under `/.sign-in/`.** The service answers them itself, and the server behind
never sees them:

| Path | What it is |
| --- | --- |
| `/.sign-in/callback` | Google's way back, the Google client's redirect URI |
| `/.sign-in/github-callback` | GitHub's way back, the GitHub OAuth app's callback URL |
| `/.sign-in/` | the visitor's own page: whom they are signed in as, and a button to sign out |
| `/.sign-in/sign-out` | sign-out, a POST the page sends |

Any other path under `/.sign-in/` is answered `404`, and so is the way back of a provider whose
client is not set.

**Every other request needs a session.**
- **With one,** the service takes out every `Symphony-Signed-In` header the caller sent, every
  `Cookie` (the server reads none) and every header that is said of one connection alone. It sets
  `Host` to the origin's and adds a word it signs for that request alone. Then it passes the
  request to `upstream` on a connection of its own. Bodies stream both ways, so reads, saves and
  uploads pass whole. Each answer comes back marked `private`, as it is that visitor's alone: the
  server's `public`, as on an upload, gives way to it, and its `no-store` stays.
- **The feed passes on too.** An event stream streams back as the server sends it. A WebSocket's
  handshake that the server answers `101` is joined to the browser's connection both ways. Each
  ends when the word it was opened with expires, five minutes later, and the client opens it again
  with a fresh word while the session holds. A session signed out keeps no feed.
- **Without a session, nothing is passed on.** A browser's navigation to a page is sent to sign
  in, and comes back to the path it asked for: straight to the one provider set, or, with both
  set, to a page of two links, "Sign in with Google" and "Sign in with GitHub". Anything else is
  answered `401 Unauthorized: not signed in`, never sent to sign in: a fetch, a script, an image,
  a socket, a write.

**The word** is the header [SIGNED_IN](SIGNED_IN.md#the-header) sets out, signed with the
service's private key:

| Claim | What it holds |
| --- | --- |
| `iss` | `google` or `github` |
| `sub` | Google's subject for the visitor, or their GitHub account's id |
| `aud` | the origin |
| `iat` | the second the request was passed on |
| `exp` | five minutes after it |

It names no display name, no email, no group and no role, and nothing in it gives a power: what a
person may do is each board's. The browser never sees it.

## How a visitor signs in

The service uses the authorization code flow with PKCE: OpenID Connect's with Google, OAuth 2's
with GitHub, whose OAuth apps issue no id token.

1. **To the provider.** A navigation with no session starts a sign-in: a fresh `state`, `nonce`
   and PKCE verifier, each 32 random bytes. The browser is sent to the provider with the `state`,
   the verifier's S256 challenge, the client id and the redirect URI; Google is also sent the
   `nonce` and the scope `openid`, and GitHub no scope. With both set, the page of two links holds
   one sign-in, and each link sends the browser to its provider for it.
2. **The flow cookie.** The browser is given a flow cookie that holds the sign-in under way: its
   `state`, `nonce`, verifier, the path to come back to and when it ends, ten minutes on. The
   service signs it (HMAC-SHA256) with a key it makes at each start and never writes, and keeps
   nothing of it. So no number of sign-ins anyone begins pushes another out, and a restart ends
   every sign-in under way. A path longer than 2,048 bytes comes back to `/`, so the cookie stays
   inside what a browser keeps of one.
3. **Back from the provider.** The way back is taken only when the browser's flow cookie is one
   the service signed (its tag checked in constant time), its `state` is the way back's (compared
   in constant time), and its ten minutes have not passed. A way back with no flow cookie, or
   another sign-in's, is refused. The provider takes a code once, so a way back repeated, cookie
   and all, is refused by the provider.
4. **The code.** The service exchanges the provider's code at its token endpoint, through `curl`,
   with the verifier, the redirect URI and the client's id and secret. `curl` reads the request on
   its standard input, so the secret, the code and the verifier are in no process's arguments or
   environment. It reads no `~/.curlrc`, verifies the provider's certificate, speaks HTTPS alone,
   follows no redirect and uses no proxy.
5. **Google's id token.** The id token comes straight from Google's token endpoint over TLS, in
   answer to the client's secret, so the service takes it as Google's word, as OpenID Connect
   Core (3.1.3.7) allows, and does not fetch a key to check its signature. It checks every claim
   it relies on:
   - the issuer is Google's;
   - it was made for this client (`aud`, and `azp` where it names one or `aud` lists several);
   - it has not expired, and says it was issued no more than 30 seconds ahead of this machine's
     clock;
   - its `nonce` is this sign-in's;
   - it names a subject.

   The subject is Google's `sub`, which never changes and is never given to another account, as an
   email address can be. Nothing else of the token is read.
6. **GitHub's account.** GitHub answers the code with an access token, which the service uses
   once, through `curl` again, the token in its standard input, to ask GitHub's API
   (`GET https://api.github.com/user`) whose it is, and then drops: it is kept nowhere and never
   said. The subject is the account's `id`, a whole number, which never changes and is never given
   to another account, as a login given up is anyone's to take. Nothing else of the answer is
   read: no login, no email, no name.
7. **The session.** Only then is a session made. The browser is sent back to the path it asked
   for, which is always a path of the origin: no parameter of any request names where it goes.

**The cookies.** The session cookie holds the session's id, 32 random bytes, and nothing else: who
the visitor is stays in the service's memory, kept under the id's digest. It is `HttpOnly`,
`SameSite=Lax`, on the path `/` and held twelve hours from the sign-in, however it is used. The flow
cookie has the same flags, held ten minutes, and holds the signed sign-in under way. The verifier
it holds stays with the browser that began the sign-in: PKCE keeps a code from anyone else, and a
browser's own code is its own. For an `https://` origin both are `Secure` and named with
`__Host-`, so only this host can set them. For an `http://` origin, which Google allows on
`localhost` alone, they are neither, and a browser sends them to every port of `localhost`, not to
the service's alone: whatever else listens on `localhost` receives the session's id.

**A new sign-in** in a browser that holds a session ends that session as the new one begins.

**Sign-out.** The visitor's own page posts to `/.sign-in/sign-out`. The service takes that only
when the request's `Origin` is the origin. It ends the session at once and clears its cookie, and a
feed still open ends within its five minutes.

**Requests from other sites.** A browser sends a `SameSite=Lax` cookie with a cross-site navigation
alone, never with a cross-site POST, fetch or socket. So a page elsewhere cannot have a visitor's
browser write through the service: the request reaches it with no session and is answered `401`.
The server also refuses a signed write or socket whose `Origin` is not the origin.

## What is refused

A way back from either provider is refused, with a page that offers to sign in again, and no
session is made, when:
- its query names no `state`, no code, or a parameter twice;
- the browser holds no flow cookie, or one the service did not sign: forged, altered, or signed
  before a restart;
- its `state` is not the flow cookie's: a cross-site way back, or another sign-in's;
- its sign-in began more than ten minutes before;
- the provider says it did not sign the visitor in (`error=access_denied`);
- the provider's token endpoint does not answer, or answers with no token (`502`), as for a code
  taken already;
- Google's id token is malformed, not Google's, for another client, expired, issued ahead of the
  clock, for another nonce, or names no subject;
- GitHub's API does not answer whose its token is (`502`), or answers with no `id` that is a whole
  number above 0.

A sign-out from another site, or naming no `Origin`, is refused `403`, and the session holds.

The service's log says each refusal by its reason, and each failure by its cause, so whoever runs
it can see why a sign-in did not take.

## What it never does

- It never passes a request on without a session, and never passes one unsigned: a request
  passed on from this machine without a word would be this machine's person.
- It never passes on a `Symphony-Signed-In` header a caller sent.
- It never shows the word to the browser.
- It never logs or prints a client secret, the private key, a code, a `state`, a `nonce`, a
  verifier, a flow cookie, a session's id, an id token, an access token or a word.
- It never reads an email, a name, a group or a role from Google, nor a login, an email, a name
  or an organization from GitHub.
- Its settings and its key are never a link, never another account's, and never readable by
  another account. A refusal says the file and the rule, never a value.
