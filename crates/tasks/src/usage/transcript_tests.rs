use super::*;
use serde_json::json;
use std::io::Write as _;

/// A folder of the test's own, removed when it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("usage-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An assistant line of message `id`, as Claude Code writes one for each content block.
fn assistant(id: &str, input: u64, five: u64, hour: u64, read: u64, output: u64) -> String {
    json!({
        "type": "assistant", "sessionId": "s-1", "isSidechain": false,
        "message": {
            "id": id, "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "text", "text": "\"usage\" in the text"}],
            "usage": {
                "input_tokens": input,
                "cache_creation_input_tokens": five + hour,
                "cache_read_input_tokens": read,
                "output_tokens": output,
                "cache_creation": {"ephemeral_5m_input_tokens": five, "ephemeral_1h_input_tokens": hour},
                "service_tier": "standard", "speed": "standard", "fallback_credit": null,
            },
        },
    })
    .to_string()
}

fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn tokens(input: u64, five: u64, hour: u64, read: u64, output: u64, calls: u64) -> Tokens {
    Tokens {
        input,
        cache_write_5m: five,
        cache_write_1h: hour,
        cache_read: read,
        output,
        calls,
    }
}

fn opus(t: Tokens) -> Spent {
    Spent {
        model: "claude-opus-5-5".into(),
        speed: "standard".into(),
        tokens: t,
    }
}

/// One beat's read of `key`'s ledger: what it hands the board, the board taking it or not.
fn beat(ledgers: &Path, key: &str, transcript: &Path, recorded: bool) -> Vec<Spent> {
    let ledger = Ledger::open(ledgers, key, transcript).expect("the ledger is free");
    let spent = ledger.spent().to_vec();
    ledger.close(recorded);
    spent
}

#[test]
fn a_beat_reads_only_what_the_transcript_gained_and_counts_each_message_once() {
    let folder = Folder::new("gained");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    let user = json!({"type": "user", "message": {"role": "user", "content": "hi"}}).to_string();
    append(
        &transcript,
        &format!(
            "{user}\n{}\n{}\n",
            assistant("m1", 2, 100, 0, 1_000, 10),
            assistant("m1", 2, 100, 0, 1_000, 10)
        ),
    );
    assert_eq!(
        beat(&ledgers, "agent-1", &transcript, true),
        [opus(tokens(2, 100, 0, 1_000, 10, 1))]
    );
    assert_eq!(beat(&ledgers, "agent-1", &transcript, true), []);
    // The message's last line comes after the beat that counted it: it counts no more.
    append(
        &transcript,
        &format!(
            "{}\n{}\n",
            assistant("m1", 2, 100, 0, 1_000, 10),
            assistant("m2", 1, 0, 50, 1_100, 20)
        ),
    );
    assert_eq!(
        beat(&ledgers, "agent-1", &transcript, true),
        [opus(tokens(1, 0, 50, 1_100, 20, 1))]
    );
}

/// The lines a fork inherits as Claude Code writes them: the parent's message `call_by`, its Agent
/// call `toolu_fork`, and the result that answers it, which carries the fork's directive.
fn forked_from(call_by: &str) -> String {
    let call = json!({"type": "assistant", "message": {"id": call_by, "model": "claude-opus-5-5",
        "content": [{"type": "tool_use", "id": "toolu_fork", "name": "Agent", "input": {}}],
        "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let result = json!({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "toolu_fork", "content": "forked"},
        {"type": "text", "text": "Your directive: triage group D"}]}});
    format!("{call}\n{result}\n")
}

fn meta(transcript: &Path, text: &str) {
    let name = transcript.file_name().unwrap().to_str().unwrap();
    let name = name.strip_suffix(".jsonl").unwrap();
    fs::write(transcript.with_file_name(format!("{name}.meta.json")), text).unwrap();
}

