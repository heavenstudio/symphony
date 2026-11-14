// The chip naming the task that proposes a card's version, which more than one page draws: a
// module of its own, so a page that draws it loads no other page's code with it.

import { href } from "./project";

/** The task proposing a card's version, as a chip leading to its page. */
export function TaskTag({ task }: { task?: string | null | undefined }) {
  return task !== undefined && task !== null ? (
    <a className="chip" href={href(`/task/${task}`)}>
      {task}
    </a>
  ) : null;
}
