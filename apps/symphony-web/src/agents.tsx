// The agents page (`/agents`), as the Dioxus program drew it: who is connected and what each works
// on, the tasks happening now, the machines' build and test context, and the lost agents on demand.
// Its reads and its rules are agents-live.ts's; this draws them.

import { Fragment, useEffect, useReducer, useRef, useState } from "react";
import {
  type AgentsRead,
  agentLine,
  agentSpent,
  connectedAgents,
  connection,
  Followed,
  finished,
  followedTasks,
  followingLabel,
  happening,
  held,
  LiveAgents,
  lostAgents,
  occupancy,
  workingOn,
} from "./agents-live";
import { Button } from "./button";
import { useFrame } from "./frame";
import { markBoardReady, PageNotices } from "./live";
import { useMount } from "./mount";
import { href } from "./project";
import { scrollAnchor } from "./route";
import { GoToSession } from "./session";
import { textOrder } from "./task";
import { PhaseChips, statusWords, TaskCard } from "./taskcard";
import { failed, ready, Trouble } from "./trouble";
import { NearViewport, Skeleton } from "./viewport";
import type { Agent, MachineObservation, MachineWork, Task, WorkGuidance } from "./wire";
import { shownScoped } from "./work";

/** No names read: a board its own writers keep, or a workflow's before its names arrive. */
const NO_NAMES: ReadonlyMap<string, string> = new Map();

/**
 * What a task's line says it stands at: where a custom workflow holds it, its place by the names
 * its IDs read as (docs/WORKFLOW.md, "The app"), its phase's and state's IDs until they are read;
 * else its status.
 */
export function standing(task: Task, names: ReadonlyMap<string, string>): string {
  const place = task.workflow;
  if (place === undefined || place === null) return statusWords(task.status);
  return (
    names.get(`${place.phase}/${place.state}`) ??
    `${statusWords(place.phase)} · ${statusWords(place.state)}`
  );
}

/**
 * The names a custom workflow's IDs read as, read through the workflow's own chunk once a task
 * the page holds has a place: none on a board its own writers keep, which loads nothing more. A
 * chunk or a read that fails leaves the IDs as they are.
 */
function useNames(scope: string, tasks: Task[]): ReadonlyMap<string, string> {
  const [names, setNames] = useState(NO_NAMES);
  const held = tasks.some((task) => task.workflow !== undefined && task.workflow !== null);
  useEffect(() => {
    if (!held) return;
    const controller = new AbortController();
    import("./flow")
      .then((flow) => flow.readNames(scope, controller.signal))
      .then((read) => {
        if (read !== null) setNames(read);
      })
      .catch(() => {});
    return () => controller.abort();
  }, [scope, held]);
  return names;
}

/** The agents page, under the board it follows. */
export function AgentsPage({ scope }: { scope: string }) {
  const frame = useFrame();
  const [, redraw] = useReducer((count: number) => count + 1, 0);
  const [boardRead, setBoardRead] = useState(false);
  const [stale, setStale] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const live = useRef<LiveAgents | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    const agents = new LiveAgents(
      scope,
      { redraw, board: () => setBoardRead(true), stale: setStale, trouble: setTrouble },
      controller.signal,
    );
    live.current = agents;
    redraw();
    void agents.start(frame);
    return () => controller.abort();
  }, [scope, frame]);
  useEffect(() => {
    if (boardRead) markBoardReady();
  }, [boardRead]);
  return <AgentsMain live={live.current} boardRead={boardRead} stale={stale} trouble={trouble} />;
}

export interface AgentsMainProps {
  live: LiveAgents | null;
  /** The board was read: the page under it is drawn. */
  boardRead: boolean;
  stale: boolean;
  /** Why the board could not be read, until it is. */
  trouble: string | null;
}