#[test]
fn a_forks_inherited_history_is_passed_over_up_to_the_result_of_the_call_that_made_it() {
    let folder = Folder::new("fork");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("agent-f1.jsonl"));
    meta(
        &transcript,
        r#"{"agentType": "fork", "isFork": true, "toolUseId": "toolu_fork",
            "parentAgentId": "p1"}"#,
    );
    // Its parent's history, under the parent's ids: the first beat reads it, and counts nothing.
    append(
        &transcript,
        &format!("{}\n", assistant("p1", 5, 0, 0, 50, 5)),
    );
    assert_eq!(beat(&ledgers, "f1", &transcript, true), []);
    let result = forked_from("p2");
    let (call, answer) = result.split_at(result.find('\n').unwrap() + 1);
    append(&transcript, call);
    assert_eq!(beat(&ledgers, "f1", &transcript, true), []);
    // The result that answers the parent's call ends it: what follows is the fork's own.
    append(
        &transcript,
        &format!("{answer}{}\n", assistant("f1", 1, 10, 0, 100, 1)),
    );
    assert_eq!(
        beat(&ledgers, "f1", &transcript, true),
        [opus(tokens(1, 10, 0, 100, 1, 1))]
    );
    // A later result of a call of the same id is the fork's own line, counted as any other.
    append(
        &transcript,
        &format!("{answer}{}\n", assistant("f2", 2, 0, 0, 200, 2)),
    );
    assert_eq!(
        beat(&ledgers, "f1", &transcript, true),
        [opus(tokens(2, 0, 0, 200, 2, 1))]
    );
}

#[test]
fn a_transcript_whose_meta_names_no_fork_or_cannot_be_read_is_read_whole() {
    for (key, said) in [
        (
            "plain",
            Some(r#"{"agentType": "general-purpose", "toolUseId": "toolu_fork"}"#),
        ),
        ("unnamed", Some(r#"{"isFork": true}"#)),
        ("broken", Some("{")),
        ("none", None),
    ] {
        let folder = Folder::new(&format!("meta-{key}"));
        let (ledgers, transcript) = (folder.path("usage"), folder.path("agent-a1.jsonl"));
        if let Some(said) = said {
            meta(&transcript, said);
        }
        append(&transcript, &forked_from("p2"));
        assert_eq!(
            beat(&ledgers, "a1", &transcript, true),
            [opus(tokens(1, 0, 0, 0, 1, 1))],
            "{key}"
        );
    }
}

#[test]
fn a_meta_names_a_fork_only_by_true_and_the_call_by_its_text() {
    let folder = Folder::new("meta-kinds");
    let transcript = folder.path("agent-a1.jsonl");
    for (said, until) in [
        (
            r#"{"isFork": true, "toolUseId": "toolu_fork"}"#,
            Some("toolu_fork"),
        ),
        (r#"{"isFork": "true", "toolUseId": "toolu_fork"}"#, None),
        (r#"{"isFork": 1, "toolUseId": "toolu_fork"}"#, None),
        (r#"{"isFork": false, "toolUseId": "toolu_fork"}"#, None),
        (r#"{"isFork": true, "toolUseId": 5}"#, None),
        (r#"{"isFork": true, "toolUseId": ""}"#, None),
    ] {
        meta(&transcript, said);
        assert_eq!(inherited_until(&transcript).as_deref(), until, "{said}");
    }
    assert_eq!(inherited_until(&folder.path("s-1.txt")), None);
}

#[test]
fn a_line_that_is_not_text_is_passed_over_and_what_follows_is_read() {
    let folder = Folder::new("bytes");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    let mut bytes = b"{\"type\": \"assistant\", \"usage\": \"\xff\"}\n".to_vec();
    bytes.extend(format!("{}\n", assistant("m1", 1, 0, 0, 10, 1)).into_bytes());
    fs::write(&transcript, bytes).unwrap();
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(1, 0, 0, 10, 1, 1))]
    );
}

#[test]
fn a_line_still_being_written_is_read_whole_at_the_next_beat() {
    let folder = Folder::new("partial");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    let line = assistant("m1", 3, 0, 0, 500, 7);
    let (head, tail) = line.split_at(line.len() / 2);
    append(&transcript, head);
    assert_eq!(beat(&ledgers, "k", &transcript, true), []);
    append(&transcript, &format!("{tail}\n"));
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(3, 0, 0, 500, 7, 1))]
    );
}

#[test]
fn what_the_board_did_not_take_is_handed_again_with_what_came_after() {
    let folder = Folder::new("pending");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    append(
        &transcript,
        &format!("{}\n", assistant("m1", 1, 10, 0, 100, 1)),
    );
    // Not bound yet: the board takes nothing, and the ledger keeps it.
    assert_eq!(
        beat(&ledgers, "k", &transcript, false),
        [opus(tokens(1, 10, 0, 100, 1, 1))]
    );
    append(
        &transcript,
        &format!("{}\n", assistant("m2", 2, 20, 0, 200, 2)),
    );
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(3, 30, 0, 300, 3, 2))]
    );
    assert_eq!(beat(&ledgers, "k", &transcript, true), []);
}

