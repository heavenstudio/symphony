//! Consumer call shapes beyond the kit/helpers semver check: argument and return types, field
//! types, cross-crate type identity and known consumers that must migrate across two releases.
//! Vocabulary inventories wider than known consumer calls are left to `just semver-check`.
//! Known names, including name-only references, remain for the two-release migration rule.
//! The product remains guarded in full.
//!
//! The public names of `symphony`, `symphony_plugin`, `symphony_api`, `symphony_docs`,
//! `symphony_schema` and `symphony_args` that consuming projects' code calls, each in the shape it
//! calls it: a signature pinned by coercing the function to its pointer type, or a call written as
//! a consumer writes it (a few functions, one of them generic, are named alone, their signatures
//! left to the project, below), a struct it builds by its fields built by them all (by those it
//! names, where it fills the rest from `Default`), a variant it builds, compares or matches built,
//! compared or matched as it does, a trait it implements implemented with the methods it writes, a
//! type it names named. Nothing it defines is called but `compared` and
//! `asserted`, which do nothing: it is compiled by the test build and by `just lint-rs` (clippy
//! over every target), so a change that would break a consumer in what this file holds fails a
//! task's gate rather than a run later.
//!
//! Exhaustive product enums are matched by every variant as a tripwire for new cases. The kit's
//! vocabulary inventories are checked against crates.io by the semver check. Its `Reply` match stays: a known consumer matches it whole, so its migration
//! must precede a new variant even when a major version bump would satisfy semver.
//!
//! The guards are this file and `tests/consumer_api.rs` in `crates/tasks` (`symphony_board`),
//! `apps/symphony-server`, `crates/process-cwd` and `crates/repository-read`: every package a
//! consumer's code calls. Not held by any of them, and the project's to meet when it moves its pin
//! (its own gate builds it on the new one, docs/workflow/RELEASES.md, *A release*): the signature of a
//! function a guard names alone, and a field it reads or a method it calls on a value that is not
//! named in a guard.
//!
//! A consumer takes a release by moving its pin in its own landing, and nothing here changes its
//! code, so a consumer's code never changes with a release: a name or signature here that a
//! consumer uses changes only with a consumer release plan (the consumer's code moved first,
//! against a release that still has the old name). New behaviour goes behind the name a consumer
//! calls, or beside it under a new one; a trait gains a method only with a default. A lint that
//! asks for a signature pinned here to change is answered by an `#[expect]` on the item, never by
//! the change (`clippy.toml`). Names were read from the consuming projects' code on 2026-11-12 and
//! 2026-11-13, and on 2026-11-25 by building it against a tree whose public items were all
//! deprecated, so the compiler named each use, the methods it calls on a value among them; add one
//! when a consumer starts calling it.
#![expect(
    clippy::type_complexity,
    reason = "each signature is written out whole: it is what a consumer's code depends on"
)]

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use symphony::board::Doc;
use symphony::plugins::Plugins;
use symphony::project::{Entrypoint, Project};
use symphony::register::{Decision, Entry, Group, State, Who};
use symphony::render::{Media, Render};
use symphony::triage::{Bucket, Item};
use symphony::{
    blocks, clock, commit, docs_audit, docview, edit, images, landing, lint, machine, markup,
    plugins, project, register, renderers, taskcli, text, triage,
};
use symphony_api::{
    Block, Cell, ContainerKind, FigureSize, Fragment, Front, Image, InlineKind, LinkTarget,
    Presentation, Span,
};
use symphony_docs::{CardInfo, DocInfo, syntax};
use symphony_plugin::vocabulary::{
    GalleryItem, NavItem, Picture, ProgressBar, Region, TaskOffer, VectorShape, View, Viewer,
};
use symphony_plugin::{
    BoardAction, Card, CardKind, Command, Component, DocCheck, Docs, Drawing, File, HomeRegion,
    Page, Plugin, Problem, Renderer, Reply, Route, Submission, Submitted, TaskRef, Theme,
    ThreadLine, Watch, vocabulary,
};

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    // The project, its plugins and its generators.
    let _: &str = project::FILE;
    let _: fn(&Path) -> Arc<Project> = Project::of;
    let _: fn(&Path) -> Result<Project, String> = Project::load;
    let _: fn(&Path) -> Project = Project::plain;
    let _ = Project::parse;
    let _: Option<&Entrypoint> = None;
    let _: fn(&Project) -> symphony::theme::Package = symphony::theme::Package::of;
    let _: fn(Vec<&'static dyn Plugin>) -> Plugins = Plugins::new;
    let _: fn(&Project) -> Plugins = Plugins::for_project;
    // Its tests ask the project's plugins, and none, as a host asks them (read from its code on
    // 2026-11-25, as every name below it gives that date).
    let _: fn(&Path) -> Plugins = Plugins::of;
    let _: fn() -> Plugins = Plugins::default;
    let _: fn(&Plugins) -> &[&'static dyn Plugin] = Plugins::list;
    let _: for<'a> fn(&'a Plugins, &str) -> Option<&'a dyn Component> = Plugins::component;
    let _: for<'a> fn(&'a Plugins, &str) -> Option<&'a dyn Route> = Plugins::route;
    let _: fn(&Plugins) -> Vec<NavItem> = Plugins::nav;
    let _: fn(&Plugins) -> Vec<&dyn Watch> = Plugins::watches;
    let _: fn(&Plugins) -> Vec<&dyn CardKind> = Plugins::card_kinds;
    let _: fn(&Plugins) -> Vec<&dyn DocCheck> = Plugins::doc_checks;
    let _: fn(&Plugins) -> Vec<&dyn HomeRegion> = Plugins::home_regions;
    let _: fn(&Plugins) -> Vec<&dyn BoardAction> = Plugins::board_actions;
    let _: fn(&Plugins, &Page<'_>) -> Option<String> = Plugins::cache_version;
    let _: fn(&Project) -> Vec<String> = Project::history;
    let _: fn(&Project, &str) -> bool = Project::is_page;
    let _: fn(&Project) -> symphony_api::ProjectView = Project::view;
    let _: fn() -> Entrypoint = Entrypoint::convention;
    let _: fn(&Entrypoint, &str) -> Option<symphony::entrypoint::Renderer> = Entrypoint::renderer;
    let _: fn(&Entrypoint, &str) -> bool = Entrypoint::shows;
    let _: fn(&symphony::theme::Package) -> String = symphony::theme::Package::css;
    let _: fn(&'static [&'static dyn Plugin]) -> Result<(), String> = plugins::install;
    let _: fn() -> &'static [&'static dyn Plugin] = plugins::compiled;
    let _: fn() -> Vec<&'static dyn Command> = plugins::commands;
    let _: fn(&Project) -> Vec<String> = plugins::problems;
    let _: fn(&str) -> Block = plugins::error;
    let _: fn(&Plugins, &str) -> Option<&'static str> = renderers::content_type;
    let _: &str = machine::DIR_ENV;
    let _: fn(&Path) -> Vec<landing::Generator> = landing::generators;
    let _: fn(&Path, &str, &Project, &[landing::Generator]) -> bool = landing::regenerates;
    let _: fn(&Project, &str) -> bool = landing::is_doc;
    let _: fn(&Path) -> Vec<String> = landing::owner_lines_left;
    let _: fn(&Path, &str) -> Result<landing::PrivateWorktree, String> =
        landing::PrivateWorktree::new;
    let _: (&str, &str) = (landing::DIRTY_MAIN, landing::RETRY_LAND);

    // The command lines a consumer's host runs.
    let _: fn(&[String], &mut dyn BufRead, &mut dyn Write, &mut dyn Write) -> i32 =
        symphony::cli::main;
    let _: fn(&[String], &mut dyn Write, &mut dyn Write) -> i32 = symphony::cli::status_main;
    let _: fn(&[String], &mut dyn Write, &mut dyn Write) -> i32 = symphony::cli::triage_main;
    let _: fn(&mut dyn Write, &str, &str, &str) -> i32 = symphony::cli::refuse;
    let _: fn() = symphony::cli::end_with_parent;
    let _: fn(&[String], &[symphony::cli::Flag]) -> Result<symphony::cli::Parsed, String> =
        symphony::cli::parse;
    let _: fn(&[String], &[symphony::cli::Flag]) -> usize = symphony::cli::command_at;
    let _: fn(&[String], &[symphony_args::Flag]) -> Result<symphony_args::Parsed, String> =
        symphony_args::parse;
    // What a parse is asked, by either name (one type).
    let _: fn(&symphony_args::Parsed, &str) -> bool = symphony_args::Parsed::flag;
    let _: for<'a> fn(&'a symphony_args::Parsed, &str) -> Option<&'a str> =
        symphony_args::Parsed::value;
    let _ = |parsed: &symphony::cli::Parsed| parsed.values("name").map(str::len).sum::<usize>();
    let _: fn(symphony_args::Parsed) -> symphony::cli::Parsed = std::convert::identity;
    let _ = |name: &'static str, value: bool| {
        (
            symphony_args::Flag { name, value },
            symphony::cli::Flag { name, value },
        )
    };
    let _: fn(&Path, &mut dyn Write, &mut dyn Write) -> i32 = lint::main;
    let _: fn(&Path) -> (Vec<String>, Vec<String>) = lint::check_findings;
    let _ = lint::rule_findings;
    let _ = |root: &Path| docs_audit::audit(root);
    let _ = |program: &str| symphony::children::command(program);
    let _: fn(&str) -> Vec<String> = taskcli::shell_words;
    let _: fn(&Path) -> Option<PathBuf> = taskcli::tasks_binary;
    let _: fn() -> String = clock::now_stamp;
    let _: fn(&Path) -> Result<Option<std::fs::File>, String> = commit::write_lock;
    let _: fn() -> bool = images::sips_available;
    let _: fn(&str, &Path, bool, bool) -> String = images::reserve;

    // The decision register and the owner's review.
    let _ = register::parse_decisions::<String>;
    let _ = register::parse_decisions::<&str>;
    let _: fn(&str, &Plugins) -> Result<String, symphony::ReviewError> = register::sort_register;
    let _: fn(&str, &str, &str) -> Result<(), symphony::ReviewError> = register::check_nothing_lost;
    let _: fn(&str, &str, &str) -> Result<(), symphony::ReviewError> =
        register::check_nothing_repeated;
    let _: fn(&str, &str, &Plugins) -> (String, Vec<String>) = register::resolve;
    let _: fn(&Decision) -> Vec<Entry> = Decision::entries;
    let _: fn(&Decision) -> Option<String> = Decision::posted_at;
    let _: fn(&Decision, &Plugins) -> bool = Decision::open;
    let _: fn(&Decision, &Plugins) -> bool = Decision::resolvable;
    let _: fn(&Decision, &Plugins) -> Group = Decision::group;
    let _: fn(State) -> char = State::mark;
    let _: (&str, &str) = (register::REGION_START, register::REGION_END);
    // The variants a consumer names: `Who`'s compared with `==`, `Group`'s two in `assert_eq!`,
    // and four of `State`'s built to mark a card with, one compared with `!=`. Each enum then
    // matched whole, the header's tripwire.
    let _ = [Who::Owner, Who::Agent];
    compared::<Who>();
    let _ = [Group::Agent, Group::Resolved];
    asserted::<Group>();
    let _ = [
        State::Pending,
        State::AutoApproved,
        State::Dismissed,
        State::Rejected,
    ];
    compared::<State>();
    let _ = |who: Who| match who {
        Who::Owner | Who::Agent => {}
    };
    let _ = |group: Group| match group {
        Group::Decide
        | Group::Auto
        | Group::Confirm
        | Group::Agent
        | Group::Rejected
        | Group::Resolved => {}
    };
    let _ = |state: State| match state {
        State::Pending
        | State::AutoApproved
        | State::Approved
        | State::Rejected
        | State::Question
        | State::Resolved
        | State::Dismissed => {}
    };
    let _: Option<(&symphony::Decision, &symphony::Who, &symphony::Group)> = None;
    let _: fn(State) -> &'static str = State::as_str;
    let _: fn(&str, &str, &str, &str, &str, &Plugins) -> Result<String, symphony::EditError> =
        edit::set_decision;
    let _: fn(&str) -> String = edit::heading_anchor;
    let _: fn(&Path) -> Result<Vec<Doc>, symphony::ReviewError> = symphony::board::scan;
    let _: fn(&Path) -> Vec<symphony::branches::BranchCard> = symphony::branches::branch_cards;
    let _: fn(&symphony::branches::BranchCard) -> String = symphony::branches::BranchCard::path;
    let _: fn(&Plugins, &[Doc], usize, &str) -> Vec<Item> = triage::queue;
    let _: fn(&[Item]) -> String = triage::render_queue;
    let _: fn(&Plugins, &[Doc]) -> Result<triage::Stats, String> = triage::stats;
    let _: fn(&triage::Stats) -> String = triage::render_stats;
    let _: fn(&Path) -> String = triage::app_url;
    let _: fn(&Plugins, &str, &Decision) -> String = triage::category;
    let _: fn(&Decision) -> Option<String> = triage::first_verdict;
    let _: fn(&Plugins, &Decision) -> bool = triage::is_picture;
    let _ = triage::gates;
    let _ = |decided: u32, approved_first: u32| Bucket {
        decided,
        approved_first,
    };

    // Rendering documents into blocks.
    let _ = |root: &Path| Render::new(root);
    let _: fn(Render, Plugins) -> Render = Render::with_plugins;
    let _: fn(&Render, &str, &str) -> String = Render::html;
    let _: fn(&Render, &str, &str) -> Vec<symphony::render::Section> = Render::sections;
    let _: fn(&Render, &str, &str) -> Vec<symphony::render::Section> = Render::typed_sections;
    let _: fn(&Render, &str, &str) -> symphony::render::Fragment = Render::typed_reply;
    let _: fn(&str) -> Vec<(String, String, Option<String>)> = symphony::render::references;
    let _: fn(&str) -> Vec<VectorShape> = blocks::vector_shapes;
    let _: fn(&[Block], &mut dyn FnMut(Media) -> usize) -> String = blocks::write::html;
    let _: fn(&symphony::render::Fragment, &str) -> Vec<Block> = blocks::fragment;
    let _: fn(&symphony::render::Section, &str) -> Vec<Block> = blocks::section;
    let _: fn(&mut String, &mut [Media], &str) = docview::scope_fragment;
    let _: fn(&Render, &str, &str, &str) -> Result<Fragment, String> = docview::section;
    let _: fn(&Render, &str, &str) -> Result<BTreeMap<String, Fragment>, String> = docview::history;
    let _: fn(&Fragment) -> String = docview::legacy_html;
    let _ = docview::folder;
    let _: fn(&str) -> String = markup::escape;
    let _: fn(&str) -> String = markup::unescape;
    let _: fn(&str, &str) -> String = markup::quote;
    let _: Option<(
        &Front,
        &Cell,
        &ContainerKind,
        &FigureSize,
        &Image,
        &InlineKind,
    )> = None;
    let _: Option<(&LinkTarget, &Presentation, &Span)> = None;

    // The plugin vocabulary a consumer's plugin builds by its fields: every field named, so one
    // added here breaks it, but for `Theme`, built by the fields it names over `Default`.
    let _ = |text: String| NavItem {
        label: text.clone(),
        title: text.clone(),
        href: text.clone(),
        document: text,
    };
    let _ = |text: String, payload: Option<String>| TaskOffer {
        action: text.clone(),
        label: text.clone(),
        branch: text,
        payload,
    };
    let _ = |said: String, released: Vec<String>| Submitted { said, released };
    let _ = |payload: String, note: String| Submission { payload, note };
    let _ = |slug: &'static str| TaskRef {
        slug,
        status: slug,
        branch: slug,
    };
    let _ = |text: String, count: u32| {
        let bar = ProgressBar {
            key: text.clone(),
            name: text.clone(),
            page: text.clone(),
            title: text.clone(),
            done: count,
            waiting: count,
            total: count,
        };
        Region {
            id: text.clone(),
            title: text.clone(),
            note: text.clone(),
            bars: vec![bar],
            legend: [text.clone(), text.clone(), text],
        }
    };
    let _ = |src: Option<String>, caption: String| Picture {
        src,
        caption,
        waiting: false,
    };
    let _ = |text: String, poster: Option<String>| GalleryItem {
        name: text.clone(),
        src: text,
        poster,
    };
    let _ = |text: String, label: Option<String>| Viewer {
        element: text.clone(),
        script: text.clone(),
        attributes: Vec::new(),
        views: vec![View {
            label: text,
            title: label.clone(),
            set: Vec::new(),
            call: label.clone(),
        }],
        download: label.clone(),
        open_label: label.clone(),
        close_label: label,
    };

    // The blocks a consumer's plugin builds: the variants of `Block`, `Span` and `Media` it builds,
    // each by every field as a variant is, with the variants of `FigureSize`, `ContainerKind` and
    // `InlineKind` it builds them with, and the structs they hold by the fields it names
    // (`Presentation` over `Default`, or by `Presentation::class`, its class read); the variants it
    // matches, as it matches them. Its plugin names them
    // through `symphony_plugin::vocabulary` and its tests through `symphony_api`: one type by either
    // path. These calls retain the field types the semver tool does not compare.
    let _ = |text: String, spans: Vec<Span>, blocks: Vec<Block>, items: Vec<GalleryItem>| {
        let presentation = Presentation {
            title: text.clone(),
            attributes: [(text.clone(), text.clone())].into(),
            ..Presentation::default()
        };
        let _: &String = &presentation.class;
        let image = Image {
            src: text.clone(),
            alt: text.clone(),
            presentation: presentation.clone(),
        };
        let cell = Cell {
            header: false,
            spans: spans.clone(),
            presentation: Presentation::class(text.clone()),
        };
        let viewer = Viewer::default();
        [
            Block::Paragraph {
                spans: spans.clone(),
            },
            Block::Figure {
                picture: Some(image),
                caption: spans,
                href: Some(text.clone()),
                badge: Some(text.clone()),
                size: FigureSize::Small,
            },
            Block::Container {
                tag: ContainerKind::Div,
                presentation: presentation.clone(),
                blocks,
            },
            Block::Table {
                head: vec![cell.clone()],
                rows: vec![vec![cell]],
                presentation,
            },
            Block::Media {
                media: Media::Gallery {
                    items,
                    viewer: viewer.clone(),
                },
            },
            Block::Media {
                media: Media::Viewer {
                    src: text.clone(),
                    label: text,
                    poster: None,
                    viewer,
                },
            },
        ]
    };
    let _ = |text: String, spans: Vec<Span>, blocks: Vec<Block>, image: Image| {
        let shape: Vec<VectorShape> = Vec::new();
        [
            Span::Text { text: text.clone() },
            Span::Strong {
                spans: spans.clone(),
            },
            Span::Code { text: text.clone() },
            Span::Link {
                href: text.clone(),
                target: None,
                title: String::new(),
                spans: spans.clone(),
            },
            Span::LineBreak,
            Span::Image { image },
            Span::Icon {
                shape,
                label: text.clone(),
                named: false,
                tip: text,
            },
            Span::Styled {
                tag: InlineKind::Small,
                presentation: Presentation::default(),
                spans: spans.clone(),
            },
            Span::Styled {
                tag: InlineKind::Span,
                presentation: Presentation::default(),
                spans,
            },
            Span::Blocks { blocks },
        ]
    };
    // These variants are constructed by known consumers; keep their names across a major too.
    let _ = [
        FigureSize::Small,
        FigureSize::Row,
        FigureSize::Large,
        FigureSize::Tile,
        FigureSize::Cell,
        FigureSize::Slot,
        FigureSize::Wide,
    ];
    let _ = |block: &Block| {
        matches!(block, Block::Heading { anchor, .. } if anchor.is_empty())
            || matches!(block, Block::Component { kind, blocks }
                if kind.is_empty() && matches!(&blocks[0], Block::Figure { picture: None, size: FigureSize::Wide, .. }))
            || matches!(block, Block::Paragraph { spans }
                if matches!(spans.as_slice(), [Span::Styled { presentation, spans, .. }]
                    if presentation.class.is_empty() && spans.is_empty()))
    };
    asserted::<Block>();
    asserted::<Span>();
    let _ = |blocks: Vec<Block>, html: String, media: Vec<Media>| Fragment {
        blocks,
        html,
        media,
    };
    let _: fn(Block) -> vocabulary::Block = std::convert::identity;
    let _: fn(Span) -> vocabulary::Span = std::convert::identity;
    let _: fn(Media) -> vocabulary::Media = std::convert::identity;
    let _: fn(Image) -> vocabulary::Image = std::convert::identity;
    let _: fn(Presentation) -> vocabulary::Presentation = std::convert::identity;
    let _: fn(FigureSize) -> vocabulary::FigureSize = std::convert::identity;
    let _: fn(ContainerKind) -> vocabulary::ContainerKind = std::convert::identity;
    let _: fn(InlineKind) -> vocabulary::InlineKind = std::convert::identity;
    // What its route and its renderer answer: its route builds `Reply::File`, and its route's test
    // matches a `Reply` whole, a file by its path and JSON by what it prints; its renderer builds
    // `Drawing::Viewer`. The field types remain checked here.
    let _ = |path: PathBuf, viewer: Viewer| (Reply::File(path), Drawing::Viewer(viewer));
    let _ = |reply: Reply| match reply {
        Reply::File(path) => Some(path),
        Reply::Json(json) => {
            let _: &dyn std::fmt::Display = &json;
            None
        }
    };
    let _ = |text: &'static str| Card {
        slug: text,
        doc: text,
        topic: text,
        text,
        thread: &[],
    };
    let _ = |package: String, name: String, icon: Option<String>| Theme {
        package,
        name,
        icon,
        ..Theme::default()
    };

    // Text, schemas and the docs' syntax.
    let _: &str = text::S;
    let _: fn(&str) -> Vec<&str> = text::splitlines;
    let _: fn(&str) -> &str = text::strip;
    let _: fn(char) -> bool = text::is_word;
    let _: fn(&str) -> String = text::repr;
    let _: fn(&std::io::Error, &Path) -> String = text::os_error;
    let _: fn(&Path) -> std::io::Result<String> = text::read_text;
    let _: fn(&Path, &str) -> std::io::Result<()> = text::write_whole;
    let _: fn(&str) -> String = symphony_schema::text::repr;
    let _: fn(&serde_json::Value) -> String = symphony_schema::repr;
    let _ = symphony_schema::validate;
    let _: Option<&symphony::schema::Schema> = None;
    let _: fn(&serde_json::Value) -> Result<symphony_schema::Schema, symphony_schema::SchemaError> =
        symphony_schema::Schema::compile;
    let _: fn(&symphony_schema::Schema, &serde_json::Value) -> Vec<String> =
        symphony_schema::Schema::errors;
    let _: fn(symphony_schema::Schema) -> symphony::schema::Schema = std::convert::identity;
    // The docs a plugin's check reads (`Docs` and `Problem` are the kit's and the helpers' by
    // either name), and a card's state and entries.
    let _: fn(&Path) -> Docs = Docs::read;
    let _: fn(&Docs) -> &Path = Docs::root;
    let _: fn(&Docs) -> &[String] = Docs::paths;
    let _: for<'a> fn(&'a Docs, &str) -> &'a str = Docs::text;
    let _: fn(&Docs, &str) -> bool = Docs::has;
    let _: for<'a> fn(&'a Docs, &str) -> &'a [syntax::Card] = Docs::cards;
    let _: fn(&Docs) -> Vec<(&str, syntax::Component)> = Docs::components;
    let _: fn(&str, usize, String) -> Problem = Problem::new;
    let _: fn(Docs) -> symphony_docs::Docs = std::convert::identity;
    let _: fn(Problem) -> symphony_docs::Problem = std::convert::identity;
    let _: fn(&syntax::Card) -> &'static str = syntax::Card::state;
    let _: fn(&syntax::Card) -> Vec<syntax::Entry> = syntax::Card::entries;
    let _ = symphony_docs::check;
    let _: fn(&str) -> Vec<(usize, &str)> = symphony_docs::prose_lines;
    let _ = |line: &str| symphony_docs::runs(line, char::is_alphabetic).len();
    let _: Option<(&CardInfo, &DocInfo)> = None;
    let _ = |text: String, line: usize| syntax::Card {
        line,
        slug: text.clone(),
        mark: ' ',
        text: text.clone(),
        topic: text,
        raw: Vec::new(),
    };
    let _ = |text: String, ts: Option<String>| syntax::Entry {
        ts: ts.clone(),
        who: "owner",
        verdict: ts,
        text,
    };
    let _: fn(char) -> bool = syntax::is_space;
    let _: fn(char) -> bool = syntax::is_word;
    let _: fn(&str) -> bool = syntax::is_fence;
    let _: fn(&str) -> Vec<&str> = syntax::lines;
    let _: fn(&str) -> Option<(char, &str, &str)> = syntax::card_line;
    let _: fn(&str) -> String = syntax::heading_anchor;
    let _: fn(&str, usize) -> usize = syntax::line_of;
    let _: fn(&str) -> Vec<&str> = syntax::pictures;
    let _ = syntax::wikilinks;
    let _: (&str, &str) = (syntax::REGION_START, syntax::REGION_END);
    let _: Option<&ThreadLine<'_>> = None;

    // Where a plugin built on the kit and the helpers alone finds what it took from the library
    // (2026-11-13): the library's names above re-export these.
    let _: &str = symphony_docs::text::S;
    let _: fn(&str) -> Vec<&str> = symphony_docs::text::splitlines;
    let _: fn(&str) -> &str = symphony_docs::text::strip;
    let _: fn(char) -> bool = symphony_docs::text::is_word;
    let _: fn(&Path) -> std::io::Result<String> = symphony_docs::text::read_text;
    let _: fn(&Path, &str) -> std::io::Result<()> = symphony_docs::text::write_whole;
    let _: fn(&str) -> String = symphony_docs::markup::escape;
    let _: fn(&str) -> String = symphony_docs::markup::unescape;
    let _: fn(&str, &str) -> String = symphony_docs::markup::quote;
    let _: fn(&std::io::Error, &Path) -> String = symphony_schema::text::os_error;
    let _: fn(&mut dyn Write, &str, &str, &str) -> i32 = symphony_args::refuse;
    let _: fn(&str) -> Block = symphony_plugin::error;
    let _: &str = symphony_plugin::ARTIFACTS;
    let _ = |page: &Page<'_>, rel: &str| -> String { page.artifact(rel) };
    let _ = |page: Page<'static>| page.in_artifacts("out");
    // A page borrows its root and its document for any lifetime.
    fn page<'a>(_: std::marker::PhantomData<&'a ()>) {
        let _: fn(&'a Path, &'a str) -> Page<'a> = Page::new;
    }
    page(std::marker::PhantomData);
    let _ = |page: &Page<'_>| -> Arc<u32> { page.once("key", || 0) };
    let _: fn() -> Submission = Submission::default;
    let _: fn() -> TaskOffer = TaskOffer::default;
    // A board action submitted on a sender's behalf, as its tests ask a process plugin's.
    let _: fn(&str, &str, Vec<String>) -> symphony_plugin::Sender = symphony_plugin::Sender::new;
    let _ = |action: &dyn BoardAction,
             root: &Path,
             task: &TaskRef<'_>,
             submission: &Submission,
             sender: &symphony_plugin::Sender|
     -> Result<Submitted, String> {
        action.submit_from(root, task, submission, sender)
    };
    // Its process plugin's test writes the declaration request a host sends first.
    let _ = || {
        use symphony_plugin::protocol::{Declare, Envelope, PROTOCOL, Request};
        let request = Envelope::<Request, ()>::asking(1, Declare::new(PROTOCOL).into());
        serde_json::to_string(&request)
    };
    // Its tests ask its components and card kinds as a host does: a request made by its `new` from
    // what the seam borrows, as no protocol struct is built by its fields (`#[non_exhaustive]`).
    let _ = |plugin: &'static dyn Plugin, page: &Page<'_>, card: &Card<'_>| {
        use symphony_plugin::protocol::{CardData, Declaration, PageData, Recognises};
        let host = symphony_plugin::testing::Host::new(plugin);
        let _: Vec<Block> = host.render(page, "component", "argument", None);
        let _: Result<Declaration, String> = host.declaration();
        let _: fn(&Page<'_>) -> PageData = PageData::of;
        let _: fn(String, CardData) -> Recognises = Recognises::new;
        let _: Result<bool, String> = host.ask(Recognises::new("kind".into(), CardData::of(card)));
    };

    // A consumer's plugin, below, is one the Symphony takes.
    static PLUGIN: Consumer = Consumer;
    let _: &'static dyn Plugin = &PLUGIN;
}

