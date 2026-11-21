# Users and identity

Several people on one board, each signed in through an identity provider, with roles that say
what each may do. This is a concept: it questions the idea against what the Symphony has, says
the shape that fits, and puts its choices to the owner a few at a time
([the first round](#the-first-round)). Nothing here is built, and nothing is built from it but by
a task filed from an approved card.

Read at `59538b83e`. The links name the modules and documents read.

## The answer in short

- **The Symphony has a place where the owner sits, and no person.** Every browser the server
  admits writes as the one sender `owner`. Who that browser belongs to is never read.
- **The hosted work already verifies a visitor, and then has nobody for them to be.** Its writes
  stop at a dispatch that refuses by default, because the board, the documents and the plugins
  know one actor.
- **So the work is one seam and three small things on it.** The seam: every write carries who
  made it, where a constant stands today. On it: a person, a short list of roles, and a provider
  that tells the server who is asking.
- **A provider is pluggable, and not a project's plugin.** It belongs to the server that is
  started, not to a project's file: whoever can land a change to that file would otherwise choose
  who everyone is.
- **A machine with one person changes in nothing.** With no people named, the person at the
  machine is the owner of every project it serves, as today.

## What the Symphony has today

| What | How it is today | Where |
| --- | --- | --- |
| Who may reach the server | The machine itself, and any address `tailscale whois` names a node for. The answer's `UserProfile` (a login, a display name) is in the same output and is not read. | `apps/symphony-server/src/gate.rs` |
| Who a browser write is from | `owner`, always: a card's verdict (`owner · approved`), a message on a task, a task filed from the `+` sheet, a move, the approve that is the `owner` stamp. | [REVIEW](../REVIEW.md), [TASKS](../TASKS.md#talking-on-a-task) |
| Who a command-line write is from | An agent session, by the handle it names (`--as HANDLE`). The handle is claimed, never proved. The command line refuses `owner` as an actor. | `crates/tasks/src/cli/mod.rs` |
| A principal | A kind (human, service or agent), an issuer and a subject, with no credential. The local host builds three: the local owner, the board's services, an agent by its handle. | `symphony_plugin::execution::Principal`, `crates/tasks/src/workflow.rs` |
| What a principal may do | A capability a host's grants name for it. The local host's grants are fixed in code: the local owner holds `owner`. A custom workflow can declare more capabilities, and an edge can wait for a person (`human_wait`). | [WORKFLOW](../WORKFLOW.md#admission) |
| A decision that needs several people | A workflow guard: the project keeps its own approval ledger and signs what it decided; the board verifies and never holds the ledger. | [WORKFLOW](../WORKFLOW.md#guards) |
| A hosted visitor | `hosted::router` asks its `Host` for proved transport, a verified identity and a grant for the actor, the project and the action. Reads are served. A save or a plugin call goes only to `Host::dispatch`, which refuses unless the host answers, and is never passed to the local owner's saves. | [the server guide](../../apps/symphony-server/README.md#hosted-admission), [hosted ingress](hosted-ingress.md) |
| A plugin | Trusted code of the project, selected by its `symphony.toml`, drawing blocks. Its calls carry no person. | [PLUGIN_PROTOCOL](../PLUGIN_PROTOCOL.md) |
| Settings | The committed project file changes by a landing. One machine's preferences are `symphony.local.toml`. The server's own state (its registry of projects, its releases) is the machine folder. | [PROJECT_FILE](../PROJECT_FILE.md) |
| Notices and the queue | Kept per recipient, and the board knows one recipient, `owner`. | [the desktop design](desktop-client.md) |

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
one is ([hosted ingress](hosted-ingress.md#identity-and-the-trusted-proxy-boundary)), and the
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

### The owner is three things

What the guides call the owner is, read closely, three things in one word:

- **Someone at the page.** Today a place, not a person.
- **A set of powers.** Deciding a card; the approve that is the `owner` stamp; cancelling,
  moving and ranking tasks; speaking on any thread; filing a task.
- **One voice in the record.** `owner ·` on a card's line, `owner` as a thread's sender. The
  rules read that voice: only the owner approves a card, an approved card decides a later one,
  and a worktree holding an owner's line no commit has is never removed.

With several people the three come apart. Someone at the page is a **person** a provider
vouches for. The powers are a **role** that person holds on a project. The voice stays one, the
owner's, and gains a name beside it. What the role is, and how many may hold it, is the first
choice ([below](#what-the-owner-becomes)).

A fourth thing is the owner's by another road and stays outside this concept: what is done in a
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
- **A plugin answers after admission.** The provider's answer is what admits a request, before
  any project is opened.

Pluggable it should be: the server must not be tied to one company's sign-in. The seam exists as
`hosted::Host`, whose `identity` and `authorize` are the provider and the grants. What remains
open is how far it opens ([below](#how-a-provider-plugs-in)).

## The shape proposed

- **A person** is what a provider vouched for: the provider's name and its subject for them (an
  email, an id), which are private to the server, and a short public name chosen for them
  (`ana`), which is what a line in a document or on a thread carries. A subject can be handed to
  someone else later, as an email is, so a grant names the provider and the subject together and
  is removed when the person leaves.
- **A role** is a named set of capabilities, the thing a workflow's edges already ask for. Four
  are proposed, and their exact powers are a later round's card:

  | Role | Held | May |
  | --- | --- | --- |
  | reader | on a project | read its documents, board and threads |
  | member | on a project | read; comment and ask; speak on a thread; file a task; attach files |
  | owner | on a project | a member's; decide a card; give the `owner` stamp; cancel, move and rank tasks |
  | admin | on the server | say who is a person here and what they hold; change what the settings page offers |

  The admin's role is the server's and the other three are a project's, which is today's line
  between what is done at the machine and what is said on the board. A project's own workflow
  can still name capabilities of its own (a design lead, say), granted the same way.
- **A grant** gives a person a role. Without one a person is admitted to nothing, and an unknown
  project and a refused one answer alike.
- **A provider** tells the server who sent a request, or that it cannot tell. It gives no role.
- **The actor seam** is the one change everything else stands on: every write the server makes
  takes the actor who asked for it (a person and what they hold) where the constant `owner`
  stands today. The local host supplies the person at the machine, so its lines read as they do
  now. The hosted host supplies the visitor it verified, and its saves can then go through the
  same code as local ones, with their grants checked first.

## The first round

Three choices, each a card. They are the ones every later question rests on.

### What the owner becomes

**Base.** One owner, unnamed: the rules say "the owner" and the record says `owner`.

- **A, one owner for a project.** Exactly one person holds the role. Others join as members and
  readers. The rules stand word for word. A member's lines carry a name, and the owner's need
  none.
- **B, a role a few people hold.** Any one holder decides a card, gives the stamp and moves a
  task, and their line says who (`owner ana · approved`). A later word of any holder overrides an
  earlier one, as the owner's own later word does today, and both stay in the card's history.
  "Waiting on you" is one queue for the holders; each keeps their own Later. A project with one
  holder is A.
- **C, owners by area.** A card or a task names who decides it (design, engineering), and each
  person's queue holds only theirs. It needs an area on every card, a rule for a card with none,
  and a queue per person.

> [!comment] C, but it could be "group owned". As mentioned, there can be an engineering group -> people there would get "notified" and assigned to resolve. There's also certainly a different setting for when an agent can be interact with. For example, there could be a role that allows people to comment in any place they want, including the board tasks, but that doesn't automatically trigger an execution from an agent. On the other hand, there could be a group or a person that holds the ability to interact with agents, even if those are not "their agents" (meaning not running under their token "credits" or even computer resources. And there's certainly those agents that are personal and run in your own computer for example that only you can interact with them. However interactions between people should be possible in any place.

An approval that must be several people's (two of three, a named sign-off) is not offered: a
workflow guard is that, kept in the project's own ledger
([its card](../DECISIONS.md#revision-bound-project-guards)).

**Recommended: B.** It is A for anyone who wants A, and it does not make a project choose
between adding a second decider and breaking its rules. Its cost over A is a name on the
owner's lines and the rule for two holders who disagree, which is the rule a single owner
changing their mind already follows. C answers a team with real areas, which nothing asks for
yet, and it can be added over B later as a way of sorting B's one queue.

### Where people are kept

**Base.** Nowhere: no person is recorded. The nearest things are the server's registry of
projects in the machine folder and a workflow authority's public key in the project file.

- **A, in the project file.** `symphony.toml` names each person and their role. A change is a
  landing, reviewed and in git's history, and every machine reads the same list. But each
  person's sign-in subject is committed, in a repository that may be public; whoever can land a
  change can make themselves an owner; and a person is added one landing at a time.
- **B, with the server, in no repository.** On a machine, a file in the machine folder beside
  the registry. On a hosted board, the deployment's protected store, whose first admin the
  deployment names. An admin changes it from a People page or a command, and each change is kept
  in a log of its own. A clone holds nobody's sign-in, and the public name is all a document
  carries. Each server has its own list, which is right: each is its own door.
- **C, in the provider.** Groups kept there (`symphony-owners`) are read as roles and the
  Symphony keeps no list. The reading of group to role still has to be kept under A or B, each
  provider says groups its own way, and the first provider proposed, Tailscale, gives a login and
  no group.

**Recommended: B.** Who may enter is the server's question, asked before any project is opened,
and the hosted design already keeps such settings out of what a visitor can edit. C can come
later as one more way to fill B's list.

### How a provider plugs in

**Base.** The local server asks Tailscale whether an address is a node, and nothing else. The
hosted seam is a Rust trait a consuming host implements; no provider ships.

What the server would trust under every option: where a request came from (this machine is its
own person), and a provider's verified word. Never a name in a request's body, a header a hop it
has not verified set, an email's domain, or a handle someone claims.

- **A, part of the server, chosen where it is started.** The Symphony ships two: Tailscale's
  login for a machine's server, and a signing proxy's header for a hosted one (the kind the
  hosted design read). A host with another need implements the Rust seam, as today. The Symphony
  draws no sign-in page and keeps no password, secret or session: the provider signed the person
  in before the request arrived.
- **B, a program of its own.** A provider is a binary the server's settings name, asked over a
  versioned line, as a process plugin and an execution adapter are. Anyone writes one, in any
  language, and replaces it without a new server. It adds a contract the Symphony must keep
  stable across releases, on the path every request takes, and a second process holding
  secrets.
- **C, the Symphony signs people in itself.** A sign-in page, an OpenID Connect exchange with
  Google or GitHub, a client secret, session cookies and sign-out. It needs no proxy in front. It
  is the most code, all of it the kind where a mistake is a break-in.

A `[plugins]` entry in the project file is not offered, for [the reasons
above](#an-identity-provider-as-a-plugin).

**Recommended: A, and B when a provider the two cannot reach is in fact needed.** A covers the
two places a Symphony runs. B's seam is A's with a line drawn through it, so nothing built for A
is lost.

## Sessions

- **On a machine's server**, none are added. A request from the machine is the machine's
  person. A request from the tailnet is the login Tailscale names for its address, asked as
  today and kept for the 30 seconds the gate already keeps it. A peer under the machine's own
  login is the same person, so a phone keeps working with nothing set up. A peer under another
  login is that person, admitted only by a grant. A tagged node has no login and is admitted
  only by a grant naming the node.
- **On a hosted board**, the session is the proxy's. The adapter verifies the signed header on
  every request, ends a stream when the assertion expires, and binds its request-forgery token
  to the session and the project, all as [hosted ingress](hosted-ingress.md#browser-behavior)
  sets out. The Symphony stores no session.

Under C of [the provider's choice](#how-a-provider-plugs-in) this section would be a design of
its own: a cookie, a store, expiry and sign-out.

## Agents beside people

An agent stays an agent: a principal of its own kind, which no claim turns into a person, so it
never decides a card, gives the `owner` stamp or takes an edge that waits for a person. Two
things are added once there are people:

- **A sponsor.** Each session records the person it works for: the one at the machine that
  started it, passed down from an orchestrator to the agents it starts. On one machine every
  session's sponsor is the same person and nothing changes. The execution design already keeps
  the two apart, as the acting principal and the initiator.
- **No more than its sponsor.** An agent can do nothing on a project its sponsor could not. A
  reader's agent reads.

Left for a later round: how an agent on another machine proves its handle to a shared board (a
credential a person issues, as the hosted execution design expects of its host), and whether a
review must come from another person's agent. Today's rule asks for a fresh session, not another
sponsor.

## The way from one owner

Each step can ship alone, and a machine with one person sees none of them.

1. **The actor seam.** Every write takes an actor. The local host supplies the local owner, and
   every line reads as it does today.
2. **Tailscale's login read.** The gate keeps who a peer is. A peer under the machine's own
   login is its person. Whether a peer under another login keeps today's reach until a list
   exists is a later card.
3. **People and roles.** The store, the grants, the People page, names on lines.
4. **The hosted provider.** The signing proxy's header verified, and hosted saves through the
   same writes as local ones.
5. **Agents' sponsors**, then a shared board's agent credentials.

With no people named, the person at the machine holds `owner` on every project and is the
server's admin. The first grant to someone else is the first moment anything differs.

## The other two concepts

- **A settings page** (`app/settings-page`). A saved change to the project file is a landing
  today, filed as the owner's task. With people it is filed as the task of the person who saved,
  and needs what any change of that file needs. What a machine alone decides, and turning an
  integration on, is the admin's. A setting that holds a command the server runs should not be
  an admin's to set from another machine at all: that would be running programs on the machine
  from a browser. The People page is that page's neighbour, at the server and not under a
  project.
- **Blueprints** (`app/blueprints`). A blueprint may carry a workflow that names capabilities (a
  design lead, an art director) and so the roles a kind of project expects. It carries no
  person, no grant and no provider: those are a server's, and a project taken from a blueprint
  starts with whoever its server already knows.

## Later rounds

Not carded yet. Each waits on the first round.

- The four roles, and exactly what a member may do: file a task, move their own, rank?
- How a person's line reads in a document and on a thread, and who authors the commit of a
  person's save.
- A tailnet peer under another login, before any list exists: today's reach, or none.
- Whose queue a card is in, and notices aimed at a named person.
- Agents: sponsors, a shared board's credentials, and review across people.
- What a plugin is told about the person who asked.
- Removing a person: their open asks, their unanswered cards, their agents' sessions.