#[test]
fn kinds_models_and_speeds_are_kept_apart_and_synthetic_messages_are_no_calls() {
    let folder = Folder::new("kinds");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    let fast = json!({"type": "assistant", "message": {"id": "f1", "model": "claude-opus-5-5",
        "usage": {"input_tokens": 5, "output_tokens": 6, "speed": "fast"}}});
    // A write with no split is the five-minute kind; one the split leaves part of, that part too.
    let unsplit = json!({"type": "assistant", "message": {"id": "u1", "model": "claude-haiku-4-5",
        "usage": {"input_tokens": 1, "cache_creation_input_tokens": 40, "output_tokens": 2}}});
    let short = json!({"type": "assistant", "message": {"id": "p1", "model": "claude-haiku-4-5",
        "usage": {"cache_creation_input_tokens": 30, "output_tokens": 1,
                  "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 20}}}});
    let synthetic = json!({"type": "assistant", "message": {"id": "x1", "model": "<synthetic>",
        "usage": {"input_tokens": 0, "output_tokens": 0}}});
    let summary = json!({"type": "summary", "usage": "x", "assistant": "y"});
    append(
        &transcript,
        &format!(
            "{fast}\n{unsplit}\n{short}\n{synthetic}\n{summary}\nnot json \"assistant\" \"usage\"\n"
        ),
    );
    let haiku = |t: Tokens| Spent {
        model: "claude-haiku-4-5".into(),
        speed: "standard".into(),
        tokens: t,
    };
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [
            haiku(tokens(1, 50, 20, 0, 3, 2)),
            Spent {
                model: "claude-opus-5-5".into(),
                speed: "fast".into(),
                tokens: tokens(5, 0, 0, 0, 6, 1),
            },
        ]
    );
}

#[test]
fn a_field_of_another_kind_passes_its_line_over_and_a_null_one_reads_as_absent() {
    let base = || {
        json!({"type": "assistant", "message": {"id": "m1", "model": "claude-opus-5-5",
            "usage": {"input_tokens": 1, "cache_creation_input_tokens": 30, "cache_read_input_tokens": 3,
                      "output_tokens": 4, "speed": "fast",
                      "cache_creation": {"ephemeral_5m_input_tokens": 10, "ephemeral_1h_input_tokens": 20}}}})
    };
    let read = |line: &Value| message(line.to_string().as_bytes());
    let fast = |t: Tokens| Spent {
        model: "claude-opus-5-5".into(),
        speed: "fast".into(),
        tokens: t,
    };
    assert_eq!(
        read(&base()),
        Some(("m1".into(), fast(tokens(1, 10, 20, 3, 4, 1))))
    );
    let usage = ["message", "usage"];
    let split = ["message", "usage", "cache_creation"];
    for (at, key, value) in [
        (&usage[..], "input_tokens", json!("1")),
        (&usage[..], "output_tokens", json!(4.5)),
        (&usage[..], "cache_read_input_tokens", json!(-3)),
        (&usage[..], "cache_creation_input_tokens", json!(true)),
        (&split[..], "ephemeral_1h_input_tokens", json!("20")),
        (&usage[..], "cache_creation", json!(30)),
        (&usage[..], "speed", json!(1)),
        (&["message"][..], "model", json!(["claude-opus-5-5"])),
        (&["message"][..], "id", json!(7)),
        (&["message"][..], "usage", json!("none")),
        (&[][..], "type", json!(["assistant"])),
    ] {
        let mut line = base();
        let mut field = &mut line;
        for step in at {
            field = &mut field[*step];
        }
        field[key] = value.clone();
        assert_eq!(read(&line), None, "{key} = {value}");
    }
    let mut nulls = base();
    for key in ["input_tokens", "cache_creation", "speed"] {
        nulls["message"]["usage"][key] = Value::Null;
    }
    nulls["message"]["model"] = Value::Null;
    // No split: the cache write is the five-minute kind; no speed: standard; no model: "".
    assert_eq!(
        read(&nulls),
        Some((
            "m1".into(),
            Spent {
                model: String::new(),
                speed: "standard".into(),
                tokens: tokens(0, 30, 0, 3, 4, 1),
            }
        ))
    );
}

#[test]
fn a_second_beat_of_the_key_meanwhile_reads_nothing_and_the_next_reads_what_it_left() {
    let folder = Folder::new("held");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    append(
        &transcript,
        &format!("{}\n", assistant("m1", 1, 0, 0, 0, 1)),
    );
    let first = Ledger::open(&ledgers, "k", &transcript).unwrap();
    assert!(Ledger::open(&ledgers, "k", &transcript).is_none());
    // Another key is its own.
    assert_eq!(
        beat(&ledgers, "other", &transcript, true),
        [opus(tokens(1, 0, 0, 0, 1, 1))]
    );
    first.close(true);
    append(
        &transcript,
        &format!("{}\n", assistant("m2", 2, 0, 0, 0, 2)),
    );
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(2, 0, 0, 0, 2, 1))]
    );
}

