// The task kind's tokens: what the task's model calls took, its subtasks' with them, and what
// they cost, estimated at API list prices, all told, by role and by review round, as `tasks show`
// says them (docs/TASKS.md, "Where the tokens go"). A chunk of its own, which the task page's
// disclosure imports as the reader first opens it (`TokensPart`, task-page.tsx), so no first visit
// carries it. Their part is the page's (task-live.ts's `tokensPart`), asked as the disclosure opens
// and again when the task changes, never at each heartbeat that adds to it: this chunk imports
// nothing of the task kind's own, as the timeline's does not, so the kind's entry stays one file.
// A task with none recorded says so in a line and draws no table. Its sheet comes inline and is put
// in the page as it first draws (`putStyle`), as the timeline's is: the build links no chunk's
// sheet of its own (vite.config.ts).
import { useLayoutEffect } from "react";
import { Button } from "./button";
import type { Shown } from "./parts";
import { said } from "./request";
import { field } from "./shared";
import {
  amount,
  calls,
  costSaid,
  dollars,
  estimate,
  isCost,
  isTokens,
  total,
  unpricedSaid,
} from "./spend";
import STYLE from "./task-tokens.css?inline";
import { failed } from "./trouble";
import type { TaskTokens } from "./wire";

/** What the disclosure says of an answer it cannot read. */
export const UNREADABLE = "The task's tokens are not readable.";

/** The id of the tokens' style element, put in the page once. */
const STYLE_ID = "task-tokens-style";

/** Put the tokens' style sheet in the page once: no first visit carries it. */
export function putStyle(): void {
  if (document.getElementById(STYLE_ID)) return;
  const element = document.createElement("style");
  element.id = STYLE_ID;
  element.textContent = STYLE;
  // Before the project's theme, as the shell links a kind's sheets: the theme's rules win.
  document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
}

const spent = (value: unknown) => isTokens(field(value, "tokens")) && isCost(field(value, "cost"));

/** A task's tokens as the wire writes them, or none. */
export function readTaskTokens(value: unknown): TaskTokens | null {
  const roles = field(value, "by_role");
  const rounds = field(value, "by_round");
  return isTokens(field(value, "total")) &&
    isCost(field(value, "cost")) &&
    Array.isArray(roles) &&
    roles.every((role) => typeof field(role, "role") === "string" && spent(role)) &&
    Array.isArray(rounds) &&
    rounds.every((round) => Number.isSafeInteger(field(round, "round")) && spent(round))
    ? (value as TaskTokens)
    : null;
}

/** A round as the terminal names it: the calls of a session that worked no task are round 0. */
export function roundName(round: number): string {
  return round === 0 ? "no task" : `round ${round}`;
}

export function TokenFigures({ tokens }: { tokens: TaskTokens }) {
  const { total: all, cost } = tokens;
  if (total(all) === 0 && all.calls === 0) return <p>No tokens recorded for this task.</p>;
  const priced = cost.dollars > 0 || cost.unpriced.length === 0;
  const rows = [
    ...tokens.by_role.map((role) => ({ ...role, key: `role ${role.role}`, name: role.role })),
    ...tokens.by_round.map((round) => ({
      ...round,
      key: `round ${round.round}`,
      name: roundName(round.round),
    })),
  ];
  return (
    <>
      <p>
        <strong>{amount(total(all))} tokens</strong> in {calls(all.calls)} (input{" "}
        {amount(all.input)} · cache write {amount(all.cache_write_5m + all.cache_write_1h)} · cache
        read {amount(all.cache_read)} · output {amount(all.output)})
      </p>
      {priced ? (
        <p>
          <strong>{dollars(cost.dollars)}</strong>, {estimate(cost.prices)} (input{" "}
          {dollars(cost.input)} · cache write {dollars(cost.cache_write_5m + cost.cache_write_1h)} ·
          cache read {dollars(cost.cache_read)} · output {dollars(cost.output)})
        </p>
      ) : (
        <p>
          <strong>Unpriced</strong>: the API list prices read {cost.prices} have none for these
          models.
        </p>
      )}
      {cost.unpriced.length > 0 && <p>No price for {unpricedSaid(cost)}: in no figure.</p>}
      {/* The shared table scrolls across itself where the screen is narrower. */}
      <table>
        <thead>
          <tr>
            <th scope="col" aria-label="Role or round" />
            <th scope="col">Tokens</th>
            <th scope="col">Calls</th>
            <th scope="col">Estimate</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <th scope="row">{row.name}</th>
              <td>{amount(total(row.tokens))}</td>
              <td>{row.tokens.calls}</td>
              <td>{costSaid(row.cost)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}

/** The body of the task page's disclosure of its tokens (`TokensPart`, task-page.tsx), which holds
 * its heading and their part (`answer`, read again by `retry`): the figures once read, why they
 * are not, or that they are being read. */
export function TaskTokensPanel({ answer, retry }: { answer: Shown<unknown>; retry: () => void }) {
  useLayoutEffect(putStyle, []);
  const tokens = answer.value === undefined ? undefined : readTaskTokens(answer.value);
  return (
    <div className="task-tokens-body" data-pending={`${!answer.answered && answer.error === null}`}>
      {(answer.error !== null || tokens === null) && (
        <p role="status">
          {answer.error !== null ? failed(said(answer.error), true) : UNREADABLE}{" "}
          <Button size="sm" disabled={answer.retrying} onClick={retry}>
            {answer.retrying ? "Reading tokens…" : "Retry"}
          </Button>
        </p>
      )}
      {tokens ? (
        <TokenFigures tokens={tokens} />
      ) : answer.error !== null || tokens === null ? null : answer.missing ? (
        <p>No tokens recorded for this task.</p>
      ) : (
        <p role="status" aria-busy="true">
          Reading the tokens…
        </p>
      )}
    </div>
  );
}
