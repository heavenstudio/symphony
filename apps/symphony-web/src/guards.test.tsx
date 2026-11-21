import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  acceptedWords,
  asksAll,
  clock,
  entryRows,
  footWords,
  type Guarded,
  GuardRows,
  GuardRowsView,
  needRows,
  type Part,
  type Row,
  readGuards,
  readRefusal,
  referred,
  type Tried,
} from "./guards";
import { CASES, type Found, fire } from "./testing";
import type {
  WorkflowGuardAuthority,
  WorkflowGuardRecord,
  WorkflowGuardRefusal,
  WorkflowGuardStanding,
  WorkflowGuardWhy,
  WorkflowTaskGuards,
} from "./wire";

const GUARDS = readGuards(CASES.samples.taskGuards) as WorkflowTaskGuards;
const REFUSAL = readRefusal(CASES.samples.guardRefusal) as WorkflowGuardRefusal;
const RECORD = REFUSAL.record as WorkflowGuardRecord;
const PASS: Guarded = { slug: "w/b", edge: "pass", label: "Pass review", revision: 7 };
const NAMES = {
  place: ({ phase, state }: { phase: string; state: string }) => `${phase}/${state}`,
};
/** 13:40:07 UTC of a day, in milliseconds. */
const AT = Date.UTC(2026, 10, 21, 13, 40, 7);

/** A row's words as one text, a link as `[text](address)` and a command in backticks. */
const text = (parts: Part[]) =>
  parts
    .map((part) =>
      typeof part === "string"
        ? part
        : "code" in part
          ? `\`${part.code}\``
          : `[${part.text}](${part.link})`,
    )
    .join("");

/** The sample's guards with the `release` authority changed. */
const withAuthority = (change: Partial<WorkflowGuardAuthority>): WorkflowTaskGuards => ({
  ...GUARDS,
  authorities: GUARDS.authorities.map((one) =>
    one.adapter === "release" ? { ...one, ...change } : one,
  ),
});

/** Pass review's one row, for the board's records as given and the take last refused. */
const need = (guards = GUARDS, tried: Tried | null = null) =>
  needRows(PASS, guards, tried, NAMES)[0] as Row;

/** The row of a take refused by `why`, with the sample's record or `record`. */
const tried = (why: WorkflowGuardWhy, record: WorkflowGuardRecord | null = RECORD, said = "") =>
  need(GUARDS, { refusal: { ...REFUSAL, why, record }, at: AT, said });

describe("a task's guards as read", () => {
  it("are its authorities, its guards, the edges they hold and what let it in", () => {
    expect(GUARDS.authorities.map((one) => [one.adapter, one.generation, one.asked_here])).toEqual([
      ["design", 12, false],
      ["release", 41, true],
    ]);
    expect(GUARDS.guards.map((one) => one.page ?? null)).toEqual([
      null,
      "/docs/LEDGER.md#w/a:release_signoff",
    ]);
    expect(GUARDS.edges).toEqual([{ edge: "pass", guards: ["release_signoff"] }]);
    expect(GUARDS.entry?.guards[0]?.standing).toBe("decided_again");
    // A board with no workflow answers none of each, and no entry.
    expect(readGuards({ authorities: [], guards: [], edges: [] })).toEqual({
      authorities: [],
      guards: [],
      edges: [],
      entry: null,
    });
  });

  it("pass a field a newer server adds, and are none for a read the page cannot draw", () => {
    const sample = CASES.samples.taskGuards as Record<string, unknown>;
    expect(readGuards({ ...sample, later: 1 })).not.toBeNull();
    const broken: unknown[] = [
      null,
      { ...sample, authorities: [{ adapter: "release" }] },
      { ...sample, guards: [{ guard: "x", adapter: "y", page: 4 }] },
      { ...sample, edges: [{ edge: "pass", guards: [1] }] },
      { ...sample, entry: { edge: "submit" } },
      { ...sample, entry: { ...(sample.entry as object), guards: [{ guard: "x" }] } },
      { ...sample, entry: { ...(sample.entry as object), guards: "none" } },
    ];
    expect(broken.map(readGuards)).toEqual(broken.map(() => null));
    const [met] = GUARDS.entry?.guards ?? [];
    const unshown = { ...sample, entry: { ...GUARDS.entry, guards: [{ ...met, record: null }] } };
    expect(readGuards(unshown)?.entry?.guards[0]?.record).toBeNull();
    const unread = { ...sample, entry: { ...GUARDS.entry, guards: [{ ...met, record: {} }] } };
    expect(readGuards(unread)).toBeNull();
  });
});