/// A type a consumer compares with `==` or `!=`.
fn compared<T: PartialEq>() {}

/// A type a consumer compares in `assert_eq!`, which prints both sides when they differ.
fn asserted<T: PartialEq + std::fmt::Debug>() {}

// A plugin's traits, implemented with the methods a consumer's plugin writes: a required method
// added to one, or one of these changed, breaks the plugin.

struct Consumer;

impl Plugin for Consumer {
    fn name(&self) -> &str {
        ""
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![self]
    }
    fn routes(&self) -> Vec<&dyn Route> {
        vec![self]
    }
    fn nav(&self) -> Vec<NavItem> {
        Vec::new()
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        vec![self]
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![self]
    }
    fn commands(&self) -> Vec<&dyn Command> {
        vec![self]
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        vec![self]
    }
    fn theme(&self) -> Option<Theme> {
        Some(Theme::default())
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        vec![self]
    }
}

impl Component for Consumer {
    fn name(&self) -> &str {
        ""
    }
    fn render(&self, _: &Page<'_>, _: &str, _: Option<&str>) -> Result<Vec<Block>, String> {
        Ok(Vec::new())
    }
    fn cache_version(&self, _: &Page<'_>) -> Option<String> {
        None
    }
}

impl CardKind for Consumer {
    fn name(&self) -> &str {
        ""
    }
    fn recognises(&self, _: &Card<'_>) -> bool {
        false
    }
    fn pictures(&self, _: &Path, _: &Card<'_>) -> Vec<Picture> {
        Vec::new()
    }
    fn markers(&self) -> &[&str] {
        &[]
    }
    fn slot(&self, _: &Card<'_>) -> Option<String> {
        None
    }
    fn triage_category(&self, _: &Card<'_>) -> Option<String> {
        None
    }
    fn auto_approvable(&self, _: &Card<'_>) -> bool {
        false
    }
}

