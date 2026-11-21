# Writing a plugin and a theme

A project's own code reaches its Symphony as a plugin, and its own look as a theme. This page
writes a first plugin from nothing and ends with its component drawn on a page, then gives a
project a theme, with and without a plugin. What a plugin's process and the host say to each other,
and every limit and failure of it, is the reference, [PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md).

A plugin is one of two kinds. **One that answers from a process of its own** is a program the
project builds, which the server starts and asks over its stdin and stdout. This is the kind every
installed release serves, and the one this page writes. A linked plugin is compiled into a server
the project builds for itself, so no release's server can serve its project
([the host](PLUGIN_PROTOCOL.md#the-host)). Both implement the same traits, so a plugin written
here can be linked later without a change to its code.

It needs a project the Symphony already serves (`symphony init` run, as
[the README](../README.md#on-your-own-project) says) and a Rust toolchain. The plugin depends on
one crate from crates.io, `symphony-plugin`, and on nothing else of the Symphony.

## The plugin

### 1. The crate

In the project's checkout, make a binary crate. It may live anywhere; here it is `plugins/hello`:

```sh
cargo new --bin --vcs none plugins/hello
```

`--vcs none` keeps Cargo from making a repository inside the project's. Its
`plugins/hello/Cargo.toml` names the kit:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2024"

[dependencies]
symphony-plugin = "1"
```

`symphony-plugin` brings the two other crates of the kit with it: the blocks a plugin renders to
(`symphony_plugin::vocabulary`) and the docs checks ([the kit](PLUGIN_PROTOCOL.md#the-kit)). A
release that breaks a name the kit exports is a new major version, so `"1"` keeps building.

### 2. The code

A plugin is a value implementing `Plugin`: its name, and what it adds at each seam, each seam
adding nothing unless the plugin says otherwise. This one adds a component, a page of its own in
the header and a theme package. Its `main` hands it to `serve`, which answers the host until the
host ends. This is the whole of `plugins/hello/src/main.rs`, tests included:

```rust
//! A first plugin, whole: the `[[hello:NAME]]` component, a page of its own in the header and a
//! theme package, answering from a process of its own. It is a plugin's `src/main.rs` as
//! docs/PLUGIN_WRITING.md walks through it, tests included.

use symphony_plugin::vocabulary::{Block, NavItem, Span};
use symphony_plugin::{Component, Page, Plugin, Theme};

/// The plugin. Its name is the one the project's `[plugins] crates` lists.
struct Hello;

impl Plugin for Hello {
    fn name(&self) -> &str {
        "hello"
    }

    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }

    /// A page of its own: `Hello` in the header, opening `/hello`, which shows a document.
    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: "Hello".into(),
            title: "The hello page".into(),
            href: "/hello".into(),
            document: "docs/HELLO.md".into(),
        }]
    }

    /// A theme package, which the project takes with `[theme] package = "hello"`.
    fn theme(&self) -> Option<Theme> {
        Some(Theme {
            package: "hello".into(),
            name: "Hello".into(),
            tokens: [
                ("primary".to_string(), "#b4491f".to_string()),
                ("radius".to_string(), "0".to_string()),
            ]
            .into(),
            ..Theme::default()
        })
    }
}

/// `[[hello:NAME]]` greets NAME, and `[[hello:NAME|shout]]` does so loudly.
impl Component for Hello {
    fn name(&self) -> &str {
        "hello"
    }

    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        let name = argument.trim();
        if name.is_empty() {
            return Err("hello needs a name: [[hello:NAME]]".into());
        }
        let greeting = match option {
            None => format!("Hello, {name}!"),
            Some("shout") => format!("HELLO, {}!", name.to_uppercase()),
            Some(other) => return Err(format!("hello takes the option shout, not {other:?}")),
        };
        Ok(vec![Block::Paragraph {
            spans: vec![
                Span::Strong {
                    spans: vec![Span::Text { text: greeting }],
                },
                Span::Text {
                    text: format!(" This is {}.", page.doc_path),
                },
            ],
        }])
    }
}

fn main() -> std::process::ExitCode {
    symphony_plugin::serve(&Hello)
}

#[cfg(test)]
mod tests {
    use symphony_plugin::testing::Host;
    use symphony_plugin::{Page, error};

    use super::*;

    /// What `[[hello:ARGUMENT|OPTION]]` shows on `docs/A.md`, asked as a host asks it.
    fn shown(argument: &str, option: Option<&str>) -> Vec<Block> {
        let page = Page::new("/project".as_ref(), "docs/A.md");
        Host::new(&Hello).render(&page, "hello", argument, option)
    }

    fn greeting(text: &str) -> Vec<Block> {
        vec![Block::Paragraph {
            spans: vec![
                Span::Strong {
                    spans: vec![Span::Text { text: text.into() }],
                },
                Span::Text {
                    text: " This is docs/A.md.".into(),
                },
            ],
        }]
    }