describe("the guard a refusal names", () => {
  it("is read with what its attestation said, and without where it verified none", () => {
    expect(REFUSAL).toMatchObject({ guard: "release_signoff", adapter: "release", why: "refused" });
    expect(RECORD.reasons).toHaveLength(2);
    const bare = { guard: "approved", adapter: "approvals", why: "unreported" };
    expect(readRefusal(bare)).toEqual({ ...bare, record: null });
    // A word a newer server says is kept, and said in the refusal's own words.
    expect(readRefusal({ ...bare, why: "later" })?.why).toBe("later");
  });

  it("is none where the answer names none or the page cannot read it", () => {
    const unread = [undefined, null, {}, { guard: "a", adapter: "b" }, { guard: 1, why: "x" }];
    expect(unread.map(readRefusal)).toEqual(unread.map(() => null));
    expect(readRefusal({ ...REFUSAL, record: { result: "refuse" } })).toBeNull();
    expect(
      readRefusal({ ...REFUSAL, record: { ...RECORD, input_revisions: { a: 1 } } }),
    ).toBeNull();
    expect(readRefusal({ ...REFUSAL, record: { ...RECORD, input_revisions: null } })).toBeNull();
  });
});

describe("what an authority points at", () => {
  it("is a link named for the task, decision, run or document an app path names", () => {
    expect(referred("a blocker is still open /task/w/b")).toEqual([
      "a blocker is still open ",
      { link: "/task/w/b", text: "task w/b" },
    ]);
    expect(referred("approved in /docs/DECISIONS.md#design-signoff by its owner")).toEqual([
      "approved in ",
      { link: "/docs/DECISIONS.md#design-signoff", text: "decision design-signoff" },
      " by its owner",
    ]);
    expect(referred("/runs?run=r-12")).toEqual([{ link: "/runs?run=r-12", text: "run r-12" }]);
    expect(referred("/docs/PLAN.md#scope")).toEqual([
      { link: "/docs/PLAN.md#scope", text: "document PLAN.md" },
    ]);
    expect(referred("see /queue")).toEqual(["see ", { link: "/queue", text: "/queue" }]);
  });

  it("puts a web address at its end on the words before it, marked by its host", () => {
    expect(referred("the release check failed https://ci.example/runs/7")).toEqual([
      { link: "https://ci.example/runs/7", text: "the release check failed", out: "ci.example" },
    ]);
    expect(referred("HTTPS://Ci.example?x")).toEqual([
      { link: "HTTPS://Ci.example?x", text: "HTTPS://Ci.example?x", out: "Ci.example" },
    ]);
  });

  it("is its text for anything else: no script, protocol-relative or bare slash is a link", () => {
    for (const said of [
      "approver directory unreachable",
      "javascript:alert(1)",
      "see //evil.example/x",
      "either / or",
      "https://ci.example/runs/7 failed",
    ]) {
      expect(referred(said)).toEqual([said]);
    }
  });
});

describe("a time of the board's", () => {
  it("is said in UTC to the second, and as unknown where it is none", () => {
    expect(clock(AT / 1000)).toBe("13:40:07 UTC");
    expect(clock(Number.NaN)).toBe("an unknown time");
  });
});

describe("what the board accepted from an authority", () => {
  it("is its generation and epoch, none yet, in conflict, or no authority", () => {
    const [, release] = GUARDS.authorities;
    expect(acceptedWords(release)).toBe("accepted gen 41 · 2026");
    expect(acceptedWords({ ...release, conflicted: true } as WorkflowGuardAuthority)).toBe(
      "in conflict at gen 41 · 2026",
    );
    const unreported = { ...release, generation: null, epoch: null } as WorkflowGuardAuthority;
    expect(acceptedWords(unreported)).toBe("accepted: none yet");
    expect(acceptedWords(undefined)).toBe("no authority committed");
  });
});

