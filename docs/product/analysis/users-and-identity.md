# Users and identity

Several people on one board. Who each one is, is proved outside the Symphony's core by a service
that signs them in; what each may do is the Symphony's own, kept with the board: the groups that
own what waits to be decided, and the powers to speak, decide and direct agents. This is
a concept: it questions the idea against what the Symphony has, says the shape that fits, and
puts its choices to the owner a few at a time; those not asked yet are [listed at its
end](#later-rounds). What is built of it is said at each step of [the way from one
owner](#the-way-from-one-owner), and nothing is built from it but by a task filed from an approved
card.

Read at `59538b83e`, and the settings concept as it landed on main at `b31695d8d`. The links name
the modules and documents read.

## The answer in short

- **The Symphony has a place where the owner sits, and no person.** Every browser the server
  admits writes as the one sender `owner`. Who that browser belongs to is never read
  ([the picture](#what-the-symphony-has-today)).
- **People are a board's.** Who is on a board, in which groups and with what powers is kept with
  that board and in no repository, so two boards can hold different people
  ([its card](#people-kept-where)). What a document carries of a person is a short
  public name.
- **Signing in and what a person may do are two things, kept apart.** Authentication is a service
  outside the core. Authorization is the board's ([the picture](#the-shape-proposed)), and no
  group a sign-in service knows of is read as one of a board's.
- **A group owns what waits to be decided.** A card or a task belongs to a group, or to one
  person. Its members are told, and any one of them settles it
  ([decided](#who-in-a-group-decides)). They also move, prioritize and cancel the group's tasks,
  and anyone who may speak files one ([decided](#who-acts-on-a-task)). The Settings page is a
  place like any other, and its group's members change its settings
  ([decided](#who-changes-settings)).
- **Three things one owner does today come apart.** Speaking to people, deciding, and directing
  agents are held separately. Anyone on a board speaks to anyone, anywhere. The words of someone
  who may not direct agents reach no agent until someone who may passes them on
  ([decided](#words-without-agent-reach)), but for the note beside a verdict, which is written with
  it ([decided](#the-note-beside-a-verdict)).
- **Agents are personal or shared.** A personal agent runs on its person's machine and credits
  and answers only them, and on a task may do what they may, a review among it
  ([decided](#whose-agent-may-review)). A shared agent is the board's, and works for whoever may
  direct agents.
- **The core signs nobody in.** A sign-in service, a plugin of the server and never of a project,
  does that, with OAuth 2, and hands the Symphony signed data saying who the person is. The core
  only verifies it ([how](#how-a-provider-plugs-in)). The Symphony ships one such service, and
  any other that signs the same way replaces it.
- **A person's lines carry their name**, in the owner's place
  ([decided](#how-a-persons-name-is-written)), and a notice by Slack, WhatsApp or email is sent by
  that service's plugin, the server's and apart from the core ([decided](#who-sends-a-notice)).
- **A project's plugin is told who sent a write, and nothing more.** Never what a person may do,
  never how to reach them, and nobody on a read ([decided](#what-a-plugin-is-told-of-who-asked)).
- **Not on a board, a person cannot see it exists.** That holds for a machine of the tailnet
  under another login as for a hosted visitor ([decided](#a-tailnet-machine-under-another-login)).
- **A machine with one person changes in nothing until they choose.** With nobody named on a
  board, the person at the machine holds everything there, as today. Their lines read `owner`
  until they give themselves a short name, which then carries them on every board
  ([decided](#how-a-persons-name-is-written)).

## What the Symphony has today

![Today: everyone the gate lets in writes as the one owner](users-and-identity/today.svg)

| What | How it is today | Where |
| --- | --- | --- |
| Who may reach the server | The machine itself, and any address `tailscale whois` names a node for. The answer's `UserProfile` (a login, a display name) is in the same output and is not read. | `apps/symphony-server/src/gate.rs` |
| Who a browser write is from | `owner`, always: a card's verdict (`owner · approved`), a message on a task, a task filed from the `+` sheet, a move, the approve that is the `owner` stamp. | [REVIEW](../../workflow/REVIEW.md), [TASKS](../../workflow/TASKS.md#talking-on-a-task) |
| Who a command-line write is from | An agent session, by the handle it names (`--as HANDLE`). The handle is claimed, never proved. The command line refuses `owner` as an actor. | `crates/tasks/src/cli/mod.rs` |
| A principal | A kind (human, service or agent), an issuer and a subject, with no credential. The local host builds three: the local owner, the board's services, an agent by its handle. | `symphony_plugin::execution::Principal`, `crates/tasks/src/workflow.rs` |
| What a principal may do | A capability a host's grants name for it. The local host's grants are fixed in code: the local owner holds `owner`. A custom workflow can declare more capabilities, and an edge can wait for a person (`human_wait`). | [WORKFLOW](../../workflow/WORKFLOW.md#admission) |
| A decision that needs several people | A workflow guard: the project keeps its own approval ledger and signs what it decided; the board verifies and never holds the ledger. | [WORKFLOW](../../workflow/WORKFLOW.md#guards) |
| A hosted visitor | `hosted::router` asks its `Host` for proved transport, a verified identity and a grant for the actor, the project and the action. Reads are served. A save or a plugin call goes only to `Host::dispatch`, which refuses unless the host answers, and is never passed to the local owner's saves. | [the server guide](../../../apps/symphony-server/README.md#hosted-admission), [hosted ingress](../../operations/analysis/hosted-ingress.md) |
| A plugin | Trusted code of the project, selected by its `symphony.toml`, drawing blocks. Its calls carry no person. | [PLUGIN_PROTOCOL](../../engineering/PLUGIN_PROTOCOL.md) |
| Settings | The committed project file changes by a landing. One machine's preferences are `symphony.local.toml`. The server's own state (its registry of projects, its releases) is the machine folder. | [PROJECT_FILE](../../operations/PROJECT_FILE.md) |
| Notices and the queue | Kept per recipient, and the board knows one recipient, `owner`. | [the desktop design](desktop-client.md) |
| What the owner says to an agent | Every word of the owner's that waits on an agent is an ask the board holds until an agent answers: a verdict with a note, a message on a card, a comment in a document, a message on a thread. It is delivered to a listening agent at once. | [TASKS](../../workflow/TASKS.md#asks) |
| A board | One for a project, in its repository's git common dir, or hosted in a PostgreSQL of the platform's. | [TASKS](../../workflow/TASKS.md#where-it-lives), [POSTGRESQL](../../operations/POSTGRESQL.md#a-hosted-board) |

Three things follow that the documents do not say in one place:

1. **The owner is whoever reaches the page.** On this machine that is anything that can open a
   connection to it, the agents among it. Over the tailnet it is any node Tailscale answers for:
   a phone under the same login, and equally a colleague's laptop in the same tailnet or a node
   shared into it.
2. **Nothing checks an agent's handle.** A session is who it says it is. That is sound on one
   machine under one account, and stops being sound the day two people's agents write to one
   board.
3. **The pieces for more than one person are there, unconnected.** Principals, grants,
   capabilities and the hosted admission were each built for their own task. No person row, no
   name on a line and no grant a page can change joins them.

## The idea, questioned

### Is there a second person?

Not on this machine: one person, their phone, their agents. There is one where the hosted work
is going. A hosted board is reached by a company's people through a proxy that signs who each
one is ([hosted ingress](../../operations/analysis/hosted-ingress.md#identity-and-the-trusted-proxy-boundary)), and the
adapter built for it can read, and write nothing, until the Symphony can record a write as a
person's. A tailnet shared with one colleague is the same case at the smallest size. So the
concept is needed for the hosted board first, and it must cost a one-person machine nothing.

### What identity cannot do on one machine

An agent on a machine runs under the account of the person who started it. It can write the
checkout, the board's database and the server's own port. No sign-in fences it: only the
operating system could, and the Symphony does not run agents under other accounts. So identity
here protects one thing, a person or a program arriving **over the network**, and the analysis
claims nothing more. An agent keeps to its role because the board's rules and its own
instructions say so, as today.

### The owner is four things

What the guides call the owner is, read closely, four things in one word:

- **Someone at the page.** Today a place, not a person.
- **The one who decides.** A card's verdict; the approve that is the `owner` stamp; cancelling,
  moving and ranking tasks.
- **The one agents answer.** Every word of the owner's that waits on an agent is an ask, held
  until one answers, and delivered at once. To speak is to set an agent to work.
- **One voice in the record.** `owner ·` on a card's line, `owner` as a thread's sender. The
  rules read that voice: only the owner approves a card, an approved card decides a later one,
  and a worktree holding an owner's line no commit has is never removed.

With several people the four come apart. Someone at the page is a **person** a provider vouches
for. Deciding belongs to the **group** a thing is owned by. Being answered by agents is a power of
its own, which a person can lack while still speaking everywhere. The voice gains a name
([what the owner becomes](#what-the-owner-becomes)).

A fifth thing is the owner's by another road and stays outside this concept: what is done in a
terminal on the machine. Restarting the shared server, a release run by hand, pointing `current`
back, holding the board's database, removing a snapshot. The machine's account decides those,
not a sign-in, and no page offers them.

### An identity provider as a plugin

The ask expects the provider to be a plugin. The Symphony's plugin is a precise thing: a
project's own code, named in its `symphony.toml`, that draws blocks for that project. A provider
cannot be that, for three reasons:

- **The file can be changed by a landing.** A provider named in the project's file is chosen by
  whoever lands a change to it, who could then be anyone. The hosted design already holds that
  trust settings never come from a document a visitor can edit.
- **One server serves many projects.** A person signs in to the server once. A provider per
  project would ask them once for each, and two projects could disagree about who they are.
  What differs from board to board is what a person holds there, never who they are.
- **A plugin answers after admission.** The provider's answer is what admits a request, before
  any project is opened.

A plugin it is all the same, of the server and not of a project: a service outside the core,
which the server's own settings name ([below](#how-a-provider-plugs-in)).

## The shape proposed

![Signing in is outside the core; what a person may do is the board's](users-and-identity/authn-authz.svg)

Two questions, kept apart, with one signed message between them.

- **Authentication: who is asking.** A sign-in service outside the core answers it. The core
  takes the service's signed word and checks it, and does nothing else about sign-in.
- **Authorization: what they may do.** The Symphony's own, kept with each board. No service
  outside is asked, and nothing a service says gives a power.

The owner's words for the line between them: "Something should authenticate it, and give symphony
the user's auth data. […] this should be some plugin that happens outside of the core […]
symphony just relies on that service to get info on who the person is. Which could be signed
header data."

- **A person** is a [profile](#a-persons-profile): what a sign-in service vouched for, the service's name and its subject for them (an
  email, an id), which stay private to the board, and a short **public name** chosen for them
  (`ana`), which is what a line in a document or on a thread carries. A subject can be handed to
  someone else later, as an email is, so a board names the service and the subject together and
  removes the person when they leave.
- **A group** is a named set of a board's people: engineering, design. It is what a card or a
  task is owned by, and what a power is given to.
- **A power** is one thing a person may do on a board, given to them or to a group they are in
  ([below](#what-the-owner-becomes)).
- **A sign-in service** tells the server who sent a request, in signed data. It gives no power.
- **The actor seam** is the one change everything else stands on: every write the server makes
  takes the actor who asked for it (a person and what they hold) where the constant `owner`
  stands today. The local host supplies the person at the machine, so its lines read as they do
  now. The hosted host supplies the visitor it verified, and its saves can then go through the
  same code as local ones, with what they hold checked first.

### How a provider plugs in

As a program of its own, outside the core: the server's plugin, which anyone writes
([its card](#identity-provider-how-added), B). The owner said how its word reaches
the core when rejecting [another card](#group-card-who-decides): "symphony just
relies on that service to get info on who the person is. Which could be signed header data." So
the program is a sign-in service that signs what it says, and the core verifies it. The card drew
B as a program the core asks on each request; signed data is the same option with less in the
core, since the core then runs and waits on nothing.

- **The service authenticates.** It sends a person to Google, GitHub or a company's own sign-in
  with OAuth 2 (OpenID Connect, the part of it that says who someone is), and keeps what that
  needs: the client secret, the exchange, the session, sign-out.
- **It hands the Symphony signed data**: who the person is (the service's name and its subject
  for them, and a display name), which server the data was made for, and until when it holds. A
  signed header on each request is the form the hosted design already read
  ([hosted ingress](../../operations/analysis/hosted-ingress.md#identity-and-the-trusted-proxy-boundary)).
- **The core verifies and nothing more.** The signature, against a key the server's own settings
  name; that the data is fresh; that it was made for this server. Unsigned, stale or made for
  another server, the request is refused. The core draws no sign-in page and holds no password
  and no client secret.
- **It is the server's plugin, never a project's.** The key and the service are named where the
  server is started, for [the reasons above](#an-identity-provider-as-a-plugin). A project's
  `symphony.toml` names neither.
- **Nothing in the signed data gives a power.** A group or a role a company's sign-in knows of
  is not read as one of the board's, and a board never fills its groups from such data: they are
  its own rows, set by its admins. That is [its card](#people-kept-where)'s B over
  its C, which had a provider's groups read as roles, and the owner's line between signing in and
  what a person may do: "Feels this is mixing authentication with authorization."
- **On a machine, Tailscale is the service.** It signed the person in to the tailnet, and the
  machine's own daemon says whose node an address is. That word comes from the daemon on the
  same machine and not as a signed header: the one place the core asks instead of verifying, as
  the gate does today. A request from the machine itself is the machine's person.

- [-] **identity-provider-plug** — You expected the identity provider to be a plugin. It should be replaceable, and it cannot be a project's plugin: a project's plugins are named in `symphony.toml`, so whoever lands a change to that file would choose who everyone is. The choice is how a provider is added. A: it is part of the server, chosen where the server is started; the Symphony ships two (Tailscale's login for a machine, a signing proxy's header for a hosted board), and draws no sign-in page. B: a provider is a program of its own that the server's settings name, as a process plugin is; anyone writes one. C: the Symphony signs people in itself, with Google or GitHub. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#How a provider plugs in]]
  [2026-11-20 06:40] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    **Base (today).** This Mac's server asks Tailscale whether an address is a machine of the tailnet. Tailscale's answer also says whose machine it is, and that part is not read. For a hosted board there is a place in the server's code where a host says who a visitor is, and no provider is shipped for it.

    **What the server would trust, under every option.** Where a request came from (this machine is its own person), and a provider's verified word. Never a name in a request, a header an unverified hop set, an email's domain or a handle someone claims.

    **A.** The provider signed the person in before the request arrived, so the Symphony keeps no password, no secret and no session. Your phone keeps working with nothing set up: a tailnet machine under this Mac's login is you. A host with another need writes its own in Rust, as it can today.

    **B.** A provider in any language, replaced without a new server. It adds a contract the Symphony must keep stable across releases, on the path every request takes, and a second process that holds secrets.

    **C** needs no proxy in front of a hosted board. It is the most code: a sign-in page, a secret, session cookies, sign-out, all of it the kind where a mistake is a break-in.

    **Why A.** It covers the two places a Symphony runs today. B's contract is A's seam with a process boundary added, so nothing built for A is lost when a third provider is in fact needed.

    **Earlier verdicts checked.** **hosted-ingress-boundary** (B) put the hosted checks in an adapter inside the server, against a separate gateway. It does not say whether the Symphony ships providers, opens the adapter to other programs or signs people in itself. **desktop-client** (A) left a hosted server's login to a later card, which is this one. **consumer-contract-versioning** (B) keeps a project's code outside the library; under A and B a host's own provider stays the host's. None decides this.
  [2026-11-21 09:49] owner · rejected — this needs some diagrams to be understood
  [2026-11-21 10:22] agent — Replaced by **identity-provider-how-added**: the same three options, each with a picture, after one of today and one of where a provider sits in a request. The pictures are in the analysis too.

- [r] **identity-provider-how-added** — Replaces: identity-provider-plug. The same choice, with pictures. An identity provider does one thing: it tells the server who is asking. What that person may do is the board's to say. You expected the provider to be a plugin; the choice is how one is added. A: providers are part of the server, and the Symphony ships two. B: a provider is a separate program the server asks, which anyone writes; it is the server's plugin, never a project's. C: the Symphony signs people in itself, with Google or GitHub. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#How a provider plugs in]]
  [2026-11-21 10:22] agent — Base: today, the server asks only whether a browser's address is this machine or a machine of the tailnet. Whose machine it is, is not read. ![Today: everyone the gate lets in writes as the one owner](users-and-identity/today.svg)

    **What a provider is, under every option.** The first box below. It says who is asking, and gives no rights: ![Proposed: first who is asking, then what they hold on this board](users-and-identity/request.svg)
  [2026-11-21 10:22] agent — Posted for the board task `app/users-and-identity`, with the pictures your rejection of **identity-provider-plug** asked for: "this needs some diagrams to be understood". A concept: nothing is built by this card. Where the provider sits under each option: ![Providers are part of the server, under A](users-and-identity/provider-a.svg) ![A provider is a program of its own, under B](users-and-identity/provider-b.svg) ![The Symphony signs people in itself, under C](users-and-identity/provider-c.svg)

    **A, part of the server.** A person is signed in before the request arrives, by Tailscale or by a company's proxy. The server only reads the result. The Symphony ships those two providers, draws no sign-in page and keeps no password, secret or session. Another provider needs a new build of the server.

    **B, a program of its own.** As A, but the reading is done by a separate program the server asks for each request. Anyone writes one, in any language, and swaps it without a new server. The Symphony must then keep that program's contract stable, on the path every request takes, and a second process holds secrets.

    **C, the Symphony signs people in itself.** Nothing in front. The server draws the sign-in page, sends the person to Google or GitHub, and keeps a session for each browser and a secret of its own. It is the most code, all of it the kind where a mistake lets someone in.

    **Why not a project's plugin, under any option.** A project's plugins are named in its `symphony.toml`. Whoever lands a change to that file would choose who everyone is.

    **Why A.** It covers the two places a Symphony runs today, with no secret or session to keep. B's contract is A's seam with a process boundary added, so nothing built for A is lost when a third provider is in fact needed.

    **Earlier verdicts checked.**
    - **people-kept-where** (B) keeps people with the board. It says what a person may do, not how the server learns who they are.
    - **hosted-ingress-boundary** (B) put the hosted checks in an adapter inside the server. It does not say whether the Symphony ships providers, opens the adapter to other programs or signs people in itself.
    - **desktop-client** (A) left a hosted server's login to a later card, which is this one.

    None decides this.
  [2026-11-21 17:27] owner · approved — B
  [2026-11-21 17:28] agent — Your note on **group-card-who-decides** answers this card too: "this should be some plugin that happens outside of the core [...] symphony just relies on that service to get info on who the person is. Which could be signed header data." That is none of the three as drawn here: not A (providers built into the server), not C (the core signs people in), and nearer B, but a service that signs what it says, not a program the core asks. So this card is replaced by **sign-in-service-shipped**, which asks only what is left open. Dismiss this one, or answer it if I read your note wrongly.
  [2026-11-21 17:29] agent — Correction to my line above, which I wrote before I saw your B of 17:27: nothing here is replaced or to be dismissed. Recorded, B: the provider is a separate program, the server's plugin, which anyone writes. One consequence, from your note on **group-card-who-decides**, "Which could be signed header data": the program signs what it says and the core verifies it, where this card drew the core asking the program on each request. It is still B, with less in the core. **sign-in-service-shipped** follows from it and asks what the Symphony ships of that program.
  [2026-11-21 17:29] agent · resolved

### What the Symphony ships for signing in

The core's check, and a sign-in service of its own
([its card](#sign-in-service-shipped), B).

![B: and a sign-in service of its own](users-and-identity/sign-in-b.svg)

- **The check and the format come first.** The core verifies a signed header, and a document
  says what the header holds and how it is signed. A hosted board behind a company's OAuth 2
  proxy needs nothing more, and is the first thing that works.
- **Then a sign-in service of the Symphony's.** A separate program, outside the core, that
  stands in front of the server, does OAuth 2 with Google or GitHub, keeps the client secret and
  the session, and signs the same header. It works once a client id is set.
- **Any other replaces it.** A service that signs the same header is taken as the Symphony's own
  is. The format is therefore a contract the Symphony keeps stable across releases.
- **It stands in front, not beside.** Every request passes through it. The core gains no
  redirect, cookie or sign-out.

What follows:

- **The service is a package of its own**, which the core does not link, as the GitHub issue
  exchange is a program no other package builds with. Its sign-in code is the Symphony's to keep
  safe, and a mistake in it is outside the core.
- **A key pair joins the two.** The service signs with a private key only it holds; the server's
  settings name the public key. Nothing in a project's `symphony.toml` names either.
- **The server must be reachable only through the service** where one is used, or a request could
  skip it. The signature is what the core trusts, so a request that skipped it carries none and
  is refused; the hosted design sets out the same boundary.

The two other ways drawn for the card, the check alone and a service beside the server, are in
[the pictures](users-and-identity/sign-in-a.svg) [kept with it](users-and-identity/sign-in-c.svg).

- [r] **sign-in-service-shipped** — Follows identity-provider-how-added, where you chose B: signing in is done by a separate program, the server's plugin, outside the Symphony's core. You added that it could use OAuth 2 and hand over signed header data, which the core only verifies. What is left to choose is what the Symphony ships of that program. A: nothing; it ships the core's check and the format, and a project puts an OAuth 2 proxy it already has in front. B: as A, and it also ships a sign-in service of its own, which any other can replace. C: the service stands beside the server, not in front; the browser signs in there once and carries a signed token. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#What the Symphony ships for signing in]]
  [2026-11-21 17:29] agent — Base: today the Symphony ships nothing for signing in. This Mac's server asks Tailscale whether an address is a machine of the tailnet, and reads nothing about whose.

    **What your words settle, under every option.** Signing in is one thing and what a person may do is another: ![Signing in is outside the core; what a person may do is the board's](users-and-identity/authn-authz.svg)
  [2026-11-21 17:29] agent — Posted for the board task `app/users-and-identity`, from your B on **identity-provider-how-added** and your note on **group-card-who-decides**: "Something should authenticate it, and give symphony the user's auth data. This could probably leverage OAuth 2 here. And this should be some plugin that happens outside of the core". A concept: nothing is built by this card. What stands outside the core under each option: ![The Symphony ships the check alone, under A](users-and-identity/sign-in-a.svg) ![The Symphony also ships a sign-in service of its own, under B](users-and-identity/sign-in-b.svg) ![The service stands beside the server, under C](users-and-identity/sign-in-c.svg)

    **A, the check alone.**
    - The Symphony ships the core's check and a document of the signed data it accepts.
    - A project puts a service it already has in front, such as the OAuth 2 proxy a company runs. That service signs a header on each request.
    - The Symphony writes no sign-in code. Nothing works for several people until someone sets a proxy up.

    **B, and a service of ours.**
    - Everything in A.
    - The Symphony also ships a sign-in service: a separate program, outside the core, that stands in front, does OAuth 2 with Google or GitHub and signs the same header.
    - It works once a client id is set. Any other service that signs the same way replaces it.
    - The sign-in code is the Symphony's to keep safe. A mistake in it is outside the core.

    **C, beside, not in front.**
    - The browser is sent to the service once, signs in, and comes back with a signed token it shows the server itself.
    - No program sits in the path of each request.
    - The core gains the redirect, a cookie and sign-out, which is part of a session.

    **Under every option.** The core holds no password and no client secret, and trusts only what a key in the server's own settings verifies. The service is the server's plugin, never a project's: a project's `symphony.toml` names neither it nor the key. On this Mac, Tailscale is the service, and a machine with one person needs none.

    **Why B.** The check and the format are built first under A and B alike, and a hosted board behind a company's proxy needs nothing more. The service of ours is what makes "some plugin" true for a project with no proxy to bring, and it is added without touching the core. C moves part of the session into the core, which is the mixing you turned down.

    **Earlier verdicts checked.**
    - **identity-provider-how-added** (B) makes the provider a separate program anyone writes. It does not say whether the Symphony ships one, or whether it stands in front or beside.
    - **hosted-ingress-boundary** (B) put the hosted checks in an adapter inside the server. That is the core's check here.
    - **people-kept-where** (B) keeps people with the board. It says what a person may do, not how they sign in.

    None decides this.
  [2026-11-21 17:31] owner · approved — B
  [2026-11-21 17:32] agent — Recorded, B. The core's check and the header's format are built first, which is all a hosted board behind a company's proxy needs; the Symphony's own sign-in service follows as a separate program in front, outside the core, replaceable by any other that signs the same header. What follows is in [[analysis/users-and-identity#What the Symphony ships for signing in]]. One cost the card did not state: the header's format becomes a contract the Symphony keeps stable across releases, since other services sign it too.
  [2026-11-21 17:32] agent · resolved

### A tailnet machine under another login

It is that login's person, with what each board gives them, and a board that has not added them
does not exist for them. The owner, on [the card that asked](#tailnet-peer-before-named):
"it should be using the auth and authorization stuff so that is not owner, but the persons role
in the board. In case they are not added to a board, they should not be able to interact with
it or see it exists at all".

- **Tailscale says who.** A machine of the tailnet reaches the server as today, and the server
  reads whose machine it is. Nobody reaching it over the tailnet is `owner` for being there.
- **Each board says what.** The person has what that board gives them: their groups and powers
  there.
- **A board that has not added them is not there.** It is in no list they are shown, and its
  pages, its documents and its threads answer them as a project the server does not hold. The
  answer does not tell a board they are not on from one that does not exist.
- **This holds for every way in**, a hosted board's sign-in service as much as Tailscale: not
  being on a board is the same as the board not being there.

What follows:

- **A machine under the Mac's own login is its person**, the owner's phone among them, with
  nothing set up. On a board with nobody named that person holds everything, as today.
- **A colleague's laptop sees nothing** until a board adds them, where today it writes as the
  owner. That is the one thing that changes on a machine with one person, and it changes when
  the login is first read.
- **A machine with no login** (a tagged node) is nobody's, and sees nothing unless a board adds
  the node itself.
- **The server's own pages** (the list of projects, the machine's releases) show a person only
  the boards they are on, and what is the machine's alone only to its person.

- [-] **tailnet-peer-before-named** — Today any machine in your tailnet writes as you, a colleague's laptop as much as your phone. Once the server reads whose machine it is, your own machines stay you. What is left to choose is what a machine under someone else's Tailscale login may do before a board has named that person. A: as today, it writes as the owner until a board names its first person. B: nothing; it is refused until a board names them. C: it reads every board and writes nothing. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#A tailnet machine under another login]]
  [2026-11-21 17:37] agent — Posted for the board task `app/users-and-identity`. On a machine Tailscale is the sign-in service (**sign-in-service-shipped**, B), so this is what its word is used for. A concept: nothing is built by this card.

    ![A tailnet machine under someone else's login](users-and-identity/tailnet-peer.svg)

    **Base (today).** The server asks Tailscale only whether an address is a machine of the tailnet. Any such machine reaches every project and writes as `owner`. Tailscale's answer also says whose machine it is, and that part is not read.

    **A, as today.**
    - A machine under another login keeps writing as the owner until a board names its first person.
    - Nothing changes for anyone until then. What is open today stays open.

    **B, nothing until named.**
    - It is refused, with a page saying whose server this is and that a board must name them.
    - A board that names that login's person lets them in, to that board.

    **C, reads until named.**
    - It reads every board the server holds and writes nothing.

    **Under every option** a machine under this Mac's own login, your phone among them, is you with nothing set up. A machine with no login (a tagged node) is treated as someone else's is. If nobody else is in your tailnet, the three are the same.

    **Why B.** A leaves a colleague's laptop able to decide your cards for as long as nobody is named, which on a one-person machine is always. C shows every project on this Mac to the whole tailnet. B changes nothing for you and your own machines, and under it being in a tailnet gives nothing by itself.

    **Earlier verdicts checked.**
    - **sign-in-service-shipped** (B) and **identity-provider-how-added** (B) say how a person is known. They do not say what a known person nobody has named may do.
    - **people-kept-where** (B) lets a board say what a named person may do. It says nothing of a person no board names.

    None decides this.
  [2026-11-21 17:39] owner · rejected — A, but it should be using the auth and authorization stuff so that is not owner, but the persons role in the board. In case they are not added to a board, they should not be able to interact with it or see it exists at all
  [2026-11-21 17:40] agent — Applied as you said, with no new card: "it should be using the auth and authorization stuff so that is not owner, but the persons role in the board. In case they are not added to a board, they should not be able to interact with it or see it exists at all". A tailnet machine is its login's person, with what each board gives them; a board that has not added them answers as a project the server does not hold, and is in no list they see. Your own machines stay you. One consequence: a colleague's laptop, which writes as you today, sees nothing from the day the login is read until a board adds them. Written in [[analysis/users-and-identity#A tailnet machine under another login]].

### Where public names are declared

The short name is one field of something larger. The owner, rejecting
[the card that asked this](#public-name-where-declared): "We will probably have a
mapping of names / emails / git handles / communication usernames and preferences (for example
slack user). So this may be become more complex then proposed". So the question is not where a
list of names is declared, but where a person's whole record is kept
([below](#a-persons-profile)).

- [-] **public-name-where-declared** — You said a person needs an id outside the database, to sign their comments and decisions. That is a short public name (`ana`), written on lines that are files in git. Something must say that `ana` is a name and whose. A: a committed file of the project lists the names and nothing else; each board keeps privately which sign-in a name belongs to. B: each board keeps its own names and nothing is committed. C: the name is taken from the sign-in itself, such as the part of an email before the @. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Where public names are declared]]
  [2026-11-21 17:32] agent — Posted for the board task `app/users-and-identity`, from your note on **people-kept-where**: "There should however be some 'non db id' in use to identify people in their comments and interactions (who decided things for example). So there will be some information that isn't the the DB only." A concept: nothing is built by this card.

    ![Where a person's public name is declared](users-and-identity/public-name.svg)

    **Base (today).** One name, `owner`, fixed in the code.

    **A, a file of the project.**
    - A committed list of names and nothing else: no sign-in, no group, nothing a person may do.
    - Each board keeps, privately, which sign-in a name belongs to and what that person may do there.
    - Every board and every clone reads one list. A retired name stays in it, so it is never given again.
    - A new name is a landing. Adding a person from a People page would file it, and the person reads meanwhile.

    **B, each board's own.**
    - A board keeps its names with its people. Nothing is committed.
    - The least to build, and enough while a project has one board.
    - With two boards, each can give `ana` to a different person, and a line signed `ana` may be either.

    **C, taken from the sign-in.**
    - The name is made from what the sign-in service says. Nothing is declared.
    - Part of a sign-in is then committed. Two people can come out as one name, an address handed to someone new gives them an old name, and a change of service renames everybody.

    **Under every option** a name once used is never given to another, the lines already written as `owner` stay, and a machine with nobody named keeps writing as `owner`.

    **Why A.** The record is the project's, so the list of who may be named in it is the project's too. A name gives no power, so landing one lets nobody in: the board still decides what `ana` may do. You spoke of several boards for one project, which is where B breaks.

    **Earlier verdicts checked.**
    - **people-kept-where** (B) keeps people, groups and what each may do with the board and in no repository, and your note adds that some id is not in the database only. It does not say where that id is declared. A commits a name alone, none of what B keeps out of a repository.
    - **sign-in-service-shipped** (B) says how a person signs in, not what they are called in the record.

    Neither decides this.
  [2026-11-21 17:37] owner · rejected — We will probably have a mapping of names / emails / git handles / communication usernames and preferences (for example slack user). So this may be become more complex then proposed
  [2026-11-21 17:39] agent — Redone from your note: a person has a profile (names, emails, git handles, chat usernames such as Slack, preferences), and the short name on their lines is one field of it. The analysis lists what each field is for and who may see it: [[analysis/users-and-identity#A person's profile]]. Replaced by **person-profile-where**, which asks what your words leave open: where the profile is kept and who writes it.

### A person's profile

A person's profile is what the Symphony knows of who they are, apart from what they may do:

| Field | What it is for | Who may see it |
| --- | --- | --- |
| short name (`ana`) | the name on every line they write, on a card, in a comment, on a thread | everyone; it is in git |
| display name | how pages show them | everyone on the board |
| sign-ins | which sign-in service's subject is this person; there can be several | the board's admins |
| emails | a notice by mail; matching a commit's author to a person | the person and admins |
| git handles | whose commit a save of theirs is authored as; whose commits are theirs | everyone; they are in git already |
| chat addresses (a Slack user, a WhatsApp number) | where a notice is sent | the person and admins |
| preferences | which notices, by which way | the person |

What a person may do (their groups, their powers) is not in the profile. It is each board's
([its card](#people-kept-where)).

Kept once, with the server, and written by the person
([its card](#person-profile-where), B).

- **The server keeps one profile for each person**: in the machine folder on a machine, in the
  platform's own store on a hosted board. The person at the machine writes any profile. A person
  writes their own display name and git handles. A board's admin makes a profile only for
  someone who has none, with no sign-in: every sign-in is the person at the machine's alone to
  give, as a sign-in says who a visitor is on every board of the machine holding the name, now or
  later, so one an admin gave could take a name another board holds or will
  ([TASKS](../../workflow/TASKS.md#the-people-page)).
- **A board keeps only that person's groups and powers**, and refers to the profile. Who someone
  is, is kept apart from what they may do, as signing in is.
- **A line carries the short name and nothing else.** A name once used is never given to
  another. Emails and chat addresses are in no repository, and no agent or project plugin is
  handed them.
- **The lines already written as `owner` stay**, and the person at the machine writes as `owner`
  until they give themselves a short name ([decided](#how-a-persons-name-is-written)).

What follows:

- **The server now keeps people**, where the first round had it keep none: who they are, never
  what they may do. Being in the server's list gives nothing, as a board that has not added a
  person is not there for them.
- **A short name is unique on one server.** A project served from two servers can give `ana` to
  two people. The option not taken, the public part of a profile as a committed file of the
  project, answers that, and can be added when a project is in fact served from two places.
- **A person's git handles are how a save of theirs is authored**, and how a commit made outside
  the Symphony is read as theirs.

- [r] **person-profile-where** — Replaces: public-name-where-declared. Your words are the base now: a person has a profile (names, emails, git handles, chat usernames such as Slack, preferences), of which the short name on their lines is one field. What is left to choose is where that profile is kept and who writes it. A: with each board, beside what the person may do there; its admin writes it. B: once, with the server, written by the person; each board keeps only their groups and what they may do. C: as B, and the part that is public anyway (short name, display name, git handles) is also a committed file of the project. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#A person's profile]]
  [2026-11-21 17:39] agent — Posted for the board task `app/users-and-identity`, from your note on **public-name-where-declared**: "We will probably have a mapping of names / emails / git handles / communication usernames and preferences (for example slack user)." A concept: nothing is built by this card.

    ![Where a person's profile is kept](users-and-identity/profile-where.svg)

    **Base (today).** No person is recorded. The one name is `owner`, fixed in the code.

    **What a profile holds, under every option.**
    - **Short name** (`ana`): the name on every line they write. It is in git, so everyone sees it.
    - **Display name**: how pages show them.
    - **Sign-ins**: which sign-in is this person. There can be several.
    - **Emails**: a notice by mail, and matching a commit's author to a person.
    - **Git handles**: whose commit a save of theirs is authored as.
    - **Chat usernames** (Slack): where a notice is sent.
    - **Preferences**: which notices, by which way.

    What a person may do is not in it. That stays each board's, as you decided.

    **A, with each board.**
    - A board keeps a whole profile beside each person's groups and what they may do. One store, one page.
    - A person on three boards has three profiles. A Slack name changed is changed on each.
    - The board's admin writes it.

    **B, once, with the server.**
    - The server keeps one profile for each person. The person writes their own, and an admin can.
    - A board keeps only their groups and what they may do, and refers to the profile.
    - Who someone is, is kept apart from what they may do, as signing in is.

    **C, and its public part in git.**
    - Everything in B.
    - The short name, the display name and the git handles are also a committed file of the project, so a clone reads whose lines are whose with no server.
    - A new person is a landing as well as a row.

    **Under every option** a line carries the short name and nothing else, emails and chat usernames are in no repository, and no agent or plugin is handed them.

    **Why B.** A profile is the person's, and they are one person on every board the server holds. What B leaves open is C's reason: a project served from two servers can give `ana` to two people. C's file can be added when a project is in fact served from two places, with nothing in B lost.

    **Earlier verdicts checked.**
    - **people-kept-where** (B), with your note: "This is a board specific setting. There could even be multiple boards for the same project where people have different permissions." That keeps what a person may do with each board. Your note then says some id is not in the database only; it does not say where the rest of a person's record is kept, which is this card.
    - **sign-in-service-shipped** (B) says how a person signs in. A profile names their sign-ins and is not one.

    Neither decides this.
  [2026-11-21 17:40] owner · approved — B
  [2026-11-21 17:41] agent — Recorded, B: one profile for each person, kept with the server and written by them; a board keeps only their groups and what they may do. Written in [[analysis/users-and-identity#A person's profile]]. One cost the card stated and you accepted with B: a short name is unique on one server, so a project served from two could give one name to two people; the committed file of C can be added if that comes.
  [2026-11-21 17:41] agent · resolved

### Where people are kept

What a person may do is kept with the board, and in no repository
([its card](#people-kept-where)). Who they are is their profile's, above.

- **The list is a board's own.** Its people, their groups and their powers are rows of that
  board, beside its tasks: in the git common dir's database on a machine, in the hosted
  database on a hosted board. An admin of the board changes them from a People page or a
  command, and the board keeps each change in a log.
- **Two boards differ.** A person can decide on one board and only read another, of the same
  project or not.
- **A clone holds nobody's sign-in.** A service's subject for a person never leaves the board.
- **The short name is the part that is in git.** It is written where the record is: on a card's
  line and in a comment, which are files, and on a thread. So it outlives the board's row for
  the person.
- **With nobody named** on a board, the person at the machine holds every power there, as today.
  The first person named is the first moment anything differs.

- [r] **people-kept-where** — With several people, the Symphony must keep a list of who they are and what each may do, and someone must be able to change it. Where it is kept decides who that someone is. A: in the project's `symphony.toml`, committed; a change is a landing. B: with the server and in no repository (the machine folder here, a hosted board's protected store); an admin changes it from a People page. C: in the identity provider; its groups are read as roles. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Where people are kept]]
  [2026-11-20 06:40] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    **Base (today).** No person is recorded anywhere. The nearest things are the server's list of projects, in the machine folder, and a workflow authority's public key, in the project file.

    **A.** The list is reviewed, in git's history, and the same on every machine. But each person's sign-in name (an email) is committed, in a repository that may be public. Whoever can land a change to the file can make themselves an owner. Adding a person is a landing.

    **B.** A clone holds nobody's sign-in: a document carries only the short name chosen for a person (`ana`). Each change is kept in a log of its own, since git no longer keeps it. Each server has its own list. With no list, the person at the machine is its admin and the owner of every project, as today.

    **C.** The Symphony keeps no list. But which group means which role must still be written down under A or B, each provider says groups its own way, and Tailscale, the first provider proposed, gives a login and no group.

    **Why B.** Who may enter is asked before any project is opened, so it is the server's to keep, and one server serves many projects. C can come later as one more way to fill B's list.

    **Earlier verdicts checked.** **hosted-ingress-boundary** (B) has the hosted adapter pass permissions to the handlers, and its analysis says trust settings never come from a document a visitor can edit. That is the reasoning behind B here, and it was said of a hosted board alone: it does not decide where a machine's server, or the product, keeps people. **revision-bound-project-guards** (B) commits a project's authority key in its file. That is a key that checks a signature, not a list of people. Neither decides this.
  [2026-11-21 09:48] owner · approved — B. This is a board specific setting. There could even be multiple boards for the same project where people have different permissions. There should however be some "non db id" in use to identify people in their comments and interactions (who decided things for example). So there will be some information that isn't the the DB only.
  [2026-11-21 10:22] agent — Recorded, B as you narrowed it: "This is a board specific setting. There could even be multiple boards for the same project where people have different permissions." The list is each board's own (its people, their groups and what each may do, kept with that board's tasks), not one list for the server, so two boards can differ. "There should however be some 'non db id' in use to identify people": that is the short public name (ana), written on a card's line, in a comment and on a thread, so it is in git and outlives the board's row. It follows that one name means one person across a project's boards and is never given to another. Where a project declares its names is a later card. The analysis says all this under Where people are kept.
  [2026-11-21 17:29] agent · resolved

### What the owner becomes

![Speaking, deciding and directing agents are three separate things](users-and-identity/people-and-agents.svg)

All of this section is authorization: the board's own rows, which no sign-in service sets or
reads.

**A group owns what waits to be decided.** Each card and each task is owned by a group, or by one
person. Its members are told when it is posted, it sits in their queue, and they are the ones
who resolve it. Where a rule says "the owner" of a card or a task, it reads as "whoever owns it":
a member of the owning group. A project that sets nothing up has one group with one person in
it, and every rule reads as it does now.

**What a person may do is said in separate powers**, each given to a group or to a person:

| Power | What it lets a person do | Who has it today |
| --- | --- | --- |
| read | see the board, its documents and its threads | everyone the gate admits |
| speak | write to people anywhere: a comment in a document, a message on a card, a message on a task's thread; file a task | the owner |
| decide | resolve what their group owns: a card's verdict, the approve that is the `owner` stamp, a task's move, priority and cancel | the owner |
| direct agents | have their words reach the board's shared agents and start work, though those agents are not theirs | the owner |
| administer | say who is on the board, in which groups and with what powers | whoever holds the machine |

Two things follow from holding them apart:

- **People talk everywhere.** Anyone who may speak writes under anything on the board, and the
  people it concerns are told. Speaking never starts an agent by itself.
- **Directing agents is its own power.** Someone can comment on every task and set no agent to
  work. Someone else can set the board's agents to work without those agents running on their
  credits or their machine.

An approval that must be several people's (two of three, a named sign-off) is not a power here: a
workflow guard is that, kept in the project's own ledger
([its card](../../workflow/analysis/revision-bound-transition-guards.md#revision-bound-project-guards)).

- [-] **owner-with-several-people** — Once several people can be on a board, the rules that say "the owner" need to say who that is: whose approve is the owner stamp, and who decides a card. A: one owner for a project, as now; other people join as members who read, comment, ask and file tasks, and never decide. B: owner is a role a few named people hold; any one of them decides, and their line says who. C: owners by area; each card or task names who decides it. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#What the owner becomes]]
  [2026-11-20 06:40] agent — Posted for the board task `app/users-and-identity`, from your ask: "an admin/user system so that multiple users could be on the board by leveraging some IdP (which itself would likely be a plugin too)". A concept: nothing is built by this card.

    **Base (today).** There is one owner and no person. Every browser the server admits writes as `owner`: this Mac, your phone, and equally any other machine in the tailnet. Nobody's name is on a line.

    **A** keeps every rule word for word. A member's lines carry their name; yours need none.

    **B.** A line reads `owner ana · approved`. A later word from any holder overrides an earlier one, as your own later word does today, and both stay in the card's history. "Waiting on you" is one queue for the holders, and each keeps their own Later. A project with one holder is A.

    **C** needs an area on every card, a rule for a card with none, and a queue for each person.

    **Not offered:** an approval that needs several people (two of three). A workflow guard already is that, from the project's own ledger.

    **Why B.** It is A for a project that wants A, and a second decider can be added later without changing the rules again. Its cost over A is the name on your lines and the rule for two holders who disagree. Nothing asks for C yet, and it can be added over B as a way of sorting B's one queue.

    **Under every option** a machine with nobody else named changes in nothing: the person at it is the owner of every project it serves.

    **Earlier verdicts checked.**
    - **revision-bound-project-guards** (B) keeps approvals by several people in a project's own ledger, which is why none is offered here. It does not say who the owner of a card is.
    - **needed-stamps** makes the owner stamp "the owner's approve naming the tip", with one owner. It does not say whose approve that is once there are several.
    - **hosted-ingress-boundary** (B) passes a verified visitor and their permissions to the server's handlers. It names no role.

    None of them decides this.
  [2026-11-21 09:49] owner · rejected — Added a comment in the analysis page that better describes the vision, use it to understand or reformulate this decision.
  [2026-11-21 10:22] agent — Replaced by **group-card-who-decides**. The analysis is rewritten from your comment there: a group, or one person, owns a card or a task and its members are told; speaking, deciding and directing agents are separate settings; agents are personal or shared. What your comment leaves open is two cards: **group-card-who-decides** (whose answer settles a card its group owns) and **words-without-agent-reach** (what an agent sees of the words of someone who may not direct agents).

### Who in a group decides

Any one member ([its card](#group-answer-settles), A). This is authorization
alone: it concerns people the board already knows, however they signed in.

- **Every member is told** when a card their group owns is posted, and has it in their queue.
- **The first answer decides**, under that member's name. The card then leaves the others'
  queues.
- **A later word from any member overrides it**, as the owner's own later word does today. Both
  stay in the card's history, each with its name.
- **A member can take a card.** That shows the others who is on it. It locks nothing: another
  member can still answer.

What follows for the rules that say "the owner":

- **The `owner` stamp** on a task is the approve of any one member of the group the task belongs
  to who holds decide, naming the tip, under that member's name, or of the owner, on a card and
  on a thread at once ([TASKS](../../workflow/TASKS.md#people)): a named person's
  `ana · approved` meets it while she holds decide and the task is her group's, and her later
  word that is no approve, or anyone's, keeps an approve from standing.
- **An approved card decides a later one** whoever of its group approved it. An agent checking
  earlier verdicts reads the verdict, not who gave it.
- **A line no commit has yet** keeps its worktree from being removed, whoever's line it is.
- **Two members who disagree** override each other in turn, and nothing stops them but the
  history showing it. A decision that must be several people's stays a workflow guard's
  ([its card](../../workflow/analysis/revision-bound-transition-guards.md#revision-bound-project-guards)); one that must be one person's is
  a thing owned by that person, not by a group.
- **An agent already acting** on an answer when another member overrides it is where it is today
  when the owner changes their mind: the later word is a new ask.

A group of one is a single owner, so a machine with one person sees none of this.

- [-] **group-card-who-decides** — Replaces: owner-with-several-people. Your comment is the base now: a card or a task is owned by a group (engineering, design) or by one person, and its members are told. What is left to choose is whose answer settles a card its group owns. A: any one member; the first answer decides and a later one from any member overrides it. B: the one member it is assigned to; the others comment. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Who in a group decides]]
  [2026-11-21 10:22] agent — Posted for the board task `app/users-and-identity`, from your comment on the analysis: "C, but it could be 'group owned'. […] there can be an engineering group -> people there would get 'notified' and assigned to resolve." A concept: nothing is built by this card.

    ![Speaking, deciding and directing agents are three separate things](users-and-identity/people-and-agents.svg)

    **Base (today).** One owner. Whatever you answer decides, and a later word of yours overrides an earlier one.

    **A, any one member.**
    - Every member is told and has the card in their queue.
    - The first answer decides, under that member's name, and the card leaves the others' queues.
    - A later word from any member overrides it, as your own later word does today. Both stay in the card's history.
    - A member can take a card. That shows the others who is on it, and locks nothing.

    **B, the one member it is assigned to.**
    - The card goes to one member: whoever takes it first, or the next in turn.
    - Only that member's answer decides. The others comment.
    - If that member is away, the card waits until someone hands it to another.

    A group of one is the same under both, so this machine sees neither.

    **Why A.** Nothing waits on a person who is away, and no rule is needed for handing a card on. B puts one accountable name on a decision before it is made; under A the name is there once it is made.

    **I read "notified and assigned to resolve" two ways**, which is why this is asked: the group is assigned and anyone in it resolves (A), or one member is (B).

    **Earlier verdicts checked.**
    - **people-kept-where** (B) says where people are kept, per board. It does not say who in a group decides.
    - **revision-bound-project-guards** (B) covers an approval that must be several people's. This card is about one answer being enough, and whose.

    Neither decides this.
  [2026-11-21 17:22] owner · approved — A
  [2026-11-21 17:25] owner · rejected — Feels this is mixing authentication with authorization. Something should authenticate it, and give symphony the user’s auth data. This could probably leverage OAuth 2 here. And this should be some plugin that happens outside of the core of symphony’s symphony just relies on that service to get info on who the person is. Which could be signed header data.
  [2026-11-21 17:28] agent — The concept is redone on your line: "Something should authenticate it, and give symphony the user's auth data." Signing in is a service outside the core, with OAuth 2, whose signed data the core only verifies; what a person may do is the Symphony's own, kept with the board, and no service sets it. The analysis opens on that now, with a picture: [[analysis/users-and-identity#The shape proposed]]. Two cards follow. **sign-in-service-shipped** asks what your words leave open about the service. **group-answer-settles** asks this card's choice again as authorization alone, since your note says nothing of whose answer settles a card and your A of 17:22 went with the rejection.

- [r] **group-answer-settles** — Replaces: group-card-who-decides. The same choice, as authorization alone: it is about people the board already knows, however they signed in, and no sign-in service has a part in it. A card or a task is owned by a group (engineering, design) or by one person, and its members are told. What is left to choose is whose answer settles a card its group owns. A: any one member; the first answer decides and a later one from any member overrides it. B: the one member it is assigned to; the others comment. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Who in a group decides]]
  [2026-11-21 17:29] agent — Posted for the board task `app/users-and-identity`. You approved A on **group-card-who-decides** at 17:22 and rejected the card at 17:25 for mixing authentication with authorization. Your note says where signing in belongs and nothing about whose answer settles a card, so A is not carried and the choice is asked again. A concept: nothing is built by this card.

    ![Speaking, deciding and directing agents are three separate things](users-and-identity/people-and-agents.svg)

    **Base (today).** One owner. Whatever you answer decides, and a later word of yours overrides an earlier one.

    **A, any one member.**
    - Every member is told and has the card in their queue.
    - The first answer decides, under that member's name, and the card leaves the others' queues.
    - A later word from any member overrides it, as your own later word does today. Both stay in the card's history.
    - A member can take a card. That shows the others who is on it, and locks nothing.

    **B, the one member it is assigned to.**
    - The card goes to one member: whoever takes it first, or the next in turn.
    - Only that member's answer decides. The others comment.
    - If that member is away, the card waits until someone hands it to another.

    A group of one is the same under both, so this machine sees neither.

    **Why A.** Nothing waits on a person who is away, and no rule is needed for handing a card on. B puts one accountable name on a decision before it is made; under A the name is there once it is made.

    **Earlier verdicts checked.**
    - **people-kept-where** (B) says where people are kept, per board. It does not say who in a group decides.
    - **revision-bound-project-guards** (B) covers an approval that must be several people's. This card is about one answer being enough, and whose.

    Neither decides this.
  [2026-11-21 17:32] owner · approved — A
  [2026-11-21 17:32] agent — Recorded, A: any one member's answer settles what their group owns, a later word from any member overrides it, and both stay in the history. What it means for the rules that say "the owner" (the owner stamp, an approved card deciding a later one) is in [[analysis/users-and-identity#Who in a group decides]].
  [2026-11-21 17:32] agent · resolved

### Which group a thing belongs to

The place it is in, unless a name is written on it
([its card](#owning-group-how-named), C).

![Which group owns a card or a task](users-and-identity/owning-group.svg)

- **The board keeps a map of places to groups**: a folder of documents, a task's area. A card or
  a task with nothing written on it belongs to the group its place maps to.
- **A name written on it overrides the map**: a group's, or one person's. The card shows which
  of the two gave its group.
- **A place the map does not name** goes to the board's default group, which on a board with
  nobody named is the person at the machine.
- **Any member can hand a thing to another group**, and the board records who did.

What follows:

- **The map is the board's own**, as its groups are, so two boards of one project can send the
  same folder to different groups. Its admins keep it ([below](#who-changes-settings)).
- **A name written on a card is in the file**, so a group's name is public as a person's is. A
  board with no group of that name treats the card as unnamed and falls back to the map.
- **An agent posting a card writes nothing** in the usual case, and names a group only when the
  question is not its document's.

- [r] **owning-group-how-named** — A card or a task is owned by a group, as you said. Something must say which group. A: whoever posts it writes the group on it. B: the board reads it from where the thing is (a card's document folder, a task's area), and nothing is written. C: the place gives the group, and a name written on the card overrides it. Recommended: C. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Which group a thing belongs to]]
  [2026-11-21 17:32] agent — Posted for the board task `app/users-and-identity`, from your comment on the analysis: "there can be an engineering group -> people there would get 'notified' and assigned to resolve." A concept: nothing is built by this card.

    ![Which group owns a card or a task](users-and-identity/owning-group.svg)

    **Base (today).** A card belongs to nobody but you. It has a document, whose first folder under `docs/` already sorts your queue (design, engineering). A task has a name whose first part is its area (`app/`, `tools/`). Neither says who decides it.

    **A, written on it.**
    - Whoever posts a card or files a task names its group, or its one person.
    - One with none goes to the board's default group.

    **B, read from where it is.**
    - The board keeps a map of places to groups: a folder of documents, a task's area.
    - Nothing is written on a card. Changing the map moves everything there at once.

    **C, the place, unless named.**
    - The map gives the group.
    - A name written on the card or the task overrides it, and the card shows which of the two gave its group.

    **Under every option** any member can hand a thing to another group, and the board records who did. With one group on a board the three are the same.

    **Why C.** Agents post most cards. Under A each must choose a group every time, and one that chooses wrong sends the card to people who cannot answer it. Under B a design question raised in an engineering document goes to engineering with no way to say otherwise. C needs nothing written in the usual case and has a way out for the other. Its cost is two rules where A and B have one.

    **Earlier verdicts checked.**
    - **people-kept-where** (B) keeps a board's groups with the board. It does not say which group a card belongs to.
    - **group-card-who-decides** was rejected; **group-answer-settles** asks who in a group decides, not which group.

    Neither decides this.
  [2026-11-21 17:36] owner · approved — C
  [2026-11-21 17:37] agent — Recorded, C: the board's map of places gives a card or a task its group, and a name written on it overrides the map. What follows is in [[analysis/users-and-identity#Which group a thing belongs to]]. One consequence the card did not state: a place the map does not name goes to the board's default group.
  [2026-11-21 17:37] agent · resolved

### Who acts on a task

It is part of deciding ([its card](#task-acts-who), A).

![Who may file, move, rank and cancel a task](users-and-identity/task-acts.svg)

- **The members of the group that owns a task** move it, set its priority and cancel it, as they
  approve it. Any one of them does it, and a later word from another overrides it, as with a card
  ([above](#who-in-a-group-decides)).
- **Anyone who may speak files a task.** It belongs to the group its place maps to, or to the one
  written on it ([above](#which-group-a-thing-belongs-to)). Filing starts no agent.
- **Starting an agent on a task** needs the power to direct agents, whoever owns the task.
- **Agents keep their roles.** What an agent does by its role (a claim, a request for review, a
  land on its stamps) is not a person's act and stays as it is. What an agent does on a person's
  word, it does only where that person may.

What follows:

- **There is no sixth power.** The five of [What the owner becomes](#what-the-owner-becomes)
  cover a task: speaking files it, deciding moves, prioritizes, cancels and approves it, and
  directing agents starts work on it.
- **A move, a priority or a cancel is a record agents read**, as a verdict is. A member who may
  not direct agents moves agents by it, as they do by deciding
  ([above](#words-without-agent-reach)).
- **Who files a task and is not in its group** cannot move or cancel it after. They speak on its
  thread as anyone does.
- **Priority is one scale for the board**, p0 to p4 as today. Each group sets its own tasks' on
  it, so two groups' p0 tasks are equal, and agents take them as they do now. Whether a board
  weighs one group's work over another's is [left for later](#later-rounds).
- **On a board with nobody named**, the person at the machine is its one group and acts on every
  task, as today.

- [r] **task-acts-who** — You file, move, rank and cancel tasks today. With several people, something must say who does. A: it is part of deciding; the members of the group that owns a task move, rank and cancel it, and anyone who may speak files one. B: it is a power of its own, to manage tasks, given apart from deciding. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Who acts on a task]]
  [2026-11-21 17:41] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    ![Who may file, move, rank and cancel a task](users-and-identity/task-acts.svg)

    **Base (today).** You file a task from the page, move it, rank it and cancel it. Agents act on tasks by the roles they hold there.

    **A, part of deciding.**
    - The members of the group that owns a task move, rank and cancel it, as they approve it.
    - Anyone who may speak files a task. It goes to the group its place gives.

    **B, a power of its own.**
    - A sixth power, to manage tasks. Whoever holds it files, moves, ranks and cancels any task on the board.
    - The owning group still decides its cards and approves its tasks.
    - A person can run a board's tasks without deciding anything on it.

    **Under both** starting an agent on a task needs the power to direct agents, and approving a task is deciding.

    **Why A.** A task's order and its end are decisions about the work, and the group that owns the work makes them. B fits a team with someone who plans and does not decide. It costs a sixth power to give, and two people who can each say what is next.

    **Earlier verdicts checked.**
    - **group-answer-settles** (A) says any one member settles what their group owns. It was asked of a card's answer, not of moving or cancelling a task.
    - **owning-group-how-named** (C) says which group a task belongs to, not what its members may do to it.

    Neither decides this.
  [2026-11-22 12:44] owner · approved — A
  [2026-11-22 12:49] agent — Recorded, A: a task's acts are part of deciding. The members of the group that owns a task move it, set its priority and cancel it. Anyone who may speak files one, into the group its place gives. Starting an agent on it still needs the power to direct agents. Written in [[analysis/users-and-identity#Who acts on a task]]. What follows from A: whoever files a task outside its group cannot move or cancel it afterwards. Priority stays one p0 to p4 scale for the whole board, so two groups' p0 tasks are equal. Whether a board weighs one group's work over another's is listed for a later round.
  [2026-11-22 12:49] agent · resolved

### Words without agent reach

Nothing reaches an agent until someone passes it on
([its card](#words-without-agent-reach), A). This is about a person who may speak
and may not direct agents. A person who may direct agents writes as the owner does today.

- **Their words are for people.** They show on the page to everyone who may read there, and the
  people concerned are told.
- **The board keeps them.** A comment of theirs is not written into the document's file, and
  their message on a task's thread is a row agents are not shown. No ask is opened, so no agent
  is handed them.
- **Passing on** is one press by someone who may direct agents. From then the words are treated
  as that person's own are: written into the file or shown on the thread, and an ask an agent
  answers. The line carries both names, whose words they are and who passed them on.

- [x] **held-callout-block-changed** — A held comment is written after the block it was left on when it is passed on. Once that block's words change, or the block is gone, every Pass on is refused and nobody can withdraw the comment, so it stays on the page for good. What should happen to it? A: its author may withdraw it. B: whoever passes it on places it at the end of its section. C: it lapses and leaves the page. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Words without agent reach]]
  [2026-11-25 01:19] agent — Posted for the board task `app/held-on-documents-follow-ups`, a question the pass leaves open once a document moves. Nothing that depends on it is built.

    **Base (today).** A pass finds the block again by its words: under its own heading, else anywhere it stands once. Where they changed or are gone, the pass is refused with "the block this comment was left after has changed or is gone: nothing was written". The comment stays held, drawn where its block was, with a Pass on that is refused every time.

    **A, its author withdraws it.**
    - Its author sees Withdraw on it, and the board marks it withdrawn. It leaves the page and is told of no more.
    - She may write it again where it belongs now.
    - Until she does, it waits, and so does whoever was told of it.

    **B, placed at its section's end.**
    - Whoever may pass it on sees Pass on at the end of its section. That puts it after the last block under the heading it was left under, or at the document's end where that heading is gone, under both names as any pass is.
    - The person deciding whether an agent reads it also decides that it still applies.

    **C, it lapses.**
    - Once its block is not found, it leaves the page and is told of no more. The board keeps the row.
    - Nobody is asked, and its author is not told.

    **Why B.** The words still say what she meant, and someone who may pass them on is the one told of them. B keeps them and needs nobody to come back. A waits on its author, who may never return, and C loses words without telling anyone.

    **Earlier verdicts checked.**
    - **words-without-agent-reach** (A) says a pass writes the words into the file. It does not say where once their block is gone.
    - **held-on-document-waits-where** (open) asks where a held comment waits. It does not ask what happens once its block is gone.

    Neither decides this.
  [2026-11-26 08:08] owner · approved — B

What follows:

- **The board holds two kinds of words** where it held one: those in the record, which every
  clone and agent reads, and those held for people. A held comment needs a place on the page
  that is not a line of the file, tied to the block it was left on.
- **A clone shows held words only after they are passed on.** Until then they are the board's
  alone, and a board that is lost loses them.
- **An agent reading a thread** is shown that some messages are held and not what they say, so it
  never answers as though the thread were complete.
- **The promise has a limit.** On a machine an agent can open the board's database by itself, as
  it can open anything of its person's. What is promised is that the Symphony hands such words
  to no agent, on a page, a thread or a callback.
- **Deciding is not speaking.** A verdict is the record itself: it is written on its card in the
  file, where every agent reads it. So a person who decides and may not direct agents still
  moves agents by what they decide, and the note beside their verdict is written with it
  ([below](#the-note-beside-a-verdict)).

- [r] **words-without-agent-reach** — You said speaking and directing agents are different settings: someone may comment anywhere without starting an agent. What is left to choose is whether an agent ever sees the words of such a person. A: no, until someone who may direct agents passes them on; until then people see them and no agent does. B: yes; they start no agent, but an agent that works there later reads them with everything else. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Words without agent reach]]
  [2026-11-21 10:22] agent — Posted for the board task `app/users-and-identity`, from your comment on the analysis: "there could be a role that allows people to comment in any place they want, including the board tasks, but that doesn't automatically trigger an execution from an agent." A concept: nothing is built by this card.

    **Base (today).** Everything you write that waits on an answer is handed to an agent at once: a comment in a document, a note on a card, a message on a task. A comment is also written into the document's file, where any agent working there reads it.

    **A, nothing until someone passes them on.**
    - Their words show on the page to everyone, and the people concerned are told.
    - The board keeps them. They are not written into the document's file, and no agent is handed them.
    - Someone who may direct agents passes a comment on with one press. From then it is treated as yours are today, under both names.

    **B, everything, but it wakes nobody.**
    - Their words are saved as yours are today, with their name.
    - No agent is started for them.
    - An agent working there later, for someone else, reads them. Its instructions tell it to act only on the words of people who may direct agents.

    **What each costs.**
    - **A:** comments live in a second place until they are passed on, so a clone shows them only afterwards.
    - **B:** someone without the setting can still steer an agent, by writing an instruction where one will read it. The setting then decides when an agent starts, not whose words it follows.

    **Why A.** On a board with guests, B leaves it to each agent to ignore what it has read. A makes it the board's rule.

    **A's limit.** On this machine an agent can open the board's database by itself, as it can open anything of yours. A promises only that the Symphony hands such words to no agent.

    **Earlier verdicts checked.**
    - **owner-asks-place** (A) says where your open asks are shown. It was decided for one owner and says nothing of a person whose words are not an ask.
    - **hosted-ingress-boundary** (B) passes a visitor's permissions to the server's handlers. It names no such permission.

    Neither decides this.
  [2026-11-21 17:22] owner · approved — A
  [2026-11-21 17:28] agent — Recorded, A. The words of someone who may speak and may not direct agents are kept by the board and shown to people; no agent is handed them until someone who may direct agents passes them on. What follows is written out in [[analysis/users-and-identity#Words without agent reach]]. One consequence the card did not state: a verdict is written on its card in the file, where agents read it, so a person who decides still moves agents by what they decide. Whether the note beside such a verdict is held is listed for a later round.
  [2026-11-21 17:29] agent · resolved

- [x] **held-on-document-agent-told** — A comment held on a document is in no file and on no thread, so an agent working there cannot tell it exists. Should it be told? A: no; an agent sees nothing of it until it is passed on. B: when an agent is handed that card's or that document's ask, it is told how many comments are held there, never what they say. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Words without agent reach]]
  [2026-11-24 21:40] agent — Posted for the board task `app/held-words-on-documents`, a question your A on **words-without-agent-reach** leaves open for documents. Nothing that depends on it is built.

    **Base (today).** A held message on a task's thread leaves a line agents read, `a message is held for people: #4`, so an agent never answers as though the thread were complete. A document has no thread, and a held comment is not written into its file.

    **A, nothing.**
    - The file is what agents read, and it holds only words that were passed on.
    - An agent answering a card may answer it without the held message on it, until someone passes that message on.

    **B, a count.**
    - When an agent is handed a card's ask or a document's comment, the callback adds `2 held here`.
    - It still cannot read them. It can say it answered without them.
    - Nothing is written into the file, so an agent reading the file alone is told nothing.

    **Why A.** B tells only an agent handed a callback, and a count it cannot read gives it nothing to act on. Whoever may pass a comment on sees it beside the card and decides whether an agent should read it.

    **Earlier verdicts checked.**
    - **words-without-agent-reach** (A) holds the words, and its text says a thread's reader is shown that messages are held. It says nothing of a document, which has no thread.
    - **verdict-note-reach** (B) is about the note beside a verdict, which is written with it.

    Neither decides this.
  [2026-11-26 08:05] owner · approved — A

- [x] **held-on-document-waits-where** — When someone who may not direct agents comments on a document, their words wait for someone to pass them on. Where do you see them waiting? A: on the document's page alone, and those who may pass them on are sent a notice. B: also in the review queue, beside the cards waiting for you. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Words without agent reach]]
  [2026-11-24 21:40] agent — Posted for the board task `app/held-words-on-documents`, a question your A on **words-without-agent-reach** leaves open for documents. Nothing that depends on it is built.

    **Base (today).** A held message on a task's thread waits on you as an agent's question does, where your questions show, until someone passes it on. A document has no thread, so a held comment has no such line.

    **A, on its page.**
    - The comment shows on the document's page, marked held, with Pass on.
    - The members of its group who may pass it on are sent a notice by the way each chose.
    - You see it when you open that document.

    **B, in the queue too.**
    - As A, and the review queue lists each held comment beside the cards waiting, linking to it, until it is passed on.

    **Why B.** On a board where nobody else may pass a comment on, you are the only one who can, and no notice reaches you by mail or chat. Under A such a comment waits until you happen to open its document. B costs the queue one more kind of row.

    **Earlier verdicts checked.**
    - **words-without-agent-reach** (A) says the people concerned are told. It does not say where you see held words waiting.
    - **notice-sender** (B) says the page and the Mac app show every notice. It does not say where on the page a held comment waits.

    Neither decides this.
  [2026-11-26 08:14] owner · approved — B

### The note beside a verdict

Written with the verdict ([its card](#verdict-note-reach), B). This is about a
person who may decide and may not direct agents.

![The note beside a verdict, from someone who may not direct agents](users-and-identity/verdict-note.svg)

- **The note is part of the decision.** It is written on the card with the verdict, where every
  agent reads it, and opens an ask an agent answers there, as the owner's note does today.
- **On what they decide, and nowhere else.** Deciding carries the right to say why on the things
  their group owns. Their comments and messages anywhere else stay held until passed on.

What follows:

- **A note can set an agent to work on what its card decides.** A rejection that asks for a
  picture has an agent draw it. Work a note asks for beyond its card is filed from it as a task,
  which waits for someone who may direct agents to start it ([above](#who-acts-on-a-task)).
- **A task's approve is deciding too** ([its card](#task-acts-who), A), so the
  note beside the approve that is a task's `owner` stamp, or beside its changes, is written with
  it on the task's thread, on that task only.
- **A reason is never cut from its verdict**, so an agent never acts on a rejection it cannot
  read the why of.

- [r] **verdict-note-reach** — You decided that the words of someone who may not direct agents are held until passed on. A person may still be allowed to decide. Their verdict is written on the card, where agents read it. What is left to choose is the note they write beside it. A: the note is held like their other words, so an agent sees a rejection and not its reason. B: the note is written with the verdict; deciding carries the right to say why, on that card only. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#The note beside a verdict]]
  [2026-11-21 17:41] agent — Posted for the board task `app/users-and-identity`, a question your A on **words-without-agent-reach** does not answer. A concept: nothing is built by this card.

    ![The note beside a verdict, from someone who may not direct agents](users-and-identity/verdict-note.svg)

    **Base (today).** Your verdict and its note are written on the card together, and an agent is handed both at once.

    **A, the note is held.**
    - The verdict is written on the card. The note is kept by the board until someone who may direct agents passes it on.
    - An agent sees a rejection and not its reason.

    **B, written with the verdict.**
    - The note is part of the decision and is written with it.
    - Deciding carries the right to say why, on that card and nowhere else. Their words anywhere else stay held.

    **Why B.** A verdict without its reason gives an agent nothing to act on, and a rejection read alone is acted on wrongly. The person was given the power to decide that card, which already moves agents.

    **Earlier verdicts checked.**
    - **words-without-agent-reach** (A) holds such a person's words. It was asked of comments and messages. Under it a verdict's note could be read either way, which is why this is asked.

    It does not decide this.
  [2026-11-22 19:55] owner · approved — B
  [2026-11-22 20:10] agent — Recorded, B: the note beside a verdict is part of the decision and is written with it, on the card, where agents read it. A person who may decide and may not direct agents says why on the cards their group owns; their words anywhere else stay held until passed on. Written in [[analysis/users-and-identity#The note beside a verdict]]. What follows from B: the note opens an ask on that card, as your notes do today, so an agent answers it there; work it asks for beyond what the card decides is the card's to file and not to start, which still needs someone who may direct agents.
  [2026-11-22 20:13] agent · resolved

## Outside a card or a task

What deciding covers on the Settings page, and what a project's plugin is told of who asked.

### Who changes settings

The group the Settings page maps to ([its card](#settings-who-changes), A). [The settings
concept](settings-page.md) keeps a change from the page in the board's store, where it holds at
once, and offers beside it a control that makes it the project's, a commit on a green settings
check. It left to this concept who may do that with several people, at [the places a permission
attaches](settings-page.md#who-may-change-what):

| Where a value is changed | Who changes it, with people |
| --- | --- |
| this device | each person, with no power |
| this machine (`symphony.local.toml`) | whoever runs that machine's server, in a terminal |
| the board's store, and making a value the project's | the members of the Settings page's group |
| a reviewed setting | a task and a reviewer, as today |

![Who changes a preference in the board's store and makes it the project's](users-and-identity/settings-who.svg)

- **The Settings page is a place**, as a folder of documents is. The board's map gives it a
  group: the default group, unless an admin maps it to another.
- **Its members change a stored value and make it the project's.** Deciding covers it, and no
  power is added. Any one of them does it, and a later change by another overrides it, as with a
  card ([above](#who-in-a-group-decides)).

What follows:

- **An admin keeps the map of places**, the Settings page among them, as an admin keeps who is on
  the board ([its card](#people-kept-where)). A single card or task is still handed to another
  group by any member of its own ([its card](#owning-group-how-named)).
- **A change names who made it.** A commit the page makes is authored by their git handle, and a
  stored change is kept in the store's record under their short name.
- **On a board with nobody named**, the person at the machine is the default group and changes
  every setting, as today.
- **A preference spends someone's credits.** A costlier model for the board's shared agents runs
  on their host's credits. What a host may limit is a [later round's](#later-rounds).
- **Making a value the project's reaches beyond the board.** It changes the committed file every
  clone and every board of the project reads. With one board for a project, that board's say is
  the whole of it; several boards for one project is a concept of its own.

- [r] **settings-who-changes** — Today you change every setting the Settings page offers. Its concept, now on main, keeps a change in the board's store, where it holds at once, and a control beside it makes the value the project's, a commit. It left to this concept who may do that once there are several people. A: the group the Settings page maps to; the page is a place like a folder of documents, so any member of its group changes a stored value and makes it the project's, and no power is added. B: the board's admins, as with who is on the board. C: a power of its own, to configure, given apart from deciding. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Who changes settings]]
  [2026-11-22 20:13] agent — Posted for the board task `app/users-and-identity`, the question the settings concept (`app/settings-page`) left to this one. A concept: nothing is built by this card.

    ![Who changes a preference in the board's store and makes it the project's](users-and-identity/settings-who.svg)

    **Base (today).** You change every setting the page offers. A change goes to the board's store and holds at once; beside it a control makes the value the project's, committed on a green settings check. A setting that holds a command the server runs is reviewed, and never changed from the page.

    **What is not asked.** One device's choices are each person's. `symphony.local.toml` stays whoever runs the machine's server, in a terminal. A reviewed setting stays a task and a reviewer.

    **A, the page's group.**
    - The Settings page is a place, as a folder of documents is. The board's map gives it a group: the default group, unless an admin maps it to another.
    - Any member of that group changes a stored value and makes it the project's. Deciding covers it.

    **B, the board's admins.** Settings are administered, as who is on the board is.

    **C, a power of its own.** A sixth power, to configure, given to a group or a person apart from deciding.

    **Under every option** a commit the page makes names who asked and is authored by their git handle. A person a board has not added sees no Settings page of it.

    **Why A.** A model, a limit or an agent switched off is a decision about how the board's work is done, and the group that owns the work makes it. B gives configuration to whoever manages people, who on a company's board is often not who runs the work. C costs a sixth power to give.

    **What it costs, under every option.** A costlier model for the board's shared agents runs on their host's credits; what a host may limit is a later round. Making a value the project's changes the file every board of the project reads; several boards for one project is a concept of its own.

    **Earlier verdicts checked.**
    - **task-acts-who** (A) makes a task's acts part of deciding, with no power added. It was asked of tasks, not settings.
    - **owning-group-how-named** (C) lets a place give the group. It does not say the Settings page is a place.
    - **people-kept-where** (B) has an admin change a board's people, groups and powers. It says nothing of settings.
    - **settings-board-store** (B) and **settings-page-project-save** (A), on main, say where a change goes and how it is committed, not who may make it.

    None decides this.
  [2026-11-23 00:41] owner · approved — A
  [2026-11-23 00:52] agent — Recorded, A: the Settings page is a place in the board's map, the default group's unless an admin maps it to another, and the members of its group change a stored value and make it the project's. Deciding covers it, and no power is added. Written in [[analysis/users-and-identity#Who changes settings]]. What follows from A: the map of places is an admin's to keep, for every place as the card said of this one ("unless an admin maps it to another"), as an admin keeps the board's people under **people-kept-where** (B). A single card or task is still handed to another group by any member, as **owning-group-how-named** said under every option. So the first item of Later rounds is answered with no card. A change names who made it: a commit is authored by their git handle, a stored change kept under their short name.
  [2026-11-23 00:52] agent · resolved

### What a plugin is told of who asked

Who sent a write, and nothing more ([its card](#plugin-told-who), C). Today a plugin's call
carries no person: a board action's submission is the owner's answer
(`symphony_plugin::Submission`, its payload and note), and an approval authority is the one call
handed who asked (the principal in `symphony_plugin::authority::Operation`)
([PLUGIN_PROTOCOL](../../engineering/PLUGIN_PROTOCOL.md)).

![What a project's plugin is told of the person who asked](users-and-identity/plugin-told.svg)

- **The core decides first.** Every call is admitted before a plugin is asked: a person a board
  has not added never reaches its plugins, and one who may only read reaches none of its actions.
- **A write names who sent it.** A board action's submission carries the short name, display name
  and git handles of who sent it, so what the plugin writes for them (a commit, a file, a record
  of its own) names them and is authored as them. An approval authority keeps the principal it is
  handed today.
- **Reads carry nobody.** A card's renderers, a home region, a route, a board action's offer, a
  watch and a doc check are drawn alike for everyone, and stay kept until the board's inputs
  change.
- **No call carries powers.** A plugin never says who may do what.

What follows:

- **An authority is told a person by their short name**, never by their sign-in's subject, which
  can be an email: no project plugin is handed an email or a chat address.
- **Mail and chat are no project plugin's to send**, since only the server knows where a person
  is reached. The server's [notice plugins](#who-sends-a-notice) send them.
- **Who sent a submission is handed beside it**, not added to the kit's `Submission`: the host
  calls `BoardAction::submit_from` with the submission and its `Sender`, the protocol's
  `Submit.sender`, and `submit_from` answers by `submit` unless an action implements it, so a
  plugin built against an older kit keeps working
  ([PLUGIN_PROTOCOL](../../engineering/PLUGIN_PROTOCOL.md)). The line the board says on the task
  for a submission names the person.
- **A plugin's commands** are run in a terminal, as the machine's account, as today.

- [r] **plugin-told-who** — A project's plugin is told nothing of who asked today: a board action's answer is taken as yours. With several people the core admits every call before a plugin is asked, so who may do what is settled before the plugin runs. What is left is what the plugin is told. A: nothing; the board's own line names the person, and the plugin's own commit is authored as the machine. B: on every call made for a person, their public part (short name, display name, git handles) and their groups and powers there. C: the public part on a write alone, a board action's answer; reads carry nobody, and no call carries powers. Recommended: C. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#What a plugin is told of who asked]]
  [2026-11-22 20:13] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    ![What a project's plugin is told of the person who asked](users-and-identity/plugin-told.svg)

    **Base (today).** A plugin's call carries no person. A board action's answer is yours, and the line the board says for it on the task is said as you. The one call already told who asked is an approval authority's, which signs what was decided. Reads (a card's drawing, a home region, a route) are alike for everyone, and kept until the board changes.

    **Under every option** the core says whether a call is allowed before it is made: a person a board has not added never reaches its plugins, and one who may only read reaches none of its actions. A plugin is handed no email and no chat username.

    **A, nothing.** The core admits the call and writes the board's line under the person's name. The plugin never knows who it was, and a commit of its own is authored as the machine.

    **B, name and powers, on every call for a person.** Short name, display name and git handles, with the person's groups and powers on the board. A plugin can draw or refuse by them.

    **C, name, on a write alone.** A board action's answer carries the short name, display name and git handles of who sent it. Reads carry nobody, and no call carries powers.

    **Why C.** What a plugin writes for a person (a commit, a file, a record of its own) can name them and be authored as them, as the board's own saves are. Who may do what stays the core's alone, on your line between signing in and what a person may do. Reads stay alike for everyone, so they stay kept. B makes each plugin a second place deciding who may do what, and every read one per person. A leaves a plugin's commits authored as the machine.

    **What it costs.** Under B or C the kit's types gain a field, shipped beside what they hold today under the kit's version rules, so a plugin built against an older kit keeps working.

    **Earlier verdicts checked.**
    - **person-profile-where** (B) keeps a profile with the server and its public part on lines. It does not say what of it a plugin is told.
    - **identity-provider-how-added** (B) and **sign-in-service-shipped** (B) are about the server's sign-in plugin, not a project's plugins.
    - Your note on **group-card-who-decides**, "Feels this is mixing authentication with authorization", keeps who may do what the Symphony's own. It leaves open what a plugin is told once the Symphony has decided.

    None decides this.
  [2026-11-23 00:40] owner · approved — C
  [2026-11-23 00:52] agent — Recorded, C: a board action's submission carries the short name, display name and git handles of who sent it; reads carry nobody; no call carries powers; an approval authority keeps the principal it has today. Written in [[analysis/users-and-identity#What a plugin is told of who asked]]. What follows from C: an authority is told a person by their short name, never by their sign-in's subject, which can be an email, since the card said no plugin is handed one. The kit's Submission gains who sent it beside what it holds, under the kit's version rules, so an older plugin keeps working. And as only the server knows where a person is reached, mail and chat are no project plugin's to send: **notice-sender** asks what sends them.
  [2026-11-23 00:52] agent · resolved

## Names and notices

How a person shows outside the board's rows: their name on the lines they write, and the notices
sent to them.

### How a person's name is written

The name in the owner's place ([its card](#person-name-on-lines), A). Today every line a person
writes is `owner`'s: a card's line reads `owner · approved — A` after its date, a message on a
task's thread has the sender `owner`, and a comment in a document names nobody.

![How a person's name is written on a card's lines](users-and-identity/name-on-lines.svg)

- **A person's line carries their short name**: `ana · approved — A` on a card, the sender of a
  message on a thread, the name on a comment in a document.
- **A person's line is any line that is not an agent's.** `agent` and `owner` are never given to
  anyone as a short name.
- **The person at the machine writes under a short name of their own once they give one**
  ([its card](#machine-person-name), C): on every board, whether it names anyone else or not.
  They give it in the You section of the People page, which asks once more before sending it,
  as the name is never changed once given. They keep every power. Until then their lines read
  `owner`, the role's word.
- **What is written stays.** A line already written is never rewritten. The lines written as
  `owner` are the machine's person's, and the pages draw them with the name that person is shown
  by, the word the line holds on hover.

What follows:

- **Every reader of the record takes one rule**: the tidy pass, the queue, the rule that an
  approved card decides a later one and the sweep that keeps a worktree holding a line no commit
  has each read a person's line as any line not an agent's, where they look for `owner ·` today.
- **`board` is kept from anyone too**, as the name the board writes its own lines under on a
  thread.
- **What the server says of them now names them**: a stamp's reading, `Approved by Stefano at
  1a2b3c4`, and the line that moves a task out of its block. A wait names them only while nobody
  else on the board decides. The `owner` stamp keeps its name, and `--to owner` still reaches
  them.
- **The header names the reader** and links to their profile: the person at the machine's own
  section, or a person's card.
- **The record's format changes for every project**, so the build that brings names says so to
  consuming projects, as any change to what a project's tools read does. A project whose person
  at the machine gives no name writes as it does today.

- [x] **machine-person-name** — Once a board names other people, your own lines there still read `owner ·`, beside `ana ·` and the others'. What is left is whether you take a short name of your own, and when. A: never; you are `owner` on every board, the one line that names no profile. B: on a board that names anyone else, your new lines carry your own short name, from a profile you give yourself, and on a board with nobody named they read `owner ·` as today. C: once you give yourself a short name, every new line of yours carries it, on every board, named or not. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#How a person's name is written]]
  [2026-11-23 19:40] agent — Posted for the board task `app/names-on-lines`. A concept: nothing is built by this card.

    **Base (today, with names on lines built).** A person a board names writes `ana · approved — A`, a thread's sender `ana` and a comment `ana · title`. You, the person at the machine, write `owner` on every board, and hold every power there. A line already written is never rewritten: the lines written as `owner` read as yours.

    **A, `owner` always.**
    - Your lines read `owner ·` on every board, beside the named people's.
    - Nothing more to build. A reader who meets `owner` must know it is the machine's person.

    **B, your name where others have theirs.**
    - On a board that names anyone else, your new lines read with your own short name (`stefano ·`), taken from a profile you give yourself; you still hold every power there.
    - On a board with nobody named, your lines read `owner ·` as today.
    - Your older `owner` lines stay as written, and still read as yours.

    **C, your name everywhere once you set it.**
    - Once you give yourself a short name, every new line of yours carries it, on every board.
    - A consuming project with nobody named starts writing your name, where it writes `owner ·` today.

    **Why B.** Once several people decide on a board, `owner` names nobody a reader can find in its people, which is why names on lines took the owner's place. B gives that board your name and leaves every board with nobody named as it is. A costs a reader the knowledge that `owner` is the machine's. C changes the record of projects where nobody but you writes.

    **Earlier verdicts checked.**
    - **person-name-on-lines** (A) reads the lines written as `owner` as the machine's person's, and keeps `owner ·` on a board with nobody named. It does not say what your new lines read once others are named.
    - **person-profile-where** (B) gives a person a profile with their short name. It does not say whether the person at the machine has one.
    - **people-kept-where** (B) gives the person at the machine every power on a board with nobody named. It does not say how their lines read once others are.

    None of them decides this.
  [2026-11-24 11:29] agent — Recorded from the owner's words on the board task `app/machine-person-name`: "it should move to person name, even if there is no one else. If we don't have the person's name, fallback is their role", "right now everyone owner should become "stefano"", "Could be "Stefano"" and "But it is missing some place to edit your profile". That is C, and it is built. The People page has a You section where the person at the machine gives themselves a short name. They are asked once more before it is sent, because the name is never changed once given. From then on every new line of theirs carries that name on every board, named or not, and they keep every power. Lines already written as `owner` stay as written, and the pages draw them with the name the person is shown by (`Stefano`), the stored word on hover. What the server says of them now names them too: "Approved by Stefano at 1a2b3c4", or "Waits on Stefano's approve" where nobody else decides. The stamp's own name, `owner`, stays. The header shows the name and links to the You section. Until a name is given, every line reads `owner`, the role's word, as before. Written in [[analysis/users-and-identity#How a person's name is written]].
  [2026-11-26 08:03] owner · approved — C

- [r] **person-name-on-lines** — Once several people are on a board, each line a person writes on a card carries their short name where it says `owner` today. The choice is how it is written. A: the name takes the owner's place, `ana · approved — A`; a person's line is any line that is not an agent's, and `agent` and `owner` are never anyone's name. B: the word stays and the name follows, `owner ana · approved — A`. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#How a person's name is written]]
  [2026-11-23 00:52] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    ![How a person's name is written on a card's lines](users-and-identity/name-on-lines.svg)

    **Base (today).** Your lines read `owner · approved — A` after their date. A message of yours on a task's thread has the sender `owner`, and a comment of yours in a document names nobody. An agent's line reads `agent —`. The tidy pass, the queue and the rule that an approved card decides a later one find a person's line by `owner ·`.

    **A, the name in the owner's place.**
    - `ana · approved — A`. A person's line is any line that is not an agent's.
    - `agent` and `owner` are never given to anyone as a short name.

    **B, the owner's word, then the name.**
    - `owner ana · approved — A`. `owner` stays the mark of a person's line.
    - Every reader that looks for `owner ·` stays as it is.

    **Under both** a comment names who left it and a message its sender, by the same short name. A line already written is never rewritten, and the lines written as `owner` read as the machine's person's. On a board with nobody named, new lines keep reading `owner ·`.

    **Why A.** The line says who in the word a reader looks at. Once several people decide, `owner` names nobody in particular: a card has an owning group, and its line is one member's. B saves each reader a change at the cost of a word that no longer says what it means.

    **Earlier verdicts checked.**
    - **person-profile-where** (B) puts the short name on a person's lines. It does not say whether `owner` stays beside it.
    - **group-answer-settles** (A) settles a card by one member's answer, under that member's name. It does not say how the name is written.

    Neither decides this.
  [2026-11-23 01:02] owner · approved — A
  [2026-11-23 01:11] agent — Recorded, A: a person's line carries their short name in the owner's place, `ana · approved — A`, a thread's sender and a comment's name likewise; a person's line is any line that is not an agent's, and `agent` and `owner` are never anyone's name. Written in [[analysis/users-and-identity#How a person's name is written]]. What follows from A: the readers that look for `owner ·` today (the tidy pass, the queue, the rule that an approved card decides a later one, the sweep that keeps an unsaved line's worktree) each take any line not an agent's as a person's. `board`, the name the board writes its own thread lines under, is kept from anyone too. And as the record's format changes for every project, the build that brings names tells consuming projects so; a project with nobody named writes as today.
  [2026-11-23 01:12] agent · resolved

### Who sends a notice

A notice plugin, the server's ([its card](#notice-sender), B), one for each service
([below](#what-the-symphony-ships-for-notices)). Today a notice reaches the owner on the page (the
queue, the Activity panel) and in the Mac app, and nothing is sent by mail or chat.

![What sends a notice outside the page](users-and-identity/notice-sender.svg)

- **Each is a separate program**, named in the server's settings as the sign-in service is,
  outside the core and never a project's: one for Slack, one for WhatsApp, one for email.
- **The server hands one notice at a time** to the plugin of the way a person chose: their
  notice and the address their profile names for that way.
- **Each keeps what its sending needs**, such as the Slack token or the mail server's password.
  Any program that takes the same message replaces one.

What follows:

- **A plugin decides nothing.** The board decides who is told (the members of the group that
  owns the thing) and a person's preferences which notices by which way. A plugin is handed no
  list of people, only each address as a notice needs it.
- **The message the server hands it is a contract** kept stable across releases, as the sign-in
  header is.
- **The page and the Mac app show every notice**, so a person who sets no address misses nothing.
- **A notice leaves the Symphony** for a mail server, Slack or WhatsApp. What it carries there, a
  link alone or the words it is about, is a [later round's](#later-rounds).

- [r] **notice-sender** — Today a notice reaches you only on the page and in the Mac app. A person's profile can hold their email and their Slack username, and which notices they want by which way; what sends a notice by mail or Slack? A: nothing; notices stay on the page and in the Mac app. B: a notice program, the server's plugin like the sign-in service, outside the core; the server hands it one person's notice and address, and it keeps the Slack token or the mail password. C: the core sends them itself, with each service's credentials in the server's settings. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#Who sends a notice]]
  [2026-11-23 00:52] agent — Posted for the board task `app/users-and-identity`, from your note on **public-name-where-declared**, which named "communication usernames and preferences (for example slack user)" for a person's record. A concept: nothing is built by this card.

    ![What sends a notice outside the page](users-and-identity/notice-sender.svg)

    **Base (today).** A notice reaches you on the page (the queue, the Activity panel) and in the Mac app. Nothing is sent by mail or chat. The server keeps a person's profile, and no project's plugin is handed an address (**plugin-told-who**, C).

    **A, nothing outside the page.** Notices stay on the page and in the Mac app, and a profile's addresses wait.

    **B, a notice program.**
    - A separate program the server's settings name, as the sign-in service is: the server's plugin, outside the core.
    - The server hands it one person's notice and the address their profile names for the way they chose.
    - It keeps the Slack token or the mail server's password. Any program that takes the same message replaces it.

    **C, the core sends them**, with each service's credentials in the server's settings.

    **Under every option** the board decides who is told (the members of the group that owns the thing), a person's preferences say which notices and by which way, and the page and the Mac app show every notice.

    **Why B.** A chat token and a mail password stay outside the core, as the sign-in service keeps the OAuth secret, on the line you drew for signing in. A leaves the chat usernames you named unused. C puts each chat service's interface and secret in the core.

    **Earlier verdicts checked.**
    - **person-profile-where** (B) keeps a person's addresses and preferences with the server. It does not say what sends to them.
    - **plugin-told-who** (C) hands a project's plugin no address, so no project plugin sends one. Each option here is the server's.
    - **identity-provider-how-added** (B) and **sign-in-service-shipped** (B) put signing in outside the core. They are about signing in, not notices.

    None decides this.
  [2026-11-23 01:01] owner · approved — B
  [2026-11-23 01:11] agent — Recorded, B: a notice by mail or Slack is sent by a separate program the server's settings name, outside the core and never a project's; the server hands it one person's notice and address at a time, and it keeps the Slack token or the mail password. Written in [[analysis/users-and-identity#Who sends a notice]]. What follows from B: the program decides nothing and is handed no list of people; the message it takes is a contract kept stable across releases, as the sign-in header is; the page and the Mac app still show every notice. Two things it leaves open: what the Symphony ships of the program, asked now as **notice-program-shipped**, and what a notice carries outside the Symphony (a link alone, or the words), listed for a later round.
  [2026-11-23 01:12] agent · resolved

### What the Symphony ships for notices

The core ships the message's format alone, and three plugins apart from it send: one for Slack,
one for WhatsApp, one for email ([its card](#notice-program-shipped), A). The owner's words for
the plugins: "I would like plugins added (separate from the current core of Symphony) for Slack,
WhatsApp and Email."

![Notices: the format in the core, a plugin for each service](users-and-identity/notice-plugins.svg)

- **The core sends nothing.** It makes the notice message and hands it on. The message is a
  contract kept stable across releases ([above](#who-sends-a-notice)), so a plugin built on it
  keeps working.
- **Each service is a plugin of its own**, a package the core does not link, as the GitHub issue
  exchange is. Whoever runs the server sets up the ones they want, and nothing of them is in a
  project's `symphony.toml`. Any program that takes the same message replaces one.
- **Each keeps its own secret**: the Slack app's token, the WhatsApp business account's token,
  the mail server's password.

What follows:

- **A notice goes to the plugin of the way its person chose.** With no plugin set up for that
  way, it stays on the page and in the Mac app, where every notice is shown anyway.
- **A person's profile gains a WhatsApp number**, beside their emails and Slack user
  ([above](#a-persons-profile)).
- **The format comes first** ([NOTICES](../../engineering/NOTICES.md)), then the three plugins,
  each built and released on its own, in any order. What each needs:
  - **Slack**: a Slack app installed in the workspace and its bot token; a direct message to the
    person's Slack user. It follows Slack's interface as Slack changes it.
  - **WhatsApp**: a WhatsApp business account with a number to send from, and its token. A
    message that opens a conversation must use a template Meta has approved, so each kind of
    notice is a template submitted first; the person agrees to be messaged; and Meta charges for
    such messages.
  - **Email**: a mail server and its password, or a mail service's key, and an address to send
    from. Mail is a standard that changes little.

- [r] **notice-program-shipped** — Follows notice-sender, where you chose B: a notice by mail or Slack is sent by a separate program, the server's plugin, outside the core. What is left is what the Symphony ships of it. A: the message's format alone; notices stay on the page and in the Mac app until someone writes a program. B: the format and a notice program of its own, for mail and Slack, which any other replaces. C: the format and a program for mail alone; Slack is left to programs others write. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/users-and-identity#What the Symphony ships for notices]]
  [2026-11-23 01:10] agent — Posted for the board task `app/users-and-identity`, from your B on **notice-sender**. A concept: nothing is built by this card.

    ![What the Symphony ships for sending notices](users-and-identity/notice-shipped.svg)

    **Base (today).** Nothing is sent outside the page. For signing in, the Symphony ships the check and the format first, then a sign-in service of its own that any other replaces (**sign-in-service-shipped**, B).

    **A, the format alone.** A notice stays on the page and in the Mac app until someone writes a program that takes it.

    **B, the format and a program for mail and Slack.** Any program that takes the same message replaces it.

    **C, the format and a program for mail alone.** Slack and other chat services are left to programs others write.

    **Under every option** the program is a package the core does not link, as the GitHub issue exchange is, set up once by whoever runs the server. Nothing of it is in a project's `symphony.toml`.

    **Why B.** Slack is the way you named for a person's record, and a format no program sends to reaches nobody. Its cost is the Symphony's: Slack's interface followed as Slack changes it, and its token kept safe. Mail (C) is a standard that changes little.

    **Earlier verdicts checked.**
    - **notice-sender** (B) puts sending in a program outside the core. It does not say whether the Symphony ships one.
    - **sign-in-service-shipped** (B) ships a sign-in service of the Symphony's own. It is about signing in, not notices.

    Neither decides this.
  [2026-11-23 11:18] owner · approved — A. But I would like plugins added (separate from the current core of Symphony) for Slack, WhatsApp and Email.
  [2026-11-23 11:21] agent — Recorded, A, with your addition: "I would like plugins added (separate from the current core of Symphony) for Slack, WhatsApp and Email." The core ships the notice message's format alone and sends nothing. Three plugins outside it send: one for Slack, one for WhatsApp, one for email. Each is a package the core does not link, set up by whoever runs the server, and each keeps its own secret. Written in [[analysis/users-and-identity#What the Symphony ships for notices]] with a new picture, and in [[analysis/users-and-identity#Who sends a notice]]. What follows: a notice goes to the plugin for the way its person chose, and stays on the page and in the Mac app when none is set up; a person's profile gains a WhatsApp number; the format comes first, and each plugin is then a build of its own, in any order. WhatsApp costs the most of the three. A notice that starts a conversation must use a template Meta has approved, so each kind of notice is a template submitted first. The person must agree to be messaged, and Meta charges for such messages.
  [2026-11-23 11:21] agent · resolved

## Sessions

- **On a machine's server**, none are added. A request from the machine is the machine's
  person. A request from the tailnet is the login Tailscale names for its address, asked as
  today and kept for the 30 seconds the gate already keeps it. A peer under the machine's own
  login is the same person, so a phone keeps working with nothing set up. A peer under another
  login is that person, and sees only the boards that have added them
  ([above](#a-tailnet-machine-under-another-login)).
- **On a hosted board**, the session is the sign-in service's. The adapter verifies the signed header on
  every request, ends a stream when the assertion expires, and binds its request-forgery token
  to the session and the project, all as [hosted ingress](../../operations/analysis/hosted-ingress.md#browser-behavior)
  sets out. The Symphony stores no session.

## Agents beside people

An agent stays an agent: a principal of its own kind, which no claim turns into a person, so it
never decides a card, gives the `owner` stamp or takes an edge that waits for a person. With
people on a board there are two kinds:

- **A personal agent** is one a person starts on their own machine, under their own credits.
  Only that person directs it. On a task it may do what they may there, no more and no less
  ([decided](#whose-agent-may-review)): a reader's agent reads, and the agent of someone who may
  direct agents works with a task's agents as they do, a review among it. Others see what it
  posts and speak under it; their words reach its person, not the agent.
- **A shared agent** is the board's: the orchestrators, executors and reviewers staffed for its
  tasks. It runs on a host, a machine and credits somebody offers the board. Anyone who may
  direct agents directs it, whoever hosts it. It keeps two names apart: the person whose word it
  is acting on, and its host. It does no more than the person whose word it acts on may. The
  execution design already keeps such a pair, as the acting principal and the initiator.

On one machine with one person every agent is both at once: started by them, on their machine,
directed by them. Nothing changes there.

- [x] **machine-agents-whose** — On a board that names other people, every agent on it still runs on your machine, on your credits: each is yours, and those holding a task's role are the board's shared agents too. What is left is whose words reach your agents there. A: every session on your machine is both, so anyone the board lets direct agents reaches any of them, as today. B: only a session holding a role on a task (its orchestrator, executor or reviewer) is shared; any other session is yours alone, and someone else's words to it come to you, not to the agent. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Agents beside people]]
  [2026-11-24 21:45] agent — Posted for the board task `app/personal-agents`, step 5's last part. A concept: nothing is built by this card.

    **Base (today).** Every agent that reaches a board runs on that board's machine: the board's commands write its database there, and no command reaches a hosted board yet. So every session is yours. Someone the board named who may speak and direct agents (`bo`) reaches every session as you do: on a task's thread, by an ask or a comment, and through the quick chat, which goes to whoever holds the chat.

    **A, every session is both.**
    - Your agents work for anyone the board lets direct agents, as they do today.
    - "Their words reach its person, not the agent" first matters for an agent from another machine, once one can show a board whose it is (a later round).
    - Nothing to build.

    **B, only a session holding a task's role is shared.**
    - Its orchestrator, executor or reviewer is the board's. Any other session is yours alone.
    - bo's message to such a session, and bo's quick chat while the chat's session holds no task's role, come to you, and no agent is handed them until you pass them on.

    **Under both** a session never decides a card or gives the `owner` stamp, and its own words are never held: it stands where you stand.

    **Why A.** Giving bo the power to direct agents already lets bo set your machine's agents to work on your credits. B keeps the sessions you run for yourself to yourself, at the cost of a second rule for whose words reach a session, and of bo's quick chat coming to you instead of an agent.

    **Earlier verdicts checked.**
    - **review-not-own-agent** (A) has a personal agent stand where its person stands, and any fresh session review. It does not say which of your machine's sessions are personal and which shared.
    - **task-acts-who** (A) says starting an agent on a task needs the power to direct agents. It does not say whose words reach an agent already running.
    - **words-without-agent-reach** (A) holds the words of someone who may not direct agents. It is about whose words are held, not which agents the words of someone who may reach.

    None of them decides this.
  [2026-11-26 08:06] owner · approved — A

Left for a later round: who hosts a board's shared agents and what a host may limit, beside the
proposed execution providers, where a project's provider owns its runtime identity and
credentials ([its analysis](../../engineering/analysis/remote-execution-attempts.md)); and how an
agent on another machine proves its handle to a board.

### Whose agent may review

Any fresh session, whoever's agent it is ([its card](#review-not-own-agent), A), on the owner's
own principle: "If the person can interact with agents on a task, so should their own agents
too."

![A person's own agent stands where its person stands](users-and-identity/agent-reach.svg)

- **The fresh-session rule is today's.** A session that executed a task, orchestrates or
  orchestrated it, or filed it never reviews it ([the default
  landing](../../workflow/TASKS.md#the-default-landing)). Any other session reviews it, the
  personal agent of the person who filed it or started its work among them.
- **A personal agent stands where its person stands.** Whether its person is a member of the
  group that owns a task, and the powers they hold, say what it may do there with the task's
  agents: what its person may, no more and no less.
- **A review is an agent's verdict**, never a person's decision. No agent decides a card or gives
  the `owner` stamp, whoever's agent it is. A person's own word on a task is still that stamp
  where its assessment names one.

What follows:

- **The agent of someone who may direct agents** reviews any task a fresh session may, and its
  messages on a task's thread reach the task's agents as its person's do.
- **The agent of someone who may not direct agents** writes as its person does. What it says on
  a task, a review among it, is held by the board until someone who may direct agents passes it
  on ([above](#words-without-agent-reach)).
- **The board's staffed reviewers are shared agents**, and review anyone's task as today.
- **The board knows whose agent a session is.** A personal agent's reach is read from its
  person, so the tie between them is a board's record. How an agent on another machine proves it
  is [left for later](#later-rounds), with how it proves its handle.
- **On a board with nobody named** every agent is the one person's and reviews as today.

- [r] **review-not-own-agent** — With several people, a task could be reviewed by a personal agent of the very person who filed it or started its work: a fresh session, but working on that person's word. Today's rule reads sessions alone. A: as today, a fresh session reviews whoever's agent it is. B: a personal agent of the person who filed the task or started work on it gives it no verdict; a shared agent, or another person's agent, does. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#Whose agent may review]]
  [2026-11-23 01:10] agent — Posted for the board task `app/users-and-identity`. A concept: nothing is built by this card.

    ![Whose agent may review a task](users-and-identity/review-whose.svg)

    **Base (today).** A task's review comes from a fresh session: never one that executed the task, orchestrates or orchestrated it, or filed it. Nothing says whose an agent is.

    **With people** a reviewer is a shared agent, staffed for the board by its role and acting on no one person's word, or a person's personal agent, which works on its person's word alone.

    **A, sessions alone.** A fresh session reviews, whoever's agent it is. A person's own word on a task is still the `owner` stamp where its assessment names one.

    **B, not their own agent.** A personal agent of the person who filed the task or started work on it gives it no verdict. A shared agent, or another person's agent, does.

    **Under both** a board with nobody named reviews as today, every agent being the one person's, and a review stays an agent's verdict, never a person's decision.

    **Why B.** A personal agent's review is its person's own read of their own work, which the fresh-reviewer rule keeps out for sessions. The board's staffed reviewers are shared agents and still review anyone's task, so B costs little.

    **Earlier verdicts checked.**
    - **task-acts-who** (A) says starting an agent on a task needs the power to direct agents. It does not say whose agent may review it.
    - **words-without-agent-reach** (A) is about what an agent is told, not who may give a verdict.

    Neither decides this.
  [2026-11-23 11:10] owner · approved — This should follow the member/owner aspect. If the person can interact with agents on a task, so should their own agents too. This sounds like A
  [2026-11-23 11:17] agent — Recorded, A, with your principle: "If the person can interact with agents on a task, so should their own agents too." Any fresh session reviews a task, whoever's agent it is, the personal agent of the person who filed it or started its work among them; the fresh-session rule itself is today's. A personal agent stands where its person stands: whether they are a member of the group that owns the task, and the powers they hold, say what it may do there with the task's agents, no more and no less. The concept said "no more" already; your note adds "no less". Written in [[analysis/users-and-identity#Whose agent may review]], with a new picture, and in [[analysis/users-and-identity#Agents beside people]]. What follows: the agent of someone who may not direct agents writes as they do, so what it says on a task, a review among it, is held until someone who may direct agents passes it on; an agent still never decides a card or gives the owner stamp; and the board must know whose agent a session is, whose proof for an agent on another machine is listed for a later round with its handle's.
  [2026-11-23 11:17] agent · resolved

## The way from one owner

Each step can ship alone, and a machine with one person sees none of them.

1. **The actor seam.** Every write takes an actor. The local host supplies the local owner, and
   every line reads as it does today. A board action's submission tells its plugin who sent it.
2. **Tailscale's login read.** The gate keeps who a peer is. A peer under the machine's own
   login is its person. A peer under another login is that login's person, and until a board
   adds them the server shows them nothing.
3. **People, groups and powers on a board.** The rows, the People page, names on lines.
   Built: names on lines; a board's people, groups and powers as rows, the command that changes them
   (`just tasks-people`, which an agent is asked before) and the log it keeps; the profiles in the
   machine folder; a visitor under another login on the boards that give them a power; each write
   checked against the powers it needs; the `owner` stamp opened by decide
   ([TASKS](../../workflow/TASKS.md#people)); and the People page, read by everyone on the board,
   its changes guarded by administer ([TASKS](../../workflow/TASKS.md#the-people-page)).
4. **Ownership.** A card and a task have their group, from the board's map of places or a name
   written on them; queues and notices follow it, and any member's answer settles what the
   group owns. Its members move, prioritize and cancel its tasks. The Settings page is one of
   the places, and its group's members change its settings. The core makes notice messages, and
   the server's plugins for Slack, WhatsApp and email send them, each shipping on its own.
   Built: the notice message's format and its hand-off, which a plugin builds on
   ([NOTICES](../../engineering/NOTICES.md)); the Slack plugin
   ([NOTICE_SLACK](../../operations/NOTICE_SLACK.md)), the WhatsApp plugin
   ([NOTICE_WHATSAPP](../../operations/NOTICE_WHATSAPP.md)) and the email plugin
   ([NOTICE_EMAIL](../../operations/NOTICE_EMAIL.md)); a person's addresses (an email, a Slack
   member id, a WhatsApp number, each held to its plugin's rule) and the way each kind of notice
   goes by, on their profile, written by them on the People page or by the person at the machine,
   and the server's read of where a notice of theirs goes ([TASKS](../../workflow/TASKS.md#people));
   the board's map of places to groups, kept by its admins, a card's `*For:*` and a task's
   `--group`, the read of whose a thing is and what gave it, drawn on a task's page and a card, and
   its hand-over by a member of its group
   ([TASKS](../../workflow/TASKS.md#which-group-a-thing-belongs-to)); a card's verdict and Later, a
   task's moves, priority, fields, cancel and approve and the settings decided by the members of
   its group who hold decide, the `owner` stamp read from them, and the controls drawn for them
   alone ([TASKS](../../workflow/TASKS.md#people)); each person's queue, Home's first cards and
   counts, the postponed and pending lists, a folder's count, a project's row and the feed holding
   the cards they decide alone ([TASKS](../../workflow/TASKS.md#people)); and the machine's one
   server telling each member of a
   thing's group of a card waiting, an agent's question and a reviewer's verdict, by the way each
   chose, through the programs the machine folder names
   ([NOTICES](../../engineering/NOTICES.md#the-servers-part)).
5. **Speaking apart from directing agents.** A person's words reach people always. The board
   holds the words of someone who may not direct agents, and passing them on makes them an ask.
   The note beside a verdict is written with it. A personal agent, tied to its person on the
   board, reaches a task's agents as its person does, a review among it.
   Built: on a task's thread, the message of a person who may speak and may not direct agents held
   for people, shown to them on the task page and waiting on the person at the machine, its line
   told to the members of the task's group as step 4 tells a question, an agent's read saying only
   how many are held; passed on with one press or `just tasks pass`, under both names, as an ask;
   and direct agents given and taken as any power
   ([TASKS](../../workflow/TASKS.md#words-held-for-people)). On a document, such a person's
   callout or card message held the same way, reaching no clone, no ask and no agent, drawn where
   it was left on the document's page and told to those who may pass it on, and passed on with one
   press under both names, as an ask, after its block found again or refused with nothing written;
   and the note beside a verdict written with it, by one who may decide and not direct agents.
   Not yet: a held ask, which [its card](#held-ask) asks of the owner, while the document's page
   draws its questions' Ask and Follow up for those who may ask alone; and the two cards above on
   what an agent is told of a document's held comments and where they wait.
   Personal agents, as far as any agent reaches a board: every session that reaches one runs on
   its machine, under its account and on its credits, since the board's commands write its
   database there and none reaches a hosted board yet
   ([POSTGRESQL](../../operations/POSTGRESQL.md#a-hosted-board)). So each is the person at the
   machine's personal agent, and a shared agent too where it holds a task's role, and it stands
   where they stand, each part held by a test:
   - its words are never held, and a pass of held words by it is theirs
     (`crates/tasks/tests/store/held_words.rs`:
     `who_may_direct_agents_writes_as_the_owner_does_and_only_a_persons_words_are_held`,
     `passed_on_the_words_reach_the_tasks_agents_once_under_both_names_and_ask`);
   - it never writes as a person, so its approve is no `owner` stamp: the command line refuses
     `owner` as its actor (`crates/tasks/tests/cli/commands.rs`:
     `owner_actors_are_refused_by_flag_and_environment_without_changing_the_board`), no session
     is given a person's name (`crates/tasks/tests/store/board_people.rs`:
     `no_person_is_named_as_a_handle_and_no_session_is_given_a_persons_or_a_groups_name`), and a
     session's approve meets no stamp (`crates/tasks/src/stamps/read/tests.rs`:
     `the_owners_stamp_is_the_owners_approve_naming_the_tip_and_stale_at_a_later_commit`);
   - it reviews by today's fresh-session rule (`crates/tasks/tests/store/verdict.rs`:
     `whoever_orchestrates_orchestrated_or_filed_the_task_is_refused_its_review_at_once`,
     `a_session_that_executed_the_task_gives_it_no_verdict_whatever_role_it_holds_now`).

   Two things wait. On a board that names others, which of the machine's sessions are its
   person's alone, whose words no one else's reach, is [an open card](#machine-agents-whose).
   Another person's personal agent needs an agent on another machine to reach a board and prove
   its handle and whose it is, a [later round's](#later-rounds), after step 7's agent
   credentials: until then no record ties a session to a person, as one claimed by hand would say
   what no session on the machine is.

- [x] **held-ask** — A person who may speak and may not direct agents is refused an ask about a document, its follow-up and the `+` sheet's chat, since asking is directing an agent. They can already leave a held question after any block, which an agent answers once someone passes it on. Should their ask also be held and passed on like a comment? A: no, it stays refused. B: held, drawn in the asks panel, and passed on as an ask under both names. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/users-and-identity#The way from one owner]]
  [2026-11-25 08:39] agent — Posted for the board task `app/held-asks-on-documents`, a question your A on **words-without-agent-reach** leaves open for asks. Nothing that depends on it is built. Under either answer the document's page draws Ask and Follow up only for someone who may ask.

    **Base (today).** An ask is a question to the agent that answers chat. It reaches that agent at once and changes no file. Someone who may speak and may not direct agents is refused it, its follow-up and the `+` sheet's chat. Their question after a block is held: everyone reading the page sees it, those who may pass it on are told, and once passed it becomes an ask an agent answers, under both names.

    **A, it stays refused.**
    - They ask about a place in a document by leaving a held question there.
    - The page draws them the asks' threads and no Ask or Follow up.

    **B, held and passed on.**
    - Their ask is kept by the board and drawn in the asks panel, marked held, with Pass on for those who may.
    - Passed on, it becomes the passer's ask under both names, and an agent answers it.
    - Their follow-up on a thread waits the same way, and so does each line of their chat in the `+` sheet.

    **Why A.** The held question already does it: it hangs on the place it asks about and becomes an ask once passed on. Only an agent answers an ask, so a held one is a question nobody answers until it is passed, and whoever passes it could ask it themselves. B costs a second held kind in the asks panel and the chat, and a chat whose every line waits for someone is no chat.

    **Earlier verdicts checked.**
    - **words-without-agent-reach** (A) says no agent sees their words until someone passes them on. It rules out letting them ask outright, and does not say whether they may ask at all.
    - **held-on-document-agent-told** and **held-on-document-waits-where** (open) are about held comments, not asks.

    Neither decides this.
  [2026-11-26 08:04] owner · approved — A

6. **The signed word verified.** The core's check of a sign-in service's signed data, and hosted
   saves through the same writes as local ones. Then the Symphony's own sign-in service, a
   separate program in front.
7. **Shared agents' hosts**, then a board's agent credentials.

With nobody named on a board, the person at the machine holds every power there and is its
admin. The first person named is the first moment anything differs.

## The other two concepts

- **A settings page** ([its analysis](settings-page.md)). A change from the page goes to the
  board's store and holds at once, and a control beside it makes it the project's, a commit on a
  green settings check; a setting that holds a command the server runs is reviewed and never
  changed from the page. It left to this concept who may change the project's preferences with
  several people, [decided here](#who-changes-settings). Three things it hands back are the
  shape here already: an identity provider's settings decide who gets in, so the page never
  writes them (here they are the server's, named where it is started); a commit the page makes
  names who asked; and "this machine" is a local server's alone, as a hosted board has no
  checkout beside it. A board's people, groups and powers are that page's neighbour: a People
  page of the board, for its admins, which changes rows of the board and no file.
- **Blueprints** (`app/blueprints`). A blueprint may carry a workflow that names capabilities (a
  design lead, an art director) and the groups a kind of project expects (engineering, design).
  It carries no person and no sign-in service: people are a board's, and a project taken from a
  blueprint starts with its groups empty but for whoever made it.

## Later rounds

Not carded yet.

- Whether a board weighs one group's tasks over another's, where both are staffed from one
  priority scale.
- Which notices a person gets, and the ways each comes by default.
- What a notice carries outside the Symphony: a link alone, or the words it is about.
- Queues and notices per person and per group.
- Shared agents: whose machine and credits host them, what a host may limit, and how an agent on
  another machine proves to a board its handle and, for a personal agent, whose it is.
- Removing a person: their open asks, their unanswered cards, their agents' sessions.
- Several boards for one project: nothing proposed here ties people to a project, and what else
  a second board needs, among it who makes a value the project's for every board, is a concept
  of its own.