    #[test]
    fn it_greets_the_name_it_is_given() {
        assert_eq!(shown("world", None), greeting("Hello, world!"));
        assert_eq!(shown("  world ", None), greeting("Hello, world!"));
        assert_eq!(shown("world", Some("shout")), greeting("HELLO, WORLD!"));
    }

    #[test]
    fn a_problem_is_shown_in_the_components_place() {
        assert_eq!(
            shown(" ", None),
            [error("hello needs a name: [[hello:NAME]]")]
        );
        assert_eq!(
            shown("world", Some("whisper")),
            [error("hello takes the option shout, not \"whisper\"")]
        );
    }

    #[test]
    fn it_declares_its_component_its_page_and_its_theme() {
        let declared = Host::new(&Hello).declaration().unwrap();
        assert_eq!(declared.name, "hello");
        assert_eq!(declared.components, ["hello"]);
        assert_eq!(declared.nav[0].href, "/hello");
        assert_eq!(declared.nav[0].document, "docs/HELLO.md");
        let theme = declared.theme.unwrap();
        assert_eq!(theme.package, "hello");
        assert_eq!(theme.tokens["primary"], "#b4491f");
    }
}
```

The same file is the kit's example, `crates/symphony-plugin/examples/hello.rs` in the Symphony's
repository, which its own build compiles and tests.

What the code relies on:

- **A component renders to blocks, never HTML.** `Block` and `Span` are the vocabulary every
  page is drawn from (headings, paragraphs, lists, tables, code, figures), and the client draws
  each with one component of its own, so a plugin ships no markup, script or style.
- **A problem is an `Err` with one line of text.** The host shows it in the component's place,
  as `⚠ hello takes the option shout, not "whisper"`, and the rest of the page is drawn.
- **`Page` is the render the component is part of**: the project's root (`page.root`), the
  document (`page.doc_path`), the project's artifacts folder (`page.artifact("screens")`), and
  `page.once(key, load)`, which loads a value once for every component of the page.
- **The stdout is the protocol's.** A plugin prints its diagnostics on stderr (`eprintln!`), which
  go to its log.

### 3. Its tests and its build

`testing::Host` asks the plugin as a host does, each call written as the protocol's request and
its answer read back, with no server running. Its `render` gives what a page would show, a problem
included, and its `declaration` what the plugin tells a host it has.

```sh
cargo test --manifest-path plugins/hello/Cargo.toml
cargo build --release --manifest-path plugins/hello/Cargo.toml
```

The build leaves the program at `plugins/hello/target/release/hello`. It is a build product, so
it is not committed: add `plugins/hello/target/` to the project's `.gitignore`, and each machine
that serves the project builds it.

### 4. The project file

In the project's `symphony.toml`, list the plugin by its name and give the command that starts it:

```toml
[plugins]
crates = ["hello"]

[plugins.process]
hello = ["plugins/hello/target/release/hello"]
```

The name is the one `Plugin::name` answers: a plugin that declares another is refused. A path
holding a `/` is read from the project's checkout; an absolute path and a bare name found on
`PATH` are the two other forms, and more words after the program are its arguments
([turning it on](PLUGIN_PROTOCOL.md#turning-it-on)).

### 5. The component in a document

A document uses a component as `[[name:argument]]` or `[[name:argument|option]]`, outside code:
alone in a paragraph, or within a line. The name is lowercase letters and hyphens, and everything
up to the `|` or the closing brackets is the argument, handed to `render` as it is written. Write
`docs/HELLO.md`, the document the plugin's page shows:

```markdown
# Hello

The component, drawn by the plugin:

[[hello:world]]