describe("a guard an action needs, before any take", () => {
  it("says the action asks its authority, which this host does", () => {
    const row = need();
    expect(row).toMatchObject({ guard: "release_signoff", name: "release signoff", tone: "quiet" });
    expect(row.state).toBe("no word yet");
    expect(text(row.why)).toBe("Pass review asks ledger for its word when you take it.");
    expect(row.authority).toEqual({
      name: "ledger",
      page: "/docs/LEDGER.md#w/a:release_signoff",
      adapter: "release",
      accepted: "accepted gen 41 · 2026",
    });
    expect(row.record.title).toBe("What the board holds for this guard");
    expect(row.record.lines.map((line) => [line.label, text(line.parts)])).toEqual([
      ["From ledger", "accepted gen 41 · 2026"],
      ["Admitted on it", "3 operations"],
      ["Asked from here", "yes, when you take the action"],
    ]);
  });

  it("gives the command to copy where this app cannot ask its authority", () => {
    const row = need(withAuthority({ asked_here: false, admitted: 1 }));
    expect(row.state).toBe("not from here");
    expect(text(row.why)).toBe(
      "This app cannot ask ledger. With ledger's attestation file, from the command line: " +
        "`just tasks workflow apply w/b pass --revision 7 --attestation FILE`",
    );
    expect(text(row.record.lines[1]?.parts ?? [])).toBe("1 operation");
    expect(text(row.record.lines[2]?.parts ?? [])).toBe("no: this app cannot ask it");
  });

  it("says an authority that reported no state, one in conflict and one not committed", () => {
    const unreported = need(withAuthority({ generation: null, epoch: null }));
    expect([unreported.state, unreported.tone]).toEqual(["unreported", "amber"]);
    expect(text(unreported.why)).toContain("`just tasks workflow observe FILE`");
    expect(unreported.authority.accepted).toBe("accepted: none yet");
    const conflict = need(withAuthority({ conflicted: true }));
    expect([conflict.state, conflict.tone]).toEqual(["conflict", "red"]);
    expect(text(conflict.why)).toContain("ledger reported two decisions at generation 41");
    const none = need({ ...GUARDS, authorities: [] });
    expect([none.state, none.tone]).toEqual(["uncommitted", "red"]);
    expect(none.authority).toMatchObject({ name: "release", accepted: "no authority committed" });
    expect(text(none.record.lines[0]?.parts ?? [])).toBe("holds no authority for this guard");
  });

  it("is none for an action no guard holds", () => {
    expect(needRows({ ...PASS, edge: "rework" }, GUARDS, null, NAMES)).toEqual([]);
  });
});

