# Users and identity

Several people on one board. Who each one is, is proved outside the Symphony's core by a service
that signs them in; what each may do is the Symphony's own, kept with the board: the groups that
own what waits to be decided, and the powers to speak, decide and direct agents. This is
a concept: it questions the idea against what the Symphony has, says the shape that fits, and
puts its choices to the owner a few at a time ([this round](#this-round)). Nothing here is built,
and nothing is built from it but by a task filed from an approved card.

Read at `59538b83e`. The links name the modules and documents read.

## The answer in short

- **The Symphony has a place where the owner sits, and no person.** Every browser the server
  admits writes as the one sender `owner`. Who that browser belongs to is never read
  ([the picture](#what-the-symphony-has-today)).
- **People are a board's.** Who is on a board, in which groups and with what powers is kept with
  that board and in no repository, so two boards can hold different people
  ([its card](../DECISIONS.md#people-kept-where)). What a document carries of a person is a short
  public name.
- **Signing in and what a person may do are two things, kept apart.** Authentication is a service
  outside the core. Authorization is the board's ([the picture](#the-shape-proposed)).
- **A group owns what waits to be decided.** A card or a task belongs to a group, or to one
  person. Its members are told, and any one of them settles it
  ([decided](#who-in-a-group-decides)).
- **Three things one owner does today come apart.** Speaking to people, deciding, and directing
  agents are held separately. Anyone on a board speaks to anyone, anywhere. The words of someone
  who may not direct agents reach no agent until someone who may passes them on
  ([decided](#words-without-agent-reach)).
- **Agents are personal or shared.** A personal agent runs on its person's machine and credits
  and answers only them. A shared agent is the board's, and works for whoever may direct agents.
- **The core signs nobody in.** A sign-in service, a plugin of the server and never of a project,
  does that, with OAuth 2, and hands the Symphony signed data saying who the person is. The core
  only verifies it ([how](#how-a-provider-plugs-in)). The Symphony ships one such service, and
  any other that signs the same way replaces it.
- **A machine with one person changes in nothing.** With nobody named on a board, the person at
  the machine holds everything there, as today.

## What the Symphony has today

![Today: everyone the gate lets in writes as the one owner](users-and-identity/today.svg)

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
| What the owner says to an agent | Every word of the owner's that waits on an agent is an ask the board holds until an agent answers: a verdict with a note, a message on a card, a comment in a document, a message on a thread. It is delivered to a listening agent at once. | [TASKS](../TASKS.md#asks) |
| A board | One for a project, in its repository's git common dir, or hosted in a PostgreSQL of the platform's. | [TASKS](../TASKS.md#where-it-lives), [POSTGRESQL](../POSTGRESQL.md#a-hosted-board) |

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
([its card](../DECISIONS.md#identity-provider-how-added), B). The owner said how its word reaches
the core when rejecting [another card](../DECISIONS.md#group-card-who-decides): "symphony just
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
  ([hosted ingress](hosted-ingress.md#identity-and-the-trusted-proxy-boundary)).
- **The core verifies and nothing more.** The signature, against a key the server's own settings
  name; that the data is fresh; that it was made for this server. Unsigned, stale or made for
  another server, the request is refused. The core draws no sign-in page and holds no password
  and no client secret.
- **It is the server's plugin, never a project's.** The key and the service are named where the
  server is started, for [the reasons above](#an-identity-provider-as-a-plugin). A project's
  `symphony.toml` names neither.
- **Nothing in the signed data gives a power.** A group or a role a company's sign-in knows of
  is not read as one of the board's. Whether a board may ever fill its groups from such data is
  a [later round's](#later-rounds).
- **On a machine, Tailscale is the service.** It signed the person in to the tailnet, and the
  machine's own daemon says whose node an address is. That word comes from the daemon on the
  same machine and not as a signed header: the one place the core asks instead of verifying, as
  the gate does today. A request from the machine itself is the machine's person.

### What the Symphony ships for signing in

The core's check, and a sign-in service of its own
([its card](../DECISIONS.md#sign-in-service-shipped), B).

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

### Where people are kept

With the board, and in no repository ([its card](../DECISIONS.md#people-kept-where)).

- **The list is a board's own.** Its people, their groups and their powers are rows of that
  board, beside its tasks: in the git common dir's database on a machine, in the hosted
  database on a hosted board. An admin of the board changes them from a People page or a
  command, and the board keeps each change in a log.
- **Two boards differ.** A person can decide on one board and only read another, of the same
  project or not. The server keeps no list of people: it asks each board about the person the
  sign-in service named.
- **A clone holds nobody's sign-in.** A service's subject for a person never leaves the board.
- **The public name is the part that is not the board's alone.** It is written where the record
  is: on a card's line and in a comment, which are files in git, and on a thread. So it outlives
  the board's row for the person, and it is read in every clone and by every board of the
  project. That asks two things of it: one name means one person for the whole project, and a
  name once used is never given to another. Where a project declares its names so that every
  board reads them alike is a [later round's](#later-rounds).
- **With nobody named** on a board, the person at the machine holds every power there, as today.
  The first person named is the first moment anything differs.

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
| speak | write to people anywhere: a comment in a document, a message on a card, a message on a task's thread | the owner |
| decide | resolve what their group owns: a card's verdict, the approve that is the `owner` stamp | the owner |
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
([its card](../DECISIONS.md#revision-bound-project-guards)).

### Who in a group decides

Any one member ([its card](../DECISIONS.md#group-answer-settles), A). This is authorization
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
  to, naming the tip, under that member's name.
- **An approved card decides a later one** whoever of its group approved it. An agent checking
  earlier verdicts reads the verdict, not who gave it.
- **A line no commit has yet** keeps its worktree from being removed, whoever's line it is.
- **Two members who disagree** override each other in turn, and nothing stops them but the
  history showing it. A decision that must be several people's stays a workflow guard's
  ([its card](../DECISIONS.md#revision-bound-project-guards)); one that must be one person's is
  a thing owned by that person, not by a group.
- **An agent already acting** on an answer when another member overrides it is where it is today
  when the owner changes their mind: the later word is a new ask.

A group of one is a single owner, so a machine with one person sees none of this.

### Which group a thing belongs to

The place it is in, unless a name is written on it
([its card](../DECISIONS.md#owning-group-how-named), C).

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
  same folder to different groups.
- **A name written on a card is in the file**, so a group's name is public as a person's is. A
  board with no group of that name treats the card as unnamed and falls back to the map.
- **An agent posting a card writes nothing** in the usual case, and names a group only when the
  question is not its document's.

### Words without agent reach

Nothing reaches an agent until someone passes it on
([its card](../DECISIONS.md#words-without-agent-reach), A). This is about a person who may speak
and may not direct agents. A person who may direct agents writes as the owner does today.

- **Their words are for people.** They show on the page to everyone who may read there, and the
  people concerned are told.
- **The board keeps them.** A comment of theirs is not written into the document's file, and
  their message on a task's thread is a row agents are not shown. No ask is opened, so no agent
  is handed them.
- **Passing on** is one press by someone who may direct agents. From then the words are treated
  as that person's own are: written into the file or shown on the thread, and an ask an agent
  answers. The line carries both names, whose words they are and who passed them on.

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
  moves agents by what they decide. Whether such a person's note beside a verdict is held like
  their other words is a [later round's](#later-rounds).

## This round

Two choices, each a card. A person is more than a name on a line, so something must say where
their profile is kept. And on a machine Tailscale is the sign-in service, so something must say
what a machine under another person's login may do before a board names them.

### Where public names are declared

The short name is one field of something larger. The owner, rejecting
[the card that asked this](../DECISIONS.md#public-name-where-declared): "We will probably have a
mapping of names / emails / git handles / communication usernames and preferences (for example
slack user). So this may be become more complex then proposed". So the question is not where a
list of names is declared, but where a person's whole record is kept
([below](#a-persons-profile)).

### A person's profile

**Base.** No person is recorded. The one name is `owner`, fixed in the code.

A person's profile is what the Symphony knows of who they are, apart from what they may do:

| Field | What it is for | Who may see it |
| --- | --- | --- |
| short name (`ana`) | the name on every line they write, on a card, in a comment, on a thread | everyone; it is in git |
| display name | how pages show them | everyone on the board |
| sign-ins | which sign-in service's subject is this person; there can be several | the board's admins |
| emails | a notice by mail; matching a commit's author to a person | the person and admins |
| git handles | whose commit a save of theirs is authored as; whose commits are theirs | everyone; they are in git already |
| chat usernames (Slack) | where a notice is sent | the person and admins |
| preferences | which notices, by which way | the person |

What a person may do (their groups, their powers) is not in the profile. It is each board's
([its card](../DECISIONS.md#people-kept-where)).

The choice is where the profile is kept and who writes it.

![Where a person's profile is kept](users-and-identity/profile-where.svg)

- **A, with each board.** A board keeps a whole profile beside each person's groups and powers.
  One store and one page. A person on three boards has three profiles, and a Slack name changed
  is changed on each. A board's admin writes it.
- **B, once, with the server.** The server keeps one profile for each person: in the machine
  folder on a machine, in the platform's own store on a hosted board. The person writes their
  own, and an admin can. A board keeps only that person's groups and powers, and refers to the
  profile. Who someone is, is kept apart from what they may do, as signing in is.
- **C, and its public part in git.** As B, and the part that is public anyway (the short name,
  the display name, the git handles) is also a committed file of the project. Every clone then
  reads whose lines are whose with no server, and two servers of one project cannot give one
  name to two people. A new person is a landing as well as a row.

Under every option a line carries the short name and nothing else, a name once used is never
given to another, emails and chat usernames are in no repository, and no agent or plugin is
handed them. The lines already written as `owner` stay, and a machine with nobody named keeps
writing as `owner`.

**Recommended: B.** A profile is the person's, and they are one person on every board the
server holds. It is also the same line the owner drew for signing in: who someone is on one
side, what they may do on the other. What B leaves open is C's reason: a project served from two
servers can give `ana` to two people. C's file answers that when a project is in fact served
from two places, and nothing in B is lost by adding it then.

### A tailnet machine under another login

**Base.** Any machine the tailnet answers for reaches the server and writes as the owner: the
owner's phone, and equally a colleague's laptop or a machine shared into the tailnet. Whose
machine it is, is in Tailscale's answer and is not read
([the picture](#what-the-symphony-has-today)).

Once the login is read, a machine under the Mac's own login is its person, as today. The choice
is what a machine under another login may do before a board has named that person.

![A tailnet machine under someone else's login](users-and-identity/tailnet-peer.svg)

- **A, as today.** It keeps writing as the owner until a board names its first person. Nothing
  changes for anyone until then, and what is open today stays open.
- **B, nothing until named.** It is refused, with a page saying whose machine the server is and
  that a board must name them. A board that names that login's person lets them in, to that
  board.
- **C, reads until named.** It reads every board the server holds and writes nothing.

A machine with no login (a tagged node) is treated as the other login's machine is. With nobody
else in the tailnet the three are the same.

**Recommended: B.** A leaves a colleague's laptop deciding the owner's cards for as long as
nobody is named, which on a one-person machine is always. C shows every project on the machine
to the whole tailnet, which a board's own list was meant to decide. B changes nothing for one
person and their own machines, and it is the only one of the three under which being in a
tailnet gives nothing by itself.

## Sessions

- **On a machine's server**, none are added. A request from the machine is the machine's
  person. A request from the tailnet is the login Tailscale names for its address, asked as
  today and kept for the 30 seconds the gate already keeps it. A peer under the machine's own
  login is the same person, so a phone keeps working with nothing set up. A peer under another
  login is that person, admitted to a board only once it names them. A tagged node has no login
  and is admitted only where a board names the node.
- **On a hosted board**, the session is the sign-in service's. The adapter verifies the signed header on
  every request, ends a stream when the assertion expires, and binds its request-forgery token
  to the session and the project, all as [hosted ingress](hosted-ingress.md#browser-behavior)
  sets out. The Symphony stores no session.

## Agents beside people

An agent stays an agent: a principal of its own kind, which no claim turns into a person, so it
never decides a card, gives the `owner` stamp or takes an edge that waits for a person. With
people on a board there are two kinds:

- **A personal agent** is one a person starts on their own machine, under their own credits.
  Only that person directs it. On the board it may do no more than they may: a reader's agent
  reads. Others see what it posts and speak under it; their words reach its person, not the
  agent.
- **A shared agent** is the board's: the orchestrators, executors and reviewers staffed for its
  tasks. It runs on a host, a machine and credits somebody offers the board. Anyone who may
  direct agents directs it, whoever hosts it. It keeps two names apart: the person whose word it
  is acting on, and its host. It does no more than the person whose word it acts on may. The
  execution design already keeps such a pair, as the acting principal and the initiator.

On one machine with one person every agent is both at once: started by them, on their machine,
directed by them. Nothing changes there.

Left for a later round: who hosts a board's shared agents and what a host may limit, how an
agent on another machine proves its handle to a board (a credential a person issues, as the
hosted execution design expects of its host), and whether a review must come from an agent
another person directed. Today's rule asks for a fresh session, not another person.

## The way from one owner

Each step can ship alone, and a machine with one person sees none of them.

1. **The actor seam.** Every write takes an actor. The local host supplies the local owner, and
   every line reads as it does today.
2. **Tailscale's login read.** The gate keeps who a peer is. A peer under the machine's own
   login is its person. What a peer under another login may do before a board names them is
   [asked](#a-tailnet-machine-under-another-login).
3. **People, groups and powers on a board.** The rows, the People page, names on lines.
4. **Ownership.** A card and a task have their group, from the board's map of places or a name
   written on them; queues and notices follow it, and any member's answer settles what the
   group owns.
5. **Speaking apart from directing agents.** A person's words reach people always. The board
   holds the words of someone who may not direct agents, and passing them on makes them an ask.
6. **The signed word verified.** The core's check of a sign-in service's signed data, and hosted
   saves through the same writes as local ones. Then the Symphony's own sign-in service, a
   separate program in front.
7. **Shared agents' hosts**, then a board's agent credentials.

With nobody named on a board, the person at the machine holds every power there and is its
admin. The first person named is the first moment anything differs.

## The other two concepts

- **A settings page** (`app/settings-page`). A saved change to the project file is a landing
  today, filed as the owner's task. With people it is filed as the task of the person who saved,
  and needs what any change of that file needs. A board's people, groups and powers are that
  page's neighbour: a People page of the board, for its admins, which changes rows of the board
  and no file. A setting that holds a command the server runs should not be anyone's to set from
  another machine at all: that would be running programs on the machine from a browser.
- **Blueprints** (`app/blueprints`). A blueprint may carry a workflow that names capabilities (a
  design lead, an art director) and the groups a kind of project expects (engineering, design).
  It carries no person and no sign-in service: people are a board's, and a project taken from a
  blueprint starts with its groups empty but for whoever made it.

## Later rounds

Not carded yet.

- What each power covers exactly, and the task board's acts: who files a task, moves, ranks or
  cancels one.
- A person who decides and may not direct agents: whether the note beside their verdict is held
  like their other words.
- How a person's line reads in a document and on a thread.
- A profile's notices: which a person gets, by which way (mail, Slack), and what a chat plugin
  is handed to send one.
- Queues and notices per person and per group.
- Shared agents: whose machine and credits host them, what a host may limit, and how an agent on
  another machine proves its handle to a board.
- Whether a review must come from an agent another person directed.
- What a plugin is told about the person who asked.
- Whether a board may fill a group from what a sign-in service says of a person (a company's
  own groups), and who says which of them counts.
- Removing a person: their open asks, their unanswered cards, their agents' sessions.
- Several boards for one project: nothing proposed here ties people to a project, and what else
  a second board needs is a concept of its own.