impl Renderer for Consumer {
    fn name(&self) -> &str {
        ""
    }
    fn extensions(&self) -> &[&str] {
        &[]
    }
    fn content_type(&self) -> &'static str {
        ""
    }
    fn draw(&self, _: &Page<'_>, _: &File<'_>) -> Result<Drawing, String> {
        Err(String::new())
    }
}

impl Route for Consumer {
    fn path(&self) -> &str {
        ""
    }
    fn serve(&self, _: &Path, _: &str) -> Option<Reply> {
        None
    }
}

impl HomeRegion for Consumer {
    fn id(&self) -> &'static str {
        ""
    }
    fn region(&self, _: &Path) -> Option<Region> {
        None
    }
}

impl BoardAction for Consumer {
    fn name(&self) -> &'static str {
        ""
    }
    fn offer(&self, _: &Path, _: &TaskRef<'_>) -> Result<TaskOffer, String> {
        Err(String::new())
    }
    fn submit(&self, _: &Path, _: &TaskRef<'_>, _: &Submission) -> Result<Submitted, String> {
        Err(String::new())
    }
}

impl Command for Consumer {
    fn name(&self) -> &str {
        ""
    }
    fn run(&self, _: &[String], _: &mut dyn Write, _: &mut dyn Write) -> i32 {
        0
    }
}

impl Watch for Consumer {
    fn paths(&self) -> Vec<String> {
        Vec::new()
    }
    fn changed(&self, _: &Path) -> Option<String> {
        None
    }
    fn forget(&self, _: &Path) {}
}

impl DocCheck for Consumer {
    fn name(&self) -> &'static str {
        ""
    }
    fn components(&self, _: &Docs) -> Vec<String> {
        Vec::new()
    }
    fn inputs(&self) -> Vec<String> {
        Vec::new()
    }
    fn check(&self, _: &Docs) -> Vec<Problem> {
        Vec::new()
    }
}