describe("a guard the owner's take was refused by", () => {
  it("says the authority's reasons as links, and opens what the board read", () => {
    const row = tried("refused");
    expect([row.state, row.tone]).toEqual(["refused", "red"]);
    expect(text(row.why)).toBe(
      "Your Pass review at 13:40:07 UTC was refused. ledger says: " +
        "a blocker is still open [task w/a](/task/w/a) · " +
        "[the release check failed](https://ci.example/runs/7)",
    );
    expect(row.record.title).toBe(
      "What the board read from your Pass review at 13:40:07 UTC · kept on this page until the task moves or the page reloads",
    );
    expect(row.record.lines.map((line) => [line.label, text(line.parts)])).toEqual([
      ["ledger's word", "refuse · decided at generation 41 · 2026"],
      [
        "Its reasons",
        "a blocker is still open [task w/a](/task/w/a) · [the release check failed](https://ci.example/runs/7)",
      ],
      ["Its time", `issued ${clock(RECORD.issued_at)} · good until ${clock(RECORD.expires_at)}`],
      ["Covers", "review/awaiting, task revision 7, taken by you, inputs: design r4"],
      ["Signature", "verifies by the key the project commits for ledger"],
      ["From ledger", "accepted gen 41 · 2026"],
      ["Admitted on it", "3 operations"],
      ["Asked from here", "yes, when you take the action"],
    ]);
  });

  it("says each refusal by its word", () => {
    const agent = { ...RECORD, principal: { kind: "agent", issuer: "board", subject: "amber" } };
    const said: [WorkflowGuardWhy, string, string, string][] = [
      ["missing", "missing", "amber", "went with no attestation from ledger."],
      ["unverified", "unverified", "red", "does not verify by ledger's committed key"],
      ["unknown", "unknown", "quiet", "ledger cannot say: a blocker is still open"],
      ["unfounded", "unfounded", "amber", "passed it on no decision of its own"],
      ["expired", "expired", "amber", `expired at ${clock(RECORD.expires_at)}. Pass review again`],
      ["early", "early", "amber", `issued at ${clock(RECORD.issued_at)}, ahead of this machine`],
      ["overlong", "held too long", "amber", "holds for longer than an attestation may."],
      ["stale", "stale", "amber", "(task revision 7); the task is at revision 7."],
      ["decided_again", "decided again", "amber", "at generation 41 and has decided again since"],
      ["not_reported", "not reported", "amber", "which it has not reported to the board yet."],
    ];
    for (const [why, state, tone, words] of said) {
      const row = tried(why);
      expect([why, row.state, row.tone]).toEqual([why, state, tone]);
      expect(text(row.why)).toContain(words);
    }
    expect(text(tried("not_yours", agent).why)).toBe(
      "The attestation ledger gave covers the agent amber, not you.",
    );
    expect(text(tried("unanswered", null, "guard_unavailable: no answer").why)).toBe(
      "Your Pass review at 13:40:07 UTC asked ledger, which gave no attestation: guard_unavailable: no answer",
    );
  });

  it("says a refusal with no attestation to show in what it has, never a blank", () => {
    expect(text(tried("expired", null).why)).toContain("expired at an unknown time.");
    expect(text(tried("stale", null).why)).toContain("(task revision ?)");
    expect(text(tried("not_yours", null).why)).toContain("covers another principal, not you.");
    expect(text(tried("refused", null).why)).toBe(
      "Your Pass review at 13:40:07 UTC was refused. ledger says: ",
    );
    const bare = { ...RECORD, input_revisions: {}, facts: ["/task/w/a"], reasons: [] };
    const lines = tried("unknown", bare).record.lines.map((line) => [line.label, text(line.parts)]);
    expect(lines[1]).toEqual(["Rests on", "[task w/a](/task/w/a)"]);
    expect(lines[3]).toEqual(["Covers", "review/awaiting, task revision 7, taken by you"]);
  });

  it("says the board's own refusal, and a word it does not know, in the refusal's words", () => {
    const said = "guard_unavailable: ledger has reported no state";
    for (const why of ["unreported", "conflict", "uncommitted", "later" as WorkflowGuardWhy]) {
      const row = tried(why as WorkflowGuardWhy, null, said);
      expect([row.state, row.tone, text(row.why)]).toEqual(["refused", "red", said]);
      expect(row.record.lines[0]).toEqual({ label: "The board said", parts: [said] });
    }
  });

  it("is said only in the row of the guard it names", () => {
    const other: Tried = { refusal: { ...REFUSAL, guard: "design_signoff" }, at: AT, said: "" };
    expect(need(GUARDS, other).state).toBe("no word yet");
  });
});

