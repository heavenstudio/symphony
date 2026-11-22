//! A first plugin, whole: the `[[hello:NAME]]` component, a page of its own in the header and a
//! theme package, answering from a process of its own. It is a plugin's `src/main.rs` as
//! docs/engineering/PLUGIN_WRITING.md walks through it, tests included.

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
