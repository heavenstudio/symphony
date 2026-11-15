// A page kind's mount: the signal its entry's `run` gives the page and its `stop` aborts. Work a
// page starts that outlives the effect or the handler that started it (a click's post, a read with
// no cleanup of its own) checks it after each wait, so a kind the shell stopped does nothing more:
// it neither moves the reader, nor writes the window, the storage or the page the next kind shows.

import { createContext, useContext } from "react";

/** The mount of the kind drawn; one never stopped where no kind gives one (a unit test's draw). */
export const MountContext = createContext<AbortSignal>(new AbortController().signal);

/** The signal the shell's `stop` of this page's kind aborts. */
export function useMount(): AbortSignal {
  return useContext(MountContext);
}
