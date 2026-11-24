// What an empty page says: one line that it has nothing yet, and the guide a new project starts
// from.

import type { ReactNode } from "react";
import { GETTING_STARTED } from "./wire";

/** An empty page's one line (`children`), then the guide's link, which leaves the Symphony and so
 * opens beside it. */
export function FirstSteps({ children }: { children: ReactNode }) {
  return (
    <p className="first-steps">
      {children}{" "}
      <a href={GETTING_STARTED} target="_blank" rel="noopener noreferrer">
        Getting started
      </a>
    </p>
  );
}

/** Whether a board whose stages count `counts` holds no task: none open, and none finished in
 * the window it shows. A board with no task counts no stage at all, and the board is drawn only
 * once its counts are read. */
export function noTasks(counts: [string, number][]): boolean {
  return counts.every(([, count]) => count === 0);
}
