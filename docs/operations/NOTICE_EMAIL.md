# Notices by email

`symphony-notice-email` sends a person's notice as a mail to their address. It is a notice
plugin: a program of its own, apart from the core, which the server hands one notice at a time on
its stdin and which answers by its exit status ([NOTICES](../engineering/NOTICES.md)). It reaches
the mail server by SMTP through `curl`, and keeps its settings and its password in the machine
folder, never in a project's `symphony.toml`. The decision is
[the concept's](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-notices).

## What works today, and what waits

The plugin sends the notice it is handed. Nothing hands it one yet: the server's part (who is
told, of what and by which way, the settings that name each way's program, and its hand again
after a `75`) is step 4 of
[the way from one owner](../product/analysis/users-and-identity.md#the-way-from-one-owner), and
the address it sends to will be the email in the person's profile. Until then it is set up as
below and [checked by hand](#checking-it).

## What it needs

- **A mail server that takes mail by SMTP**, and an account on it: an address to send from, and
  the user and password that log in. A mail service is the same thing: its SMTP settings name its
  host and its user, and the password is its key (SendGrid's user is `apikey`, for one). A relay
  on the machine itself (`127.0.0.1`) may need no login.
- **`curl` on the `PATH`** of the server that runs the plugin, with SMTP (`curl --version` lists
  `smtp smtps`), as macOS and most Linux systems ship it.

## Building it

It is not on crates.io and no release installs it, so it is built from this repository's
source. In a clone of it:

```sh
cargo install --locked --path crates/symphony-notice-email
```

That puts `symphony-notice-email` in Cargo's `bin` folder (`~/.cargo/bin`). It links
`symphony-notice` alone of the workspace, and nothing of the core links it
(`crates/symphony-notice-email/tests/boundary.rs`).

## Its settings

Everything it keeps is in the machine folder's `notices/`: the folder named by
`SYMPHONY_MACHINE_DIR` where the server's environment sets it, else
`~/Library/Application Support/Symphony` on macOS and `$XDG_DATA_HOME/symphony` (or
`~/.local/share/symphony`) elsewhere, as the server finds it. Every notice plugin keeps its files
there, each named for its way.

- `notices/` is a folder its owner alone reaches: `chmod 700`.
- `notices/email.toml` holds the settings, a file its owner alone reads: `chmod 600`.
- `notices/email.token` holds the password on one line, the whitespace around it dropped, also
  `chmod 600`. It is there exactly when the settings name a `user`.

A folder or a file that is a link, or that others can read, is refused before anything in it is
read, and nothing is sent: keep the folder and the files themselves there. Settings that do not
read are refused by their line, never by what it holds: a key the settings do not know (a
password in `email.toml` is such a key) is said only to be one, and a value of another type than
its key takes is named by its key.

| Key | What it sets |
| --- | --- |
| `server` | the mail server: `smtps://HOST[:PORT]` for TLS from the start (port 465 unless named), or `smtp://HOST[:PORT]`, where STARTTLS is required or nothing is sent (port 25 unless named; a submission port is usually 587). To this machine's loopback (`localhost`, `127.0.0.1`, `[::1]`), `smtp://` goes in the clear, as `curl` asks no STARTTLS there. No path, query or login in it |
| `from` | the address the mail is from, plain `local@domain`, shown as `Symphony <from>` |
| `user` | the user that logs in, with `email.token`'s password; none for a relay that takes mail without a login |

A mail service, with a key:

```toml
server = "smtps://smtp.sendgrid.net"
from = "symphony@example.com"
user = "apikey"
```

A relay on the machine:

```toml
server = "smtp://127.0.0.1:25"
from = "symphony@example.com"
```

To make the files, on macOS:

```sh
folder="$HOME/Library/Application Support/Symphony/notices"
mkdir -p "$folder" && chmod 700 "$folder"
touch "$folder/email.toml" && chmod 600 "$folder/email.toml"
touch "$folder/email.token" && chmod 600 "$folder/email.token"   # with a `user` alone
```

then write the settings and the password in with an editor, never on a command line, where the
shell's history keeps it.

## What it sends

One mail for each notice, all ASCII, to the notice's address alone: `multipart/mixed` with one
part, its text plain.

- **From** `Symphony <from>`, **To** the person's address.
- **Subject** what waits on the person and the project: `A card waits on you in symphony`,
  `A question waits on you in symphony`, `A reviewer gave a verdict in symphony`, and for a kind
  this build does not know, `Something waits on you in symphony`.
- **The body** that sentence and the notice's link: a notice carries no title and no words
  ([NOTICES](../engineering/NOTICES.md#the-message)).
- **Message-ID** made from the notice's id, at the domain of `from`, and **Auto-Submitted:
  auto-generated**, so a vacation reply does not answer it.

The password, the address and the link are in no argument and no environment of any process:
`curl` reads the whole request on its stdin, and `-q` keeps a `~/.curlrc` from adding to it.
No proxy is used, whatever proxy variable the server's environment sets (`noproxy = "*"`), so
the login goes to the mail server alone. `curl` checks the mail server's certificate against the
system's. The plugin keeps nothing of a notice and logs nothing: what it says on stderr names
neither the address, the link nor the password, and of the mail server's answer only its code and
the command it answered, as an answer often repeats the address.

**A notice may arrive twice.** SMTP takes no key that makes a second hand of one mail the same
mail. Where the connection breaks after the mail server took the mail, before it said so, the
plugin answers `75`, and the server's hand again sends it again. The Message-ID is the same on
both, so a mailbox that drops a repeated Message-ID shows it once; many do not.

## What each exit says

| Exit | When |
| --- | --- |
| `0` | the mail server took the mail |
| `75` | not sent this time: the mail server answered a `4xx` (busy, a limit, a login it cannot check now), or it was not reached, a TLS handshake did not finish, it stopped answering, a minute passed, the machine ran short of processes, memory or open files to start `curl`, or `curl` failed in any other way that may clear |
| `1` | not sent, and handing it again will not send it: the mail server answered a `5xx` (the login, `from`, the address or the message refused); the notice is by another way than `email`, or its address is no plain `local@domain`; the settings are missing or refused; `curl` failed in a way a second hand meets again (the certificate refused, STARTTLS not offered, CA certificates it cannot read, a protocol, feature or option this `curl` lacks: its exits 1, 2, 3, 4, 26, 48, 60, 64 and 77); `curl` is not on `PATH` or may not be run (not executable); or the message is not one it reads ([NOTICES](../engineering/NOTICES.md#the-hand-off)) |

Stderr says which, in one line, with `curl`'s own line where it has one:

```text
not sent: the mail server answered 535 to AUTH, refusing the login: check `user` in notices/email.toml and the password in notices/email.token (curl: (67) Login denied)
```

## Checking it

With the settings written, hand it a notice to an address of your own, as the server will:

```sh
printf '%s\n' '{"notice":1,"id":"check-1","kind":"card","project":"symphony","link":"https://example.com/symphony/","to":{"way":"email","address":"you@example.com"}}' \
  | symphony-notice-email; echo "exit $?"
```

`exit 0` and the mail in that mailbox is the plugin set up. Anything else is said on the line
before the exit.