#[test]
fn a_closed_ledger_lets_its_key_go_though_a_child_started_meanwhile_holds_its_lock_still() {
    let folder = Folder::new("inherited");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    append(
        &transcript,
        &format!("{}\n", assistant("m1", 1, 0, 0, 0, 1)),
    );
    let first = Ledger::open(&ledgers, "k", &transcript).unwrap();
    // What a child forked now holds until its exec closes it: the same open lock.
    let inherited = first.lock.try_clone().unwrap();
    first.close(true);
    append(
        &transcript,
        &format!("{}\n", assistant("m2", 2, 0, 0, 0, 2)),
    );
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(2, 0, 0, 0, 2, 1))]
    );
    drop(inherited);
}

#[test]
fn a_ledger_that_does_not_parse_starts_at_the_end_and_counts_nothing_again() {
    let folder = Folder::new("corrupt");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    append(
        &transcript,
        &format!("{}\n", assistant("m1", 1, 0, 0, 0, 1)),
    );
    fs::create_dir_all(&ledgers).unwrap();
    fs::write(ledgers.join("k.json"), "{cut sh").unwrap();
    assert_eq!(beat(&ledgers, "k", &transcript, true), []);
    append(
        &transcript,
        &format!("{}\n", assistant("m2", 2, 0, 0, 0, 2)),
    );
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(2, 0, 0, 0, 2, 1))]
    );
}

#[test]
fn another_transcript_or_a_shorter_one_is_read_from_its_start() {
    let folder = Folder::new("another");
    let ledgers = folder.path("usage");
    let (one, two) = (folder.path("one.jsonl"), folder.path("two.jsonl"));
    append(&one, &format!("{}\n", assistant("m1", 1, 0, 0, 0, 1)));
    append(&two, &format!("{}\n", assistant("n1", 4, 0, 0, 0, 4)));
    assert_eq!(beat(&ledgers, "k", &one, true).len(), 1);
    assert_eq!(
        beat(&ledgers, "k", &two, true),
        [opus(tokens(4, 0, 0, 0, 4, 1))]
    );
    // Replaced by a shorter file: its start is new.
    let shorter = assistant("z", 1, 0, 0, 0, 0);
    assert!(shorter.len() < assistant("n1", 4, 0, 0, 0, 4).len());
    fs::write(&two, format!("{shorter}\n")).unwrap();
    assert_eq!(
        beat(&ledgers, "k", &two, true),
        [opus(tokens(1, 0, 0, 0, 0, 1))]
    );
}

#[test]
fn a_transcript_longer_than_one_read_is_read_over_several_beats() {
    let folder = Folder::new("long");
    let (ledgers, transcript) = (folder.path("usage"), folder.path("s-1.jsonl"));
    let padding = "x".repeat(usize::try_from(READ_AT_MOST / 2).unwrap());
    let filler = json!({"type": "user", "message": {"content": padding}}).to_string();
    append(
        &transcript,
        &format!("{filler}\n{filler}\n{}\n", assistant("m1", 1, 0, 0, 0, 1)),
    );
    assert_eq!(beat(&ledgers, "k", &transcript, true), []);
    assert_eq!(
        beat(&ledgers, "k", &transcript, true),
        [opus(tokens(1, 0, 0, 0, 1, 1))]
    );
}