describe("the guards that let a task in", () => {
  const standing = (word: WorkflowGuardStanding, change: Partial<WorkflowGuardAuthority> = {}) => {
    const entry = GUARDS.entry as NonNullable<WorkflowTaskGuards["entry"]>;
    const met = entry.guards.map((one) => ({ ...one, standing: word }));
    const authorities = GUARDS.authorities.map((one) => ({ ...one, ...change }));
    return entryRows({ ...GUARDS, authorities, entry: { ...entry, guards: met } }, NAMES)[0] as Row;
  };

  it("say one the authority decided again since no longer stands, and what it rested on", () => {
    const [row] = entryRows(GUARDS, NAMES) as [Row];
    expect([row.guard, row.state, row.tone]).toEqual(["design_signoff", "decided again", "amber"]);
    expect(text(row.why)).toBe(
      "Met at generation 11, resting on the design was approved " +
        "[decision design-signoff](/docs/DECISIONS.md#design-signoff). ledger has decided again " +
        "since (generation 12 now), so it no longer stands: no land is sent from here on it.",
    );
    expect(row.authority).toMatchObject({ name: "ledger", page: null, adapter: "design" });
    expect(row.record.title).toBe("What the board recorded when it let the task in");
    expect(text(row.record.lines[0]?.parts ?? [])).toBe(
      `Submit · revision 7 · ${clock(GUARDS.entry?.at ?? 0)}, taken by the agent amber-wren`,
    );
    expect(row.record.lines.map((line) => line.label)).toEqual([
      "Let in by",
      "ledger's word",
      "Rests on",
      "Its time",
      "Covers",
      "From ledger",
      "Admitted on it",
      "Asked from here",
    ]);
  });

  it("say each standing by its word", () => {
    const met = standing("met");
    expect([met.state, met.tone]).toEqual(["met", "green"]);
    expect(text(met.why)).toContain(", which the board still accepts.");
    const said: [WorkflowGuardStanding, string, string, string][] = [
      ["conflict", "conflict", "red", "ledger is in conflict now, so it no longer stands"],
      ["unreported", "unreported", "amber", "The board holds no state from ledger now"],
      ["uncommitted", "uncommitted", "red", "The active workflow no longer commits design"],
      ["unverified", "unverified", "red", "no longer verifies by the key committed for ledger"],
      ["later" as WorkflowGuardStanding, "not standing", "amber", ", and so it no longer stands"],
    ];
    for (const [word, state, tone, words] of said) {
      const row = standing(word);
      expect([word, row.state, row.tone]).toEqual([word, state, tone]);
      expect(text(row.why)).toContain(words);
    }
    expect(text(standing("decided_again", { generation: null }).why)).toContain(
      "(another generation now)",
    );
  });

  it("say an admission whose attestation is not shown, and none where nothing let it in", () => {
    const entry = GUARDS.entry as NonNullable<WorkflowTaskGuards["entry"]>;
    const unshown = entry.guards.map((one) => ({ ...one, record: null }));
    const [row] = entryRows({ ...GUARDS, entry: { ...entry, guards: unshown } }, NAMES) as [Row];
    expect(text(row.why)).toMatch(/^Met at generation 11\. ledger has decided again/);
    expect(row.record.lines[1]).toEqual({
      label: "ledger's word",
      parts: ["its attestation is not shown"],
    });
    expect(entryRows({ ...GUARDS, entry: null }, NAMES)).toEqual([]);
  });
});

describe("an action its guards alone hold", () => {
  it("goes from this app only when this host asks every guard's authority", () => {
    expect(asksAll("pass", GUARDS)).toBe(true);
    expect(asksAll("pass", withAuthority({ asked_here: false }))).toBe(false);
    expect(asksAll("pass", { ...GUARDS, authorities: [] })).toBe(false);
    expect(asksAll("rework", GUARDS)).toBe(false);
    expect(asksAll("pass", null)).toBe(false);
    const two = {
      ...GUARDS,
      edges: [{ edge: "pass", guards: ["release_signoff", "design_signoff"] }],
    };
    expect(asksAll("pass", two)).toBe(false);
  });
});

describe("who decides", () => {
  it("names each authority once, and says nothing here approves", () => {
    const row = need();
    expect(footWords([row], "Pass review")).toBe(
      "ledger decides this in the project's own ledger. Pass review passes on its signed word, which the board checks; nothing on this page approves.",
    );
    const other = { ...row, authority: { ...row.authority, name: "board of review" } };
    expect(footWords([row, row], "Pass review")).toMatch(/^ledger decides this/);
    expect(footWords([row, other], "Pass review")).toBe(
      "ledger and board of review decide this in the project's own ledger. Pass review passes on their signed word, which the board checks; nothing on this page approves.",
    );
  });
});

