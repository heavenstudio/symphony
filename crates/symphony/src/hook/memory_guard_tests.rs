//! The memory guard's reading of each tool's event: what it refuses and what it lets through.

use super::*;
use serde_json::json;

const CLAUDE: &str = "/Users/someone/.claude/projects/-Users-someone-repo/memory";
const CODEX: &str = "/Users/someone/.codex/memories";

/// Whether the guard refuses `tool` called with `input` from `cwd`.
fn refused_in(cwd: &str, tool: &str, input: &Value) -> bool {
    let event = json!({"hook_event_name": "PreToolUse", "cwd": cwd, "tool_name": tool, "tool_input": input});
    refuses(event.to_string().as_bytes())
}

fn refused(tool: &str, input: &Value) -> bool {
    refused_in("/repo", tool, input)
}

fn bash(command: &str) -> bool {
    refused("Bash", &json!({"command": command}))
}

/// Each write tool's input, aimed at `path`.
fn writes_to(path: &str) -> [(&'static str, Value); 4] {
    [
        ("Write", json!({"file_path": path, "content": "x"})),
        (
            "Edit",
            json!({"file_path": path, "old_string": "a", "new_string": "b"}),
        ),
        (
            "MultiEdit",
            json!({"file_path": path, "edits": [{"old_string": "a", "new_string": "b"}]}),
        ),
        (
            "NotebookEdit",
            json!({"notebook_path": path, "new_source": "x", "cell_type": "markdown"}),
        ),
    ]
}

#[test]
fn every_write_tool_into_either_memory_folder_is_refused() {
    for folder in [CLAUDE, CODEX] {
        for path in [
            format!("{folder}/note.md"),
            format!("{folder}/MEMORY.md"),
            format!("{folder}/nested/deeper.md"),
            folder.to_string(),
            format!("{folder}//double.md"),
        ] {
            for (tool, input) in writes_to(&path) {
                assert!(refused(tool, &input), "{tool} {path}");
            }
        }
    }
    for path in [
        "~/.claude/projects/-Users-someone-repo/memory/note.md",
        "/Users/someone/.Claude/Projects/p/MEMORY/note.md", // one folder on macOS's file system
        "/Users/someone/.CODEX/Memories/note.md",
    ] {
        for (tool, input) in writes_to(path) {
            assert!(refused(tool, &input), "{tool} {path}");
        }
    }
}

#[test]
fn a_path_field_at_any_depth_is_read_and_the_text_written_is_not() {
    let nested = json!({"edits": [{"file_path": format!("{CLAUDE}/a.md")}]});
    assert!(refused("MultiEdit", &nested));
    let listed = json!({"paths": ["/repo/a.md", format!("{CODEX}/b.md")]});
    assert!(refused("Write", &listed));
    let mention = format!("Never write to {CLAUDE}/MEMORY.md");
    assert!(!refused(
        "Write",
        &json!({"file_path": "/repo/RULES.md", "content": mention})
    ));
    assert!(!refused(
        "Edit",
        &json!({"file_path": "/repo/AGENTS.md", "old_string": mention, "new_string": mention})
    ));
}

#[test]
fn reading_a_memory_folder_with_any_tool_passes() {
    let memory = format!("{CLAUDE}/MEMORY.md");
    assert!(!refused("Read", &json!({"file_path": memory})));
    assert!(!refused("Grep", &json!({"pattern": "x", "path": CLAUDE})));
    assert!(!refused("Glob", &json!({"pattern": "*.md", "path": CODEX})));
    for command in [
        format!("cat {CLAUDE}/MEMORY.md"),
        format!("ls -la {CLAUDE}/"),
        format!("head -5 {CODEX}/a.md | tail -2"),
        format!("grep -rn rule {CLAUDE} {CODEX}"),
        format!("rg -l rule {CLAUDE}"),
        format!("wc -l {CLAUDE}/*.md"),
        format!("find {CLAUDE} -name '*.md' -newer /repo/RULES.md"),
        format!("find {CLAUDE} -executable"),
        format!("sed -n '1,5p' {CLAUDE}/a.md"),
        format!("diff {CLAUDE}/a.md /repo/docs/a.md"),
        format!("cat {CLAUDE}/a.md 2>/dev/null"),
        format!("ls {CLAUDE} 2>&1 | head"),
        format!("ls {CLAUDE} > /dev/null 2>&1 && echo there"),
        format!("cat {CLAUDE}/a.md &>/dev/null || echo gone >&2"),
        format!("cat {CLAUDE}/a.md 2>\"/dev/null\" 1>/dev/stdout 3>&-"),
        format!("stat {CLAUDE}; git status"),
    ] {
        assert!(!bash(&command), "{command}");
    }
}

#[test]
fn every_shell_write_shape_into_either_memory_folder_is_refused() {
    for folder in [
        CLAUDE,
        CODEX,
        "~/.claude/projects/-Users-someone-repo/memory",
        "\"$HOME/.claude/projects/-Users-someone-repo/memory\"",
        "'/Users/someone/.codex/memories'",
    ] {
        for shape in [
            "echo hi > @/a.md",
            "echo hi >@/a.md",
            "echo hi >> @/MEMORY.md",
            "echo hi >| @/a.md",
            "printf x &> @/a.md",
            "printf x &>> @/a.md",
            "run 2> @/err.log",
            "run >& @/a.md",
            "cat > @/a.md <<'EOF'\nhi\nEOF",
            "cat <<'EOF' >@/a.md\nhi\nEOF",
            "echo hi | tee @/a.md",
            "echo hi | tee -a @/MEMORY.md >/dev/null",
            "cp /tmp/a.md @/a.md",
            "mv /tmp/a.md @/",
            "ln -s /tmp/a.md @/a.md",
            "rsync -a /tmp/notes/ @/",
            "install -m 644 /tmp/a.md @/a.md",
            "ditto /tmp/notes @",
            "scp host:notes.md @/",
            "tar -xf /tmp/notes.tar -C @",
            "unzip /tmp/notes.zip -d @",
            "zip -r @/notes.zip /tmp/notes",
            "gzip @/a.md",
            "gunzip @/a.md.gz",
            "curl -o @/a.md https://example.com/a.md",
            "wget -O @/a.md https://example.com/a.md",
            "rm -f @/note.md",
            "rm -rf @",
            "/bin/rm @/note.md",
            "rmdir @",
            "unlink @/a.md",
            "shred -u @/a.md",
            "touch @/a.md",
            "mkdir -p @/sub",
            "truncate -s 0 @/a.md",
            "chmod 600 @/a.md",
            "chown me @/a.md",
            "patch @/a.md /tmp/fix.diff",
            "dd if=/tmp/a.md of=@/a.md",
            "sed -i '' 's/old/new/' @/a.md",
            "sed -i.bak 's/old/new/' @/a.md",
            "sed -E -i 's/old/new/' @/a.md",
            "sed --in-place 's/old/new/' @/a.md",
            "perl -pi -e 's/old/new/' @/a.md",
            "python3 -c \"open('@/a.md', 'w').write('x')\"",
            "python -c \"import os; os.remove('@/a.md')\"",
            "node -e \"require('fs').writeFileSync('@/a.md', 'x')\"",
            "ruby -e \"File.write('@/a.md', 'x')\"",
            "bash -c 'echo hi > @/a.md'",
            "sh -c \"rm @/a.md\"",
            "eval \"rm @/a.md\"",
            "find @ -name '*.md' -delete",
            "find @ -type f -exec rm {} +",
            "ls @/* | xargs rm",
            "cd @ && echo hi >> MEMORY.md",
            "cd @; touch a.md",
            "awk 'BEGIN { system(\"rm @/a.md\") }'",
            "awk '{ print > \"@/a.md\" }' /tmp/in",
        ] {
            let command = shape.replace('@', folder);
            assert!(bash(&command), "not refused: {command}");
        }
    }
}

#[test]
fn a_shell_write_from_inside_a_memory_folder_is_refused_and_a_read_there_passes() {
    for folder in [CLAUDE, CODEX] {
        let write = json!({"command": "echo hi > MEMORY.md"});
        assert!(refused_in(folder, "Bash", &write), "{folder}");
        assert!(refused_in(&format!("{folder}/sub"), "Bash", &write));
        for read in ["cat MEMORY.md", "ls", "just tasks inbox --as h"] {
            assert!(!refused_in(folder, "Bash", &json!({"command": read})));
        }
    }
    let beside = format!("{CLAUDE}-old");
    let write = json!({"command": "echo hi > MEMORY.md"});
    assert!(!refused_in(&beside, "Bash", &write));
}

#[test]
fn a_path_that_only_resembles_a_memory_folder_passes() {
    for path in [
        "/repo/docs/memory/notes.md",
        "/repo/memory/MEMORY.md",
        "/Users/someone/.claude/projects/p/memory-notes/a.md",
        "/Users/someone/.claude/projects/p/memory.md",
        "/Users/someone/.claude/projects/p/notes/memory/a.md",
        "/Users/someone/.claude/projects/memory/a.md",
        "/Users/someone/.claude/projects-old/p/memory/a.md",
        "/Users/someone/.claude/memory/a.md",
        "/Users/someone/x.claude/projects/p/memory/a.md",
        "/Users/someone/claude/projects/p/memory/a.md",
        "/Users/someone/.codex/memory/a.md",
        "/Users/someone/.codex/memories-old/a.md",
        "/Users/someone/.codex/memories.md",
        "/Users/someone/.codex/sessions/memories/a.md",
        "/Users/someone/x.codex/memories/a.md",
    ] {
        for (tool, input) in writes_to(path) {
            assert!(!refused(tool, &input), "{tool} {path}");
        }
        for command in [
            format!("echo hi > {path}"),
            format!("rm -f {path}"),
            format!("sed -i '' s/a/b/ {path}"),
        ] {
            assert!(!bash(&command), "{command}");
        }
    }
}

#[test]
fn a_write_that_names_no_memory_folder_passes() {
    for command in [
        "echo hi > /tmp/note.md",
        "rm -rf build && mkdir build",
        "just verify",
        "python3 -c 'print(1)'",
        "find . -name '*.orig' -delete",
    ] {
        assert!(!bash(command), "{command}");
    }
}

#[test]
fn an_event_that_cannot_be_read_refuses_nothing() {
    let write = format!("{CLAUDE}/a.md");
    for raw in [
        b"not json".to_vec(),
        b"\xff".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        json!({"tool_name": ["Write"], "tool_input": {"file_path": write}})
            .to_string()
            .into_bytes(),
        json!({"tool_name": "Write", "tool_input": write})
            .to_string()
            .into_bytes(),
        json!({"tool_name": "Bash", "tool_input": {"command": 7}, "cwd": CLAUDE})
            .to_string()
            .into_bytes(),
        json!({"tool_name": "Bash"}).to_string().into_bytes(),
    ] {
        assert!(!refuses(&raw), "{}", String::from_utf8_lossy(&raw));
    }
}

#[test]
fn the_reason_is_one_line_naming_where_the_guidance_goes() {
    assert!(!REASON.contains('\n'));
    assert!(REASON.starts_with("Blocked: "));
    for place in [
        "repository",
        "RULES.md",
        "AGENTS.md",
        "skill",
        "doc",
        "task on the board",
    ] {
        assert!(REASON.contains(place), "{place}");
    }
}