/** The page as its board and its agents stand, under its notices. */
export function AgentsMain({ live, boardRead, stale, trouble }: AgentsMainProps) {
  const shown = boardRead && live !== null;
  return (
    <main
      data-ready={shown ? "board" : ready(false, trouble !== null)}
      data-feed={stale ? "stale" : "live"}
    >
      {shown ? (
        <AgentsContent live={live} connected={!stale} />
      ) : trouble !== null ? (
        <Trouble
          title="The board could not be loaded"
          text={failed(trouble, true)}
          retry={() => {
            if (live !== null) live.retry.current += 1;
          }}
        />
      ) : (
        <Skeleton lines={8} label="Loading the board…" />
      )}
      {/* A read that failed once the agents are drawn is a notice, out of the page's flow. */}
      <PageNotices
        reconnecting={stale && boardRead}
        error={live !== null && live.d.view !== null ? live.d.error : ""}
      />
    </main>
  );
}

/** The agents, or why they are not drawn yet. */
function AgentsContent({ live, connected }: { live: LiveAgents; connected: boolean }) {
  const { d } = live;
  const view = d.view;
  const arrived = view !== null;
  const names = useNames(live.scope, d.tasks);
  // A link to a part of the page (`/agents#machine-context`) names an element that mounts only
  // once the agents arrive: located then.
  useEffect(() => {
    if (arrived) scrollAnchor();
  }, [arrived]);
  return (
    <div className="task-page" data-ready={view !== null ? "agents" : ready(false, d.error !== "")}>
      {view !== null ? (
        <AgentsView
          view={view}
          work={connection(view.work, connected)}
          local={d.local}
          tasks={d.tasks}
          total={d.total}
          more={d.agentCursor !== null}
          lostMore={!d.lostLoaded || d.lostCursor !== null}
          onMore={live.more}
          scope={live.scope}
          names={names}
        />
      ) : d.error !== "" ? (
        <Trouble
          title="The agents could not be loaded"
          text={failed(d.error, true)}
          retry={() => void live.load()}
        />
      ) : (
        <p>Loading agents…</p>
      )}
    </div>
  );
}

export interface AgentsViewProps {
  view: AgentsRead;
  /** The machines' work as the page shows it: all of it disconnected while the feed is lost. */
  work: MachineWork;
  local: boolean;
  tasks: Task[];
  total: number;
  /** Whether the server has more connected agents, and more lost ones. */
  more: boolean;
  lostMore: boolean;
  /** Read the next page of the connected agents, or of the lost ones (`true`). */
  onMore: (lost: boolean) => void;
  scope: string;
  /** What a custom workflow's IDs read as, where one holds the page's tasks. */
  names?: ReadonlyMap<string, string>;
}

/** Who is connected, what is happening now, the machines' context and the lost agents. */
export function AgentsView({
  view,
  work,
  local,
  tasks,
  total,
  more,
  lostMore,
  onMore,
  scope,
  names = NO_NAMES,
}: AgentsViewProps) {
  const connected = connectedAgents(view.agents);
  const row = (agent: Agent) => (
    <AgentRow agent={agent} local={local} tasks={tasks} work={work} scope={scope} names={names} />
  );
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Agents</h1>
          <p className="lead">Who is connected, and what they are working on.</p>
        </div>
      </div>
      <p className="muted">{`Terminal panes: ${view.probes.panes} · Test slots: ${view.probes.slots}`}</p>
      <h2>Happening now</h2>
      <div className="agents-now">
        {happening(tasks).map((task) => (
          <NearViewport key={task.slug} id={`now-${task.slug}`} height={120}>
            <TaskCard task={task} work={work} />
          </NearViewport>
        ))}
      </div>
      <MachinesView work={work} />
      <h2>Connected</h2>
      <div className="agents agents-card card">
        {connected.map((agent) => (
          <NearViewport key={agent.handle} id={`agent-${agent.handle}`} height={240}>
            {row(agent)}
          </NearViewport>
        ))}
      </div>
      {more && connected.length < total && (
        <NearViewport id="more-agents" onEnter={() => onMore(false)}>
          <Button onClick={() => onMore(false)}>Load more agents</Button>
        </NearViewport>
      )}
      {/* Each time it opens or closes, the next page of the lost agents, the first the first time. */}
      <details onToggle={() => onMore(true)}>
        <summary>Lost agents</summary>
        <div>
          {lostAgents(view.agents).map((agent) => (
            <NearViewport key={agent.handle} id={`agent-${agent.handle}`} height={120}>
              {row(agent)}
            </NearViewport>
          ))}
          {lostMore && <Button onClick={() => onMore(true)}>Load more lost agents</Button>}
        </div>
      </details>
    </>
  );
}