[[hello:world|shout]]
```

and link it from `docs/README.md`, as every document is.

### 6. Seeing it

```sh
just symphony-status --lint
just symphony
```

The first checks the project: it names a plugin whose process does not start, does not declare or
is refused, and passes when there is none. The second prints the address the project is served
at. There the header has a `Hello` page, which opens `/hello` and shows the document, with
**Hello, world!** and **HELLO, WORLD!** where the two components stand. The document's own
address, `/docs/HELLO.md`, shows the same.

To try it apart from the machine's server, `just symphony --port 0` runs a server of this checkout
alone on a free port, in the foreground, and prints its address.

### When it does not show

- **`just symphony-status --lint`** says why a plugin is not answering, on a line that starts with
  the project file's name:

  ```text
  symphony.toml: the plugin hello: ./plugins/hello/target/release/hello cannot start: No such file or directory (os error 2); it starts again in a moment
  ```

- **The plugin's log** is `artifacts/plugins/hello.log` in the project (`plugins/NAME.log` under
  the artifacts folder `[project] artifacts` names). It holds a stamped line for each start and
  each end of the process, each call that had no answer, and every line the plugin wrote on its
  stderr ([its process](PLUGIN_PROTOCOL.md#its-process)).
- **A `⚠` line where the component should be** is the component's own `Err`, or the host's word
  that the call had no answer, naming the plugin and why.
- **`⚠ unknown component hello`** means no plugin of the project has a component of that name:
  check `[plugins] crates` and what `Component::name` answers.
- **An old answer after a rebuild.** A server keeps the process it started, so a new build of the
  program answers once that process ends: stop the process whose number the log's last `started`
  line gives (`kill NUMBER`) and the next call starts the new build, or restart the server.
- **A branch's checkout** (a linked worktree) starts no process of its own: the main checkout's
  process answers for it, reading the branch's files, so a branch's change to the plugin's code
  shows once it is built in the main checkout.

## What else a plugin adds

Each is a method of `Plugin` returning the objects of one seam, and a trait in `symphony-plugin`
whose documentation says what it is asked:

| Seam | What it adds |
| --- | --- |
| `components` | `[[name:argument\|option]]` in a document |
| `card_kinds` | a kind of decision card it recognises by its text: its pictures, slot and triage category |
| `renderers` | a file type the project's documents folder serves, drawn as blocks or in a viewer |
| `routes` | an answer under `/x/<path>/`: JSON, or a file sent as it is |
| `nav` | entries in the header and the home's links, after the Symphony's own |
| `home_regions` | a region of the home page |
| `board_actions` | a button on a task, and what its submission does |
| `watches` | paths whose change drops what the plugin cached |
| `doc_checks` | a check over the project's documents, run by `just symphony-status --lint` |
| `commands` | a command of the project's `symphony`, run as `just symphony-cli NAME` ([its commands](PLUGIN_PROTOCOL.md#its-commands)) |
| `authorities` | an approval authority of the project's workflow ([approval authorities](PLUGIN_PROTOCOL.md#approval-authorities)) |
| `theme` | a theme package, below |

Five seams are a linked plugin's alone: board fields, gates, prose rules, generators and
transports. A plugin that declares one of them is refused under `[plugins.process]`, by name.

## A theme

A theme is a name, an icon, tokens, stylesheets and layout variants. A project sets any of them in
its `symphony.toml` with no plugin at all, or takes a package a plugin brings and sets parts over
it ([`[theme]`](PROJECT_FILE.md#theme) lists the keys).

### Its tokens

A token is a CSS custom property the Symphony's own stylesheets read: `primary = "#0f766e"`
declares `--primary` on every page of the project. A value is any CSS value without `;`, braces,
angle brackets, a backslash, a comment, `url(` or `@import`. A token set by a theme holds in the
light and the dark scheme alike, so a theme that sets `background` sets `foreground`, `card` and
`border` with it.

| Token | What it colours or sets |
| --- | --- |
| `background` | the page behind everything, and the header over it |
| `foreground` | text |
| `muted-foreground` | secondary text: paths, captions, counts, the breadcrumbs |
| `card` | the surface of a card, a panel, a button and a document's body |
| `border` | the lines around and between them |
| `primary` | links, the current page in the header, the filled button, the focus ring and the Symphony's mark |
| `primary-foreground` | text on the filled button |
| `muted` | a faint fill: code, table heads, counts |
| `accent` | a faint tint of the primary: the owner's lines in a thread, the home's next card |
| `like`, `dislike`, `question`, `decision`, `resolved`, `pending`, `auto` | the states of a card, a comment and a task's column: approved or done, rejected, asked or in review, a decision or an agent's line, settled, not started, ready |
| `live`, `stale`, `lost` | an agent's health |
| `radius` | the corners of buttons, fields and cards, which the larger and smaller corners follow |
| `font-sans`, `font-mono` | the text's font and the code's |
| `shadow-1`, `shadow-2` | the shadow under a card and under a panel |
| `measure` | the widest a line of a document's text runs (`72ch`) |

A name outside this list is declared all the same, for a stylesheet of the project's own to read.

### In the project file alone

```toml
[theme]
name = "Scratch & Co"

[theme.tokens]
primary = "#0f766e"
background = "#f6f3ea"
card = "#fffdf7"
foreground = "#1f2a28"
border = "#d8d2c2"
font-sans = "Georgia, serif"
radius = "0.125rem"
```

The name takes the Symphony's place in the header and the tab, and the pages take the colours, the
font and the corners at their next load, with no restart. `icon` names a picture in the project
for the header and the tab, `stylesheet` a stylesheet of the project's served after the tokens,
and `[theme.layouts]` a variant for a region: `header = "compact"`, `home = "focus"`.

### From a plugin

A plugin's `theme` returns a package, as the example's does: its name `hello`, the name it shows
and two tokens. The project takes it by name, and may still set any part over it:

```toml
[theme]
package = "hello"

[theme.tokens]
radius = "0.5rem"
```

A package is the way to give several projects one look: each lists the plugin and names the
package. `just symphony-status --lint` names a package none of the project's plugins brings
(`[theme] package nope is shipped by none of the project's plugins`), and a package's token or
layout the client cannot read, which is left out.
