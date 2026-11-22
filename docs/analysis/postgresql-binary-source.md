# Where the board's PostgreSQL comes from

The board's store is moving from SurrealDB to PostgreSQL, and a machine that runs a board needs a
PostgreSQL to start. This page says what each place it could come from publishes, what that costs
here and in a consuming project, and which is recommended. The choice is the owner's, on
[its card](#postgresql-binary-source). Every fact below was looked up at its
source on 2026-11-15 ([sources](#sources)) or measured that day on this machine (an Apple M3 Max,
16 cores, macOS 26.6.2, at a one-minute load of 50 to 75); [what was not
verified](#not-verified) is listed apart. Sizes are in MB of a million bytes.

## What the options are measured against

SurrealDB's makers publish one ready program for each platform, and the board takes it as it is
(`tools/surreal.sh`, and the same fetch as one line the board prints in a project where it is
missing, `surreal_fetch` in `crates/tasks/src/server.rs`):

- **Whose build:** the publisher's own, from its own download host.
- **Pinned and checked:** one version, and for each of four platforms (macOS and Linux, arm64 and
  x86-64) the SHA-256 the publisher prints beside the tarball. A tarball that does not match is
  not installed.
- **Size:** a 91.1 MB download for an Apple Silicon Mac, one file of 293 MB once unpacked.
- **Needs:** curl, tar and a SHA-256 tool. Nothing is compiled, and nobody is asked for a
  password.
- **Where:** once per machine, in the git common dir, shared by every worktree.

## What the PostgreSQL project publishes

- **Source code, and only that, for every platform.** The current release is 18.7 (files dated
  2026-11-09, announced 2026-11-12): `postgresql-18.7.tar.bz2`, 23.2 MB, with its SHA-256 and MD5
  beside it and no signature file. Its licence is the PostgreSQL Licence, which the project
  describes as similar to the BSD and MIT licences.
- **No program for a Mac.** Its macOS page points to five things made by others: an installer
  hosted by the company EDB, Postgres.app, Homebrew, MacPorts and Fink. Its file browser holds no
  folder of built programs.
- **For Linux, packages for the system's package manager.** The project keeps an apt repository
  (Debian 12 and 13, Ubuntu 22.04 to 26.10; x86-64 and arm64, arm64 on Ubuntu's long-term
  releases only) and a yum repository for Red Hat, Rocky and AlmaLinux. For any other Linux it
  says to use the distribution's packages or build from source. It publishes no archive that
  unpacks into a folder.
- **Releases.** A minor release at least every quarter (the next on 2027-02-11), each replacing
  the programs and keeping the data. A major release once a year, whose data folder the previous
  major cannot read without a dump and reload or `pg_upgrade`. Version 19 is at its second
  release candidate and planned for 2026-11-19.

So nothing the project publishes can be fetched and started the way SurrealDB is. What follows are
the three ways left.

## The options

### A, a ready-made build from a third party

The one bundle found that covers both systems and both processors with a checksum beside each
archive is `theseus-rs/postgresql-binaries` on GitHub.

- **What it is.** PostgreSQL built from the project's source at the release's tag, with the
  libraries it needs copied in so the folder runs from anywhere. Its release 18.7.0 came out on
  2026-11-12, the day PostgreSQL announced 18.7 and three days after the source files' date; its
  18.6.0 came four days after that source's.
- **Who makes it.** Its own policy calls it a community-maintained distribution and not a vendor
  support contract. Of the 100 contributions GitHub lists for it, 96 are one person's. The repository dates from January
  2024.
- **Pinned and checked.** As today: a version and a SHA-256 per platform in the fetch. Each
  archive has its SHA-256 beside it, a list of every file's hash, and a GitHub attestation that
  the archive was made by that repository's release workflow. Its policy says a tag is never
  reused and a file never replaced. The attestation says where the archive was built, not that
  the build can be repeated or that anyone read it.
- **Size.** Apple Silicon Mac: an 89.0 MB download, 283 MB and 3,868 files unpacked (fetched and
  measured, its checksum matching, nothing in it run). Of that, 134 MB is the LLVM library, 34 MB
  ICU's data and about 30 MB a copy of Python. Linux: 40.5 MB (x86-64) and 39.9 MB (arm64) to
  download.
- **What comes with it.** Twelve packages taken from Homebrew when the archive was built, OpenSSL
  among them, each fixed until the bundle's maker publishes a new revision. Their licences are
  their own: the archive's `psql` uses GNU Readline, under the GPL version 3. The board would
  start the server, not ship the archive, so this is a note and not an obstacle.
- **Where it runs.** macOS 15 or later; on Linux, glibc 2.36 or later (its baseline is Debian
  12.4), with a second set of archives for musl. Older systems are outside what it tests.
- **Work.** The least: `tools/surreal.sh` and the line the board prints, with other names and a
  folder where one file is today. A consuming project's machine fetches it the same way.

### B, the PostgreSQL project's source, compiled once on each machine

- **What it is.** The fetch downloads `postgresql-18.7.tar.bz2` from the PostgreSQL project,
  checks it against the pinned SHA-256 the project publishes, compiles it and installs it in the
  folder the board uses. No third party is involved.
- **Tried here.** With only what a Mac's command line developer tools hold (the compiler, make,
  Bison 2.3, Flex, Perl) and ICU left out: about 6 minutes of CPU (configure 38 s, make 322 s,
  install 9 s), 4 minutes 33 seconds on the clock with eight jobs. The result is 34.5 MB, and its
  server uses no library but the system's own two. A database made with it started on a Unix
  socket alone, with no network port, answered a full-text query and stopped. The build's working
  folder, 227 MB, is deleted afterwards.
- **Pinned and checked.** The source is pinned by the SHA-256 beside it. The compiled program is
  not the same bytes on two machines, so there is no checksum of the result: what is checked is
  what went in.
- **Needs on every machine.** A C compiler, GNU make, Bison, Flex, Perl, tar and bzip2. A Mac
  with the developer tools has all of them. A Linux machine has them only once its build packages
  are installed, which the client build already asks for in part (a C/C++ toolchain) and not in
  full (Bison and Flex).
- **Time.** Those minutes are paid once per machine and again for each new version, at most a few
  times a year; every worktree shares the result. The first test run on a new machine waits for
  it.
- **It stays where it was built.** A build made this way stops working when its folder is moved:
  its tools look for their library at the path it was installed to (tried: moved, `initdb`
  failed; moved back, it ran). The fetch builds it in its final place, so this matters only when
  a repository's folder is moved or copied, which then builds again.
- **What can go wrong that cannot today.** A compile can fail where a download cannot: on a
  machine missing a build tool, or after a system update. The third-party bundle carries patches
  for exactly that on macOS for PostgreSQL 14 to 17, and none for 18. The remedy is then a
  newer PostgreSQL release, or a patch kept here.
- **Work.** More than A: the fetch grows a build step, and a consuming project needs it as an
  installed script or command, since a build does not fit the one line the board prints today.

### C, the PostgreSQL the machine's package manager installed

- **What it is.** Symphony fetches nothing. It looks for `postgres` where Homebrew or the system
  puts it, or where a setting names it, and refuses to start a board on a version too old.
- **On a Mac.** `brew install postgresql@18`: Homebrew builds from the same source tarball, by
  the same SHA-256, and ships it ready for Apple Silicon Macs and for Linux. There is no ready
  build for an Intel Mac, where Homebrew compiles it. It brings seven other Homebrew packages, is
  tied to Homebrew's own folder, and is not put on the search path. This machine has Homebrew and
  no PostgreSQL.
- **On Linux.** The distribution's package or the project's apt or yum repository, installed with
  the administrator's password.
- **Pinned and checked.** Not by Symphony. The formula holds one minor release at a time and
  moves when Homebrew does, so two machines can run different minors, and a machine's PostgreSQL
  changes when its owner updates other software. The major version stays 18 as long as the
  package asked for is `postgresql@18`.
- **Who installs it.** A person, once on each machine: here, and in every consuming project's
  machine. Tests on a machine without it fail until someone does.
- **Work.** The least of the three in Symphony, and the most outside it.

### Sources looked at and left out

| source | why not |
|---|---|
| EDB's archive of its installer's files, the one the PostgreSQL macOS page links for inclusion in another application | macOS and Windows only for version 18 (its Linux archives stop at version 10); a 459.8 MB download; no checksum on its page |
| Postgres.app | macOS only; a 122.6 MB disk image holding an application, with PostGIS; its checksums are GitHub's own digests of the files |
| `zonkyio/embedded-postgres-binaries` | describes itself as intended for testing; its macOS archive is EDB's, repacked; published inside Java archives on Maven Central; its newest is 18.6, with no 18.7 three days after that release |
| The official Docker image | Linux only, and needs a container runtime on every machine |
| MacPorts and Fink | listed by the PostgreSQL project; not examined, as each is a package manager like Homebrew and this machine has neither |
| A bundle Symphony builds and publishes itself | needs a builder for each of four platforms at every release; this repository has none but this Mac |

- [r] **postgresql-binary-source** — The board is moving from SurrealDB to PostgreSQL, so every machine that runs a board needs the PostgreSQL program. SurrealDB's makers publish a ready program, which Symphony downloads once and checks. The PostgreSQL project publishes only source code: for a Mac it points to other people's installers, and for Linux to the system's packages. Nothing on the board's pages changes under any option. Where should the program come from? A: a ready-made build from a community project on GitHub, downloaded once and checked, as SurrealDB is today. B: PostgreSQL's own source code from postgresql.org, downloaded, checked and compiled once on each machine. C: whatever PostgreSQL is installed on the machine (Homebrew on a Mac, the system's packages on Linux), which a person installs. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/postgresql-binary-source#The options]]
  [2026-11-15 12:05] agent — Posted for the board task `docs/postgresql-binary-source-card`. Each source was read today, and B was tried on this Mac; the pages read are listed in the analysis.

    **What each costs.**
    - **A:** the least work, and it installs in seconds: today's download with other names. The build is made by a project that is one person in practice (96 of its 100 listed contributions) and calls itself community-maintained, not a vendor. For this Mac it is an 89 MB download and 283 MB on disk, most of it libraries the board never uses. It has followed each PostgreSQL release within four days, the two times checked. It needs macOS 15, or a Linux as new as Debian 12.
    - **B:** nobody's build but PostgreSQL's own source, checked against the checksum PostgreSQL publishes. Tried here: about 6 minutes of CPU (4 and a half on the clock), once per machine and again for each new version, a few times a year. The result is 35 MB from a 23 MB download, and it started, answered a text search and stopped. It needs a compiler and four build tools on every machine: a Mac with the developer tools has them, a Linux machine needs them installed. A compile can fail where a download cannot, after a system update say. It is more to write than A, and a consuming project needs it as an installed command where today the board prints one line.
    - **C:** nothing for Symphony to download or build. You install PostgreSQL once on this Mac (`brew install postgresql@18`), and so does whoever sets up each other machine; on Linux that takes the administrator's password. The version is then the machine's and not Symphony's: it moves when Homebrew or the system updates, two machines can differ, and tests fail on a machine where nobody installed it.

    **Not verified.** B was not tried on Linux or on an Intel Mac: no such machine was at hand. A's build was downloaded and measured, not run. Whether the board will need a library the trial left out (ICU, for sorting text by language) is not known until its PostgreSQL schema is written; if it does, B grows a second thing to compile.

    **Why B.** It is the only option that keeps what you have today: the program comes from the people who write it, at a version Symphony names, checked before use. It is also the smallest, an eighth of A on disk. Its price is minutes once per machine and a build step to keep working. A is the fallback if that build proves a burden. C gives up knowing that a test, a consuming project and this machine run the same database.

    **Earlier verdicts checked.** Your approval of PostgreSQL for the board (2026-11-09, recorded on the board task `tools/board-postgresql`) chose the database and that Symphony's server starts it; it does not say whose build of it a machine runs. No card in this register speaks of where a database program comes from.
  [2026-11-15 12:32] owner · approved — C. This project should only depend on it being available, not manage it's installation for now. In the future, some build based aspect can be added, including:
    1. Devcontainer - user by the maestro project
    2. Dockefile - very commonly used. Should ideally be the same of the devcontainer since it supports docker anyway
    3. Nix based - interesting for microvm post start install

    And those can be documented to be further explored later. For now however, a computer based install using whatever is available should be setup, by having an agent do it and add settings to connect to the symphony.toml settings
  [2026-11-15 12:33] owner · approved — Ah, and do use the latest version locally in homebrew, update it if needed.
  [2026-11-18 18:12] agent — C is built. The board takes the PostgreSQL the machine has and installs none: it finds `postgres` on `PATH`, or in the folder `[board] postgresql` names in `symphony.toml`, and asks for 18 or later, the Homebrew version on this Mac (`tools/board-pg-server`, `tools/postgresql-setup-portable`). The setup line is in [PROJECT_FILE](../PROJECT_FILE.md#board). A devcontainer, a Dockerfile and Nix are not built and no task is filed for them.
  [2026-11-19 01:15] agent · resolved

## Side by side

| | today, SurrealDB | A, third-party bundle | B, source, compiled | C, package manager |
|---|---|---|---|---|
| whose build | the publisher's | one maintainer's project | this machine's, from the project's source | Homebrew's or the distribution's |
| version pinned | yes | yes | yes | major only |
| checked against | publisher's SHA-256 | the bundle's SHA-256 | the project's SHA-256 of the source | the package manager's own checks |
| download, Apple Silicon Mac | 91.1 MB | 89.0 MB | 23.2 MB | not looked up |
| on disk | 293 MB | 283 MB | 34.5 MB | not looked up |
| first use on a machine | seconds | seconds | about 6 minutes of CPU | a person installs it |
| needs | curl, tar | curl, tar | a compiler, make, Bison, Flex, Perl | Homebrew, or an administrator on Linux |
| a consuming project's machine | one printed line | one printed line | an installed script or command | its owner installs PostgreSQL |
| a new PostgreSQL release reaches it | n/a | when the bundle follows (3 and 4 days after the source, the two checked) and the pin moves | when the pin moves | when the machine updates |

## Recommendation

**B.** The source is the only thing the PostgreSQL project itself publishes and vouches for, and
it is the only option that keeps what holds today: the program comes from its makers, at a pinned
version, checked before use. It is also the smallest by far, a 23 MB download and 35 MB on disk
against 89 and 283, because it holds only what the board uses. Its price is about six minutes of
CPU once per machine and version, tools a Mac set up for this repository already has, and a
build step to write and keep working.

A is the fallback if owning a build proves a burden: it is the least work and installs in
seconds, at the cost of running a database built by one person's project, with eight times the
disk and libraries the board never calls. C gives up the pin, which is what lets a test, a
consuming project and this machine know they run the same database.

## Not verified

- **B on Linux.** No Linux machine was at hand. That it builds there with the listed tools is the
  PostgreSQL documentation's word, not a trial.
- **B on an Intel Mac**, and on a macOS older than this one.
- **Whether the board needs ICU.** The trial left it out rather than add a library to fetch,
  and full-text search worked without it. The board's PostgreSQL schema is not written yet;
  if it needs ICU's collations, B grows a second source to fetch and compile, or the choice is
  looked at again.
- **A's archives were not run**, on any platform, and the Linux ones were not downloaded: their
  sizes are GitHub's listing. The attestation was not checked with GitHub's tool.
- **Whether EDB's macOS archive holds both processors' programs.** Its page lists both for the
  installer; the archive was not downloaded.
- **Homebrew's download and installed size**, and what a Linux package of PostgreSQL starts on
  its own when installed.
- **How often a system update breaks a source build.** The bundle's patches show it has happened
  for older majors; there is no count.

## Sources

Read on 2026-11-15.

- PostgreSQL downloads: <https://www.postgresql.org/download/>,
  <https://www.postgresql.org/download/macosx/>, <https://www.postgresql.org/download/linux/>,
  <https://www.postgresql.org/download/linux/debian/>,
  <https://www.postgresql.org/download/linux/ubuntu/>,
  <https://www.postgresql.org/download/linux/redhat/>
- PostgreSQL's files: <https://www.postgresql.org/ftp/>,
  <https://www.postgresql.org/ftp/source/v18.7/>,
  <https://ftp.postgresql.org/pub/source/v18.7/postgresql-18.7.tar.bz2.sha256>
- PostgreSQL's licence, versions and schedule: <https://www.postgresql.org/about/licence/>,
  <https://www.postgresql.org/support/versioning/>,
  <https://www.postgresql.org/developer/roadmap/>
- Building from source: <https://www.postgresql.org/docs/18/install-requirements.html>,
  <https://www.postgresql.org/docs/18/install-make.html>
- The third-party bundle: <https://github.com/theseus-rs/postgresql-binaries>, its
  `docs/support-policy.md`, `docs/provenance.md`, `scripts/build-macos.sh` and `LICENSE`, its
  release 18.7.0 and that release's `aarch64-apple-darwin` archive
- EDB's archives: <https://www.enterprisedb.com/download-postgresql-binaries>
- Postgres.app: <https://postgresapp.com/downloads.html> and its release v2.9.7 on GitHub
- The testing bundles: <https://github.com/zonkyio/embedded-postgres-binaries>, its
  `scripts/repack-postgres.sh`, and
  <https://repo1.maven.org/maven2/io/zonky/test/postgres/embedded-postgres-binaries-darwin-arm64v8/>
- Homebrew's formula: <https://formulae.brew.sh/api/formula/postgresql@18.json>
- The Docker image's tags: <https://hub.docker.com/_/postgres>
- SurrealDB's tarball, for its size: `tools/surreal.sh` names it
