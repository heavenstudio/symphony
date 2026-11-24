# Sending notices by Slack

`symphony-notice-slack` sends a person's notice as a direct message from your Slack workspace's
app to that person. It is a notice plugin: a program of its own, apart from the core, which the
server hands one notice at a time by [the notice message's contract](../engineering/NOTICES.md),
on its stdin, and reads what came of it from its exit status. It keeps its own secret, the app's
bot token, and reaches Slack's Web API through `curl`
([what the Symphony ships for notices](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-notices)).

Nothing in a project's `symphony.toml` sets it up: whoever runs the server does, once for the
machine.

## What works today, and what waits

- **The machine's one server hands it each notice by Slack** once it is named for the way `slack`
  ([below](#setting-it-up)): who is told, of what, and when a notice is handed again are
  [the server's part](../engineering/NOTICES.md#the-servers-part).
- **A notice handed again may arrive twice.** Slack's `chat.postMessage` takes no idempotency key,
  and the program keeps no record of the notices it sent. A notice it answered `75` for because
  Slack did not answer in time may have reached the person, and handed again it is sent again.

## What it needs

- **A Slack app installed in the workspace**, with the bot token scope `chat:write` and nothing
  else, and its bot token (`xoxb-…`).
- **`curl` on `PATH`.** macOS has one; most Linux systems too.
- **Rust**, to build it: the toolchain this repository's `rust-toolchain.toml` names.

## Setting it up

1. **Make the app.** At <https://api.slack.com/apps>, Create New App, From scratch, in your
   workspace. Under OAuth & Permissions, add the Bot Token Scope `chat:write`. Under App Home,
   keep the Messages Tab shown: a person reads the notices there.
2. **Install it** in the workspace (OAuth & Permissions, Install to Workspace) and copy its Bot
   User OAuth Token. Reinstall it after any change of its scopes.
3. **Keep the token** in the machine folder's `notices/slack.token`, the folder its owner's alone
   (mode 700) and the file too (600). The machine folder is the server's:
   `~/Library/Application Support/Symphony` on macOS, `$XDG_DATA_HOME/symphony` or
   `~/.local/share/symphony` elsewhere, `SYMPHONY_MACHINE_DIR` when set. On macOS, with the
   token copied:

   ```sh
   folder="$HOME/Library/Application Support/Symphony/notices"
   mkdir -p -m 700 "$folder"
   (umask 077; pbpaste > "$folder/slack.token")
   ```

   The program refuses a folder or a file anyone else may read, or a link in the place of either,
   and reads nothing from it: keep the folder and the file themselves there. The file is opened
   without following a link, and its mode is read from the file opened, so the file checked is the
   file read.
   Every notice plugin keeps what it needs in `notices/`, named for its way.
4. **Build it** in a clone of this repository. It is not on crates.io and no release installs it:

   ```sh
   cargo install --locked --path crates/symphony-notice-slack
   ```

5. **A person's address** for the way `slack` is their Slack member id (`U024BE7LH`): in Slack,
   their profile, then ⋮, then Copy member ID. An address of any other shape is refused, a
   channel's id (`C…`) among them, so a notice is never posted to a whole channel. A person writes
   theirs on their card of the People page, held to the same rule
   ([TASKS](../workflow/TASKS.md#the-people-page)).
6. **Name it for the server** in the machine folder's `notices/programs.toml`, by the absolute path
   `cargo install` put it at, the file the owner's alone and written by nobody else
   ([the server's part](../engineering/NOTICES.md#the-servers-part)):

   ```toml
   slack = "/Users/ana/.cargo/bin/symphony-notice-slack"
   ```

## Trying it by hand

This sends a real message, to the member id you write, so write your own:

```sh
printf '%s\n' '{"notice":1,"id":"try-1","kind":"card","project":"symphony","link":"https://example.com/","to":{"way":"slack","address":"U024BE7LH"}}' \
  | symphony-notice-slack; echo "exit $?"
```

## What it sends

One message in the app's direct messages with the person, saying what waits and in which project,
with the notice's link:

| Kind | The message |
|---|---|
| `card` | A card waits on you in *symphony*. Open it |
| `question` | An agent's question waits on you in *symphony*. Open it |
| `verdict` | A reviewer gave a verdict in *symphony*. Open it |
| one this build does not know | Something on the Symphony waits on you in *symphony*. Open it |

"Open it" is the link. No link is unfurled, so Slack never fetches the page.

## What came of it

Its exit status, with why on stderr when it did not send:

- **`0`**: Slack sent it.
- **`75`**, worth handing again: Slack limits the rate (HTTP 429, `ratelimited`), is down or slow
  (HTTP 5xx, `service_unavailable`, `internal_error`, `fatal_error`, `request_timeout`), or is
  moving the workspace into an Enterprise organization (`team_added_to_org`); `curl` reached no
  answer (not reached, out of time, cut off), or the machine ran short of processes, memory or open
  files to start it; or Slack's answer said neither that it sent nor that it did not.
- **`1`**, never sent, and handing it again will not send it: the message is not one it takes
  (by the contract), the notice goes by another way than `slack`, the address is no member id, no
  token was read (stderr names the path and what to do), `curl` is not on `PATH` or may not be
  run (not executable), `curl` failed in a way a second hand meets again (a certificate that does
  not verify, CA certificates it cannot read, a protocol, feature or option this `curl` lacks: its
  exits 1, 2, 3, 4, 26, 48, 60, 64 and 77), Slack answered another HTTP status, or Slack refused it
  with any other error. The ones met most:

| Slack's word | What to do |
|---|---|
| `channel_not_found`, `user_not_found` | The member id names nobody in the app's workspace: correct the person's address. |
| `cannot_dm_bot` | The member id is a bot's. |
| `invalid_auth`, `not_authed`, `token_revoked`, `token_expired`, `account_inactive` | The token is not the installed app's: install it again and replace `slack.token`. |
| `missing_scope` | Add `chat:write` to the app's bot token scopes and reinstall it. |
| `not_allowed_token_type` | The file holds a user or app-level token: keep the Bot User OAuth Token. |

## What it never does

- **Print the address, the link or the token.** Stderr names Slack's error word, never a word
  Slack sends that is not a plain one, and a failure `curl` reports is said with the token
  replaced.
- **Put the token where another process reads it.** `curl` reads the whole request on its stdin
  (`--config -`), so the token is in no argument and no environment, and `-q` keeps a `~/.curlrc`
  from adding to it. No redirect is followed and no proxy is used, whatever proxy variable the
  server's environment sets (`noproxy = "*"`): the token goes to `slack.com` alone.
- **Keep or pass on the message**, or decide who is told: it sends the one notice it was handed
  to the one address in it.
