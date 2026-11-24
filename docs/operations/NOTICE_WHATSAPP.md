# WhatsApp notices

`symphony-notice-whatsapp` sends a person's notice by WhatsApp: a template message, from a
business number of yours, to the number in the person's profile, carrying the link to the page
the notice is about. It is a notice plugin, the server's and apart from the core
([users and identity](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-notices)):
a program of its own that the server hands one notice at a time on its stdin, by the contract in
[NOTICES](../engineering/NOTICES.md). Nothing of it is in a project's `symphony.toml`, and no other
package of the workspace builds with it (`crates/symphony-notice-whatsapp/tests/plugin/boundary.rs`).

Of the three services WhatsApp costs the most to set up and to run: Meta must approve each
message's template before anything is sent, the person must agree to be messaged, and Meta charges
for the messages.

## What works today, and what waits

- **The machine's one server hands it each notice by WhatsApp** once it is
  [named for the way `whatsapp`](#naming-it-for-the-server): who is told of what, and when a notice
  is handed again, are [the server's part](../engineering/NOTICES.md#the-servers-part).
- **A notice carries a link alone**, so the message says what kind of thing waits on the person
  and links to it. What a notice carries outside the Symphony is a later round's card.
- **It sends through the WhatsApp Cloud API**, the Graph API's `/{phone number id}/messages`, and
  no other provider's.

## What you need from Meta

1. **A WhatsApp Business account on Meta's Cloud API** and a business phone number registered
   on it, the number notices are sent from. WhatsApp Manager shows the number's **phone number
   id**, a string of digits, which the settings name; the number itself is not needed.
2. **A token that may send from that number**: a system user's token with the
   `whatsapp_business_messaging` permission, made in Meta's Business settings. A user's
   temporary token from the app's dashboard works for a try and expires within a day.
3. **The templates, approved.** WhatsApp opens a conversation with a person only by a template
   Meta has approved, so each kind of notice is a template submitted first, in WhatsApp
   Manager, in the category Utility. Each template's body takes one variable, `{{1}}`, which the
   program fills with the notice's link, and no other variable, header or button. For example:

   | Template | Body | Sent for |
   | --- | --- | --- |
   | `symphony_waits` | `Something on the Symphony waits on you: {{1}}` | a kind with no template of its own, and a kind this build does not know |
   | `symphony_card` | `A card waits on you on the Symphony: {{1}}` | `card` |
   | `symphony_question` | `An agent asks you a question on the Symphony: {{1}}` | `question` |
   | `symphony_verdict` | `A reviewer gave a verdict on the Symphony: {{1}}` | `verdict` |

   The one for any kind is needed; the others are optional, and a kind without its own is sent
   by it. All of them are approved in one language, which the settings name (`en_US`).

## The person's agreement, and what it costs

- **The person agrees to be messaged.** Meta lets a business message only people who agreed to
  it. Here the person agrees by writing their WhatsApp number in their own profile and choosing
  WhatsApp as a way their notices come by. The program decides nothing of that: it sends the one
  notice it is handed to the one number the notice names.
- **Meta charges for the messages**, by its price list for the person's country and the
  template's category, to the payment method of the WhatsApp Business account. An account
  without one sends nothing, and the program says so (`131042`, [below](#what-it-answers)).
- **A notice may be sent twice.** The Cloud API takes no idempotency key, so a request Meta took
  and whose answer never came back (the connection lost, or 30 seconds passed) is a notice not
  sent this time (`75`), and when the server hands it again the person gets it again, and it is
  charged again.

## Building the program

```sh
tools/cargo.sh build --release -p symphony-notice-whatsapp
```

The program is `.target/release/symphony-notice-whatsapp`; keep it where the machine will name
it ([below](#naming-it-for-the-server)). It needs `curl` on `PATH`, which it reaches Meta through. No proxy is used, whatever
proxy variable the server's environment sets (`noproxy = "*"`), so the token goes to the API's host
alone.

## Its folder, settings and token

Everything it keeps is in the machine folder's `notices/`: `~/Library/Application
Support/Symphony/notices/` on macOS, `$XDG_DATA_HOME/symphony/notices/` or
`~/.local/share/symphony/notices/` elsewhere, and `$SYMPHONY_MACHINE_DIR/notices/` where that
variable names the machine folder, as it does in a test. The folder is its owner's alone (mode
700) and so is each file in it (mode 600); one open to anyone else, or a link in the place of
one, is refused before anything in it is read. A file is opened without following a link, and its
mode is read from the file opened, so the file checked is the file read. Keep the folder and the
files themselves there, and a token kept elsewhere at the path `token_file` names, which is held to
the same.

```sh
mkdir -p ~/Library/Application\ Support/Symphony/notices
cd ~/Library/Application\ Support/Symphony/notices
chmod 700 .
touch whatsapp.toml whatsapp.token
chmod 600 whatsapp.toml whatsapp.token
```

Then write the token, as Meta's page shows it, in `whatsapp.token`, and the settings in
`whatsapp.toml`:

```toml
phone_number_id = "106540352242922"
language = "en_US"

[templates]
other = "symphony_waits"
card = "symphony_card"
question = "symphony_question"
verdict = "symphony_verdict"
```

| Key | Default | What it sets |
| --- | --- | --- |
| `phone_number_id` | none | the id of the business number notices are sent from: 1 to 32 digits, written as text |
| `language` | none | the code of the language the templates are approved in: `en`, `en_US`, `pt_BR` |
| `templates.other` | none | the template a kind with no template of its own is sent by |
| `templates.card`, `templates.question`, `templates.verdict` | `templates.other` | each kind's own template |
| `token_file` | `notices/whatsapp.token` | the absolute path of the file holding the token, held to the same mode 600 |
| `api` | `https://graph.facebook.com/v27.0` | the Graph API and its version: an `https` URL, or `http://127.0.0.1:PORT` for a test's stand-in |

A template's name is Meta's: lowercase letters, digits and underscores. Settings that do not read
are refused by their line, never by what it holds: a missing key and a value of another type or
shape are named by their key, and a key the program does not know is said only to be one, or, where
it is one of the program's own keys written in the wrong table, named. The token file holds the one
token and nothing else.

## Naming it for the server

The machine folder's `notices/programs.toml` names each way's program by its absolute path, the
file its owner's alone and written by nobody else
([the server's part](../engineering/NOTICES.md#the-servers-part)). A copy kept outside the
checkout outlives the next build:

```toml
whatsapp = "/Users/ana/.local/bin/symphony-notice-whatsapp"
```

## What it answers

The exit status is the contract's ([NOTICES](../engineering/NOTICES.md#the-hand-off)), and stderr
says why a notice was not sent, in one line. Nothing it says holds the token, the person's number
or the link: what Meta answers has each of them taken out.

| Status | When |
| --- | --- |
| `0` | Meta took the message. It is delivered or not later, as Meta's own reports of it say; the program reads none of them |
| `75` | Meta did not answer (`curl` failed in any other way, or the machine ran short of processes, memory or open files to start it), answered `429` or `5xx`, or answered one of its errors that say to try later: `2`, `4`, `80007`, `130429`, `131000`, `131016`, `131056`, `131057`, `133004` |
| `1` | anything else: Meta's other refusals (an expired token `190`, a template that does not exist in the language `132001`, a number not on WhatsApp `131026`, the account's payment `131042`); a notice of another way; a number without its `+` and country code, which is never sent, as Meta would put the business number's country code before it and could reach another person; settings or a token that do not read; no `curl` on `PATH`, or one that may not be run (not executable); `curl` failed in a way a second hand meets again, nothing sent (a certificate that does not verify, CA certificates it cannot read, a protocol, feature or option this `curl` lacks: its exits 1, 2, 3, 4, 26, 48, 60, 64 and 77); a message the program does not read |

A refusal from Meta reads `WhatsApp refused the notice: HTTP 400, code 132001: …`, with Meta's
code and its words. Meta's
[error codes](https://developers.facebook.com/docs/whatsapp/cloud-api/support/error-codes) say
what each one asks of you.

## Checking it

A notice written on its stdin by hand is sent like any other, to a real number, and charged: send
one to your own number, which has agreed. With the folder set up as above:

```sh
printf '%s\n' '{"notice":1,"id":"check-1","kind":"card","project":"symphony","link":"https://example.com/","to":{"way":"whatsapp","address":"+15551234567"}}' \
  | .target/release/symphony-notice-whatsapp; echo "exit $?"
```

Write your own number in place of `+15551234567`. `exit 0` and the message on your phone is the
plugin working; any other status comes with its line on stderr, as above.