export interface AgentRowProps {
  agent: Agent;
  local: boolean;
  /** The page's tasks, of which the agent holds some. */
  tasks: Task[];
  work: MachineWork;
  scope: string;
  /** What a custom workflow's IDs read as, where one holds the page's tasks. */
  names?: ReadonlyMap<string, string>;
}

/** One agent: its line, its way into its session, its work, how to reach it and what it holds. */
export function AgentRow({ agent, local, tasks, work, scope, names = NO_NAMES }: AgentRowProps) {
  const [, redraw] = useReducer((count: number) => count + 1, 0);
  const mount = useMount();
  const [following] = useState(() => new Followed(agent.handle, scope, mount, redraw));
  return (
    <AgentLines
      agent={agent}
      local={local}
      tasks={tasks}
      work={work}
      following={following}
      names={names}
    />
  );
}

export interface AgentLinesProps extends Omit<AgentRowProps, "scope"> {
  /** The open tasks it orchestrates, as its Following list has read them. */
  following: Followed;
}

/** An agent's row as it stands. */
export function AgentLines({
  agent,
  local,
  tasks,
  work,
  following,
  names = NO_NAMES,
}: AgentLinesProps) {
  const { handle, following: count } = agent;
  const holding = held(tasks, handle);
  const done = finished(holding);
  const followed = followedTasks(following.rows, tasks, handle);
  const spent = agentSpent(agent);
  return (
    <div className="agent">
      <strong>{handle}</strong>{" "}
      <span className={`chip health-${agent.health}`}>{agent.health}</span>
      <p className="muted">{agentLine(agent)}</p>
      {spent !== null && <p className="muted agent-spent">{spent}</p>}
      <GoToSession handle={handle} go={agent.go} local={local} scope={following.scope} />
      <PhaseChips rows={shownScoped(work, { agent: handle }, true)} />
      <details className="agent-connection">
        <summary>Connection details</summary>
        {agent.callback !== null ? (
          <>
            <p>
              {"Native callback: "}
              <code>{agent.callback}</code>
            </p>
            <p>
              {"Send: "}
              <code>{`just agent-send ${handle} "text"`}</code>
            </p>
          </>
        ) : (
          <>
            <p>
              {"Send on a task: "}
              <code>{`just tasks say SLUG "text" --to ${handle}`}</code>
            </p>
            <p>
              {"No channel registered — "}
              <code>{`just tasks inbox --as ${handle}`}</code>
            </p>
            <p>
              {"Register in that agent's session: "}
              <code>{`just tasks callback --as ${handle}`}</code>
            </p>
          </>
        )}
      </details>
      {agent.chat && <p>{`Chat goes to ${handle}.`}</p>}
      {agent.parent !== null && <p>{`parent: ${agent.parent}`}</p>}
      {/* Its slots (crates/tasks/src/slots.rs), PULL's word: the tasks it is building, apart from
          the open tasks it follows. */}
      {agent.slots !== null && <p>{`slots: ${agent.slots.open} of ${agent.slots.cap}`}</p>}
      {agent.state_detail !== null && (
        <p>{`${agent.state_detail.recipe} · ${agent.state_detail.phase}`}</p>
      )}
      <ul>
        {workingOn(holding).map(([task, role]) => (
          <li key={task.slug}>
            <a href={href(`/task/${task.slug}`)}>{task.title}</a>
            {` · ${role.role} · ${standing(task, names)}`}
          </li>
        ))}
      </ul>
      {/* An agent following nothing offers nothing to open. */}
      {count > 0 && (
        <details onToggle={(event) => following.toggle(event.currentTarget.open)}>
          <summary>{followingLabel(count)}</summary>
          {following.open && (
            <>
              <p role="status">{following.trouble}</p>
              <ul>
                {followed.map((task) => (
                  <NearViewport
                    key={task.slug}
                    listItem
                    id={`following-${handle}-${task.slug}`}
                    height={40}
                  >
                    <a href={href(`/task/${task.slug}`)}>{task.title}</a>
                    {` · ${standing(task, names)}`}
                  </NearViewport>
                ))}
              </ul>
              {followed.length < count && (
                <Button onClick={() => void following.load(true)}>Load more followed tasks</Button>
              )}
            </>
          )}
        </details>
      )}
      {/* A dot between two finished tasks, none after the last. */}
      {done.length > 0 && (
        <p className="muted">
          {"Finished: "}
          {done.slice(0, 5).map((task, at) => (
            <Fragment key={task.slug}>
              {at > 0 && " · "}
              <a href={href(`/task/${task.slug}`)}>{task.title}</a>
            </Fragment>
          ))}
          {done.length > 5 && ` · and ${done.length - 5} more`}
        </p>
      )}
      {agent.last_message !== null && (
        <p>
          <a href={href(`/task/${agent.last_message.slug}`)}>Latest message</a>
          {`: ${agent.last_message.text}`}
        </p>
      )}
    </div>
  );
}