#[test]
fn a_missing_transcript_hands_only_what_waited() {
    let folder = Folder::new("missing");
    let ledgers = folder.path("usage");
    assert_eq!(beat(&ledgers, "k", &folder.path("gone.jsonl"), true), []);
    assert!(Ledger::open(&ledgers, "../k", &folder.path("gone.jsonl")).is_none());
}

#[test]
fn a_subagents_transcript_is_its_own_never_its_parents() {
    let event = |pairs: Value| pairs.as_object().unwrap().clone();
    let main = "/h/.claude/projects/-r/s-1.jsonl";
    assert_eq!(
        of_event(&event(
            json!({"session_id": "s-1", "transcript_path": main})
        )),
        Some(PathBuf::from(main))
    );
    assert_eq!(
        of_event(&event(
            json!({"agent_id": "a1", "session_id": "s-1", "transcript_path": main})
        )),
        Some(PathBuf::from(
            "/h/.claude/projects/-r/s-1/subagents/agent-a1.jsonl"
        ))
    );
    let own = "/h/.claude/projects/-r/s-1/subagents/agent-a1.jsonl";
    assert_eq!(
        of_event(&event(json!({"agent_id": "a1", "transcript_path": own}))),
        Some(PathBuf::from(own))
    );
    assert_eq!(
        of_event(&event(json!({"agent_id": "a1", "transcript_path": main,
                                "agent_transcript_path": "/elsewhere/agent-a1.jsonl"}))),
        Some(PathBuf::from("/elsewhere/agent-a1.jsonl"))
    );
    // Another agent's path is not this one's.
    assert_eq!(
        of_event(&event(json!({"agent_id": "a1", "transcript_path": main,
                                "agent_transcript_path": "/elsewhere/agent-b2.jsonl"}))),
        Some(PathBuf::from(
            "/h/.claude/projects/-r/s-1/subagents/agent-a1.jsonl"
        ))
    );
    for refused in [
        json!({"agent_id": "../a1", "transcript_path": main}),
        json!({"session_id": "s-1", "transcript_path": "relative.jsonl"}),
        json!({"session_id": "s-1"}),
        json!({"agent_id": "a1"}),
    ] {
        assert_eq!(of_event(&event(refused.clone())), None, "{refused}");
    }
}

#[test]
fn a_new_ledger_prunes_those_left_a_week_whose_transcript_is_gone() {
    let folder = Folder::new("prune");
    let ledgers = folder.path("usage");
    fs::create_dir_all(&ledgers).unwrap();
    let (kept, gone) = (folder.path("kept.jsonl"), folder.path("gone.jsonl"));
    append(&kept, &format!("{}\n", assistant("m1", 1, 0, 0, 10, 1)));
    // Each as a beat left it: the transcript read to its end.
    let naming = |transcript: &Path| {
        serde_json::to_string(&Kept {
            transcript: transcript.to_string_lossy().into_owned(),
            offset: fs::metadata(transcript).map_or(0, |m| m.len()),
            ..Kept::default()
        })
        .unwrap()
    };
    for (name, text) in [
        ("old.json", naming(&gone)),
        ("old.lock", String::new()),
        ("resumed.json", naming(&kept)),
        ("resumed.lock", String::new()),
        ("broken.json", "{".into()),
        ("lone.lock", String::new()),
        ("new.json", naming(&gone)),
        ("new.lock", String::new()),
    ] {
        fs::write(ledgers.join(name), text).unwrap();
    }
    let week = SystemTime::now() + PRUNE_AFTER;
    let fresh = |name: &str| {
        let file = File::options()
            .write(true)
            .open(ledgers.join(name))
            .unwrap();
        file.set_modified(week).unwrap();
    };
    fresh("new.json");
    prune(&ledgers, week);
    let mut left: Vec<String> = fs::read_dir(&ledgers)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    crate::order::sort_by(&mut left, &Ord::cmp);
    assert_eq!(
        left,
        [
            "broken.json",
            "new.json",
            "new.lock",
            "resumed.json",
            "resumed.lock"
        ],
        "a week idle, only the ledger whose transcript is gone goes, with its lock"
    );
    // The session resumed after its week reads on from where its ledger stopped.
    append(&kept, &format!("{}\n", assistant("m2", 2, 0, 0, 20, 2)));
    assert_eq!(
        beat(&ledgers, "resumed", &kept, true),
        [opus(tokens(2, 0, 0, 20, 2, 1))]
    );
}