describe("the rows drawn", () => {
  const rows = [tried("refused")];
  const view = (open = "", onOpen = vi.fn()) =>
    GuardRowsView({
      of: "pass",
      label: "Pass review needs",
      heading: "Pass review needs",
      rows,
      foot: "ledger decides",
      open,
      onOpen,
    });
  const name = (element: Found) => element.props.className === "guard-open";

  it("are a guard's name, state, why and authority, its record closed", () => {
    const drawn = renderToStaticMarkup(
      <GuardRows of="pass" label="Pass review needs" heading="Pass review needs" rows={rows} />,
    );
    expect(drawn).toContain('<section class="guard-box" aria-label="Pass review needs"');
    expect(drawn).toContain('class="guard guard-red" data-guard="release_signoff"');
    expect(drawn).toContain(
      '<button type="button" class="guard-open" aria-expanded="false" aria-controls="guard-record-pass-release_signoff"',
    );
    expect(drawn).toContain('<span class="pill guard-state">refused</span>');
    // A link of the app is written under the project's base; one that leaves it opens a tab of
    // its own, with its host beside it.
    expect(drawn).toContain('<a href="/task/w/a">task w/a</a>');
    expect(drawn).toContain(
      '<a class="guard-out" href="https://ci.example/runs/7" target="_blank" rel="noreferrer">the release check failed</a><span class="guard-host"> ci.example</span>',
    );
    // The authority's name is the link to its ledger's page for the guard.
    expect(drawn).toContain('<a href="/docs/LEDGER.md#w/a:release_signoff">ledger</a> · release');
    expect(drawn).toContain('<span class="guard-accepted">accepted gen 41 · 2026</span>');
    expect(drawn).not.toContain("guard-record-title");
    expect(drawn).not.toContain("guard-foot");
    // Nothing in the box sends, approves or takes a file.
    expect(drawn).not.toMatch(/<input|<form|<textarea|type="submit"/);
    expect(drawn.match(/<button/g)).toHaveLength(1);
  });

  it("open the board's record under the row its name was pressed on, and close it again", () => {
    const onOpen = vi.fn();
    fire(view("", onOpen), name, "onClick");
    expect(onOpen).toHaveBeenLastCalledWith("release_signoff");
    const drawn = renderToStaticMarkup(view("release_signoff", onOpen));
    expect(drawn).toContain('aria-expanded="true"');
    expect(drawn).toContain('<div class="guard-record" id="guard-record-pass-release_signoff">');
    expect(drawn).toContain("<dt>Its reasons</dt>");
    expect(drawn).toContain('<p class="guard-foot">ledger decides</p>');
    fire(view("release_signoff", onOpen), name, "onClick");
    expect(onOpen).toHaveBeenLastCalledWith("");
  });

  it("draw a command as code, a web page of the authority's in its own tab, and no box of none", () => {
    const apart = need(withAuthority({ asked_here: false }));
    const out = { ...apart, authority: { ...apart.authority, page: "https://ledger.example/w" } };
    const bare = {
      ...apart,
      authority: { ...apart.authority, page: "javascript:1", adapter: "ledger" },
    };
    const drawn = renderToStaticMarkup(
      <GuardRows of="pass" label="needs" heading="needs" rows={[out]} />,
    );
    expect(drawn).toContain("<code>just tasks workflow apply w/b pass --revision 7");
    expect(drawn).toContain(
      '<a class="guard-out" href="https://ledger.example/w" target="_blank" rel="noreferrer">ledger</a>',
    );
    const plain = renderToStaticMarkup(
      <GuardRows of="pass" label="needs" heading="needs" rows={[bare]} />,
    );
    // A page that is no link is no link, and an adapter named as its issuer is said once.
    expect(plain).toContain('<span class="guard-by"><span>ledger</span>');
    expect(renderToStaticMarkup(<GuardRows of="x" label="x" heading="x" rows={[]} />)).toBe("");
  });
});