/** Machine occupancy, apart from any task's lifecycle and never an admission rule. */
export function MachinesView({ work }: { work: MachineWork }) {
  const listed = new Set(work.machines.map((machine) => machine.machine_id));
  const advised = Object.keys(work.guidance)
    .sort(textOrder)
    .filter((id) => !listed.has(id));
  return (
    <details id="machine-context" className="machine-work" aria-label="Machine build and test work">
      <summary>Machine context</summary>
      {work.machines.length === 0 && <p className="muted">Machine occupancy unknown</p>}
      {work.machines.map((machine) => (
        <MachineRow key={machine.machine_id} machine={machine} work={work} />
      ))}
      {advised.map((id) => (
        <div key={id} className="machine-work-row" data-machine-id={id}>
          <strong>{id}</strong>
          <p>Machine observation unknown</p>
          <Guidance id={id} guidance={work.guidance[id]} />
        </div>
      ))}
      {work.unassigned.length > 0 && (
        <>
          <p className="muted">
            Some work has no proven machine identity; capacity remains unknown.
          </p>
          <PhaseChips rows={shownScoped(work, "unassigned", true)} />
        </>
      )}
    </details>
  );
}

function MachineRow({ machine, work }: { machine: MachineObservation; work: MachineWork }) {
  return (
    <div className="machine-work-row" data-machine-id={machine.machine_id}>
      <strong>{machine.label}</strong>
      <p>{occupancy(machine)}</p>
      <Guidance id={machine.machine_id} guidance={work.guidance[machine.machine_id]} />
      <PhaseChips rows={shownScoped(work, { machine: machine.machine_id }, true)} />
      {machine.reason !== "" && <p className="muted">{machine.reason}</p>}
    </div>
  );
}

/** A machine's advisory targets: what orchestrators may aim at, never what the board admits. */
function Guidance({ id, guidance }: { id: string; guidance: WorkGuidance | undefined }) {
  const target = (value: number | null | undefined) =>
    value === null || value === undefined ? "unknown" : String(value);
  return (
    <details className="machine-guidance">
      <summary>{`Advisory targets · builds ${target(guidance?.build)} · tests ${target(guidance?.test)}`}</summary>
      <p className="muted">
        Orchestrators decide when to start work. These targets do not enforce admission.
      </p>
      <p className="muted">
        {"Machine ID: "}
        <code>{id}</code>
      </p>
    </details>
  );
}
