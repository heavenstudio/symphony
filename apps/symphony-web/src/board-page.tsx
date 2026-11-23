// A page that follows the board as a whole, home's and the queue's (`BoardPage`): it draws what this
// device kept, reads the board, follows its feed, and draws what it heard under its notices, the
// board's loop and its notices being live.tsx's. Apart from live.tsx, which the pages that follow
// one part of the board share with them (the board's, a task's, the agents', a document's), so those
// pages load none of it.

import { createContext, type ReactNode, useContext, useEffect, useRef, useState } from "react";
import {
  BOARD,
  boardItem,
  drawn,
  FRESH,
  type Freshness,
  type Item,
  readKept,
  shown,
  visitLater,
} from "./cache";
import { useFrame } from "./frame";
import {
  keepBoard,
  markBoardReady,
  type PageBoard,
  PageNotices,
  Retry,
  readPageBoard,
  type Views,
} from "./live";
import { failed as failedRead, ready, Trouble } from "./trouble";
import { Skeleton } from "./viewport";

/** Changes what the page shows from this device; the views under the board read it. */
const FreshnessContext = createContext<(change: (f: Freshness) => Freshness) => void>(() => {});

export function useFreshen(): (change: (f: Freshness) => Freshness) => void {
  return useContext(FreshnessContext);
}

export interface BoardPageProps<B extends PageBoard = PageBoard> {
  scope: string;
  /** Whether the page draws the board this device kept, and keeps the one it reads. */
  kept: boolean;
  /** The page under the board, once the board is drawn: given how many times it was read, and it. */
  children: (refreshed: number, board: B) => ReactNode;
  /** Each feed event the page under the board hears, by whether it moves what it shows. */
  moves?: (delta: unknown) => boolean;
  /**
   * The board as a page that draws it reads it, its server's answer and its kept copy alike
   * (`readPageBoard` by default).
   */
  read?: (value: unknown) => B | null;
  /**
   * Each feed event applied to the board such a page draws: the board after it, or none for an
   * event it cannot read, which loses the feed.
   */
  apply?: (board: B, delta: unknown) => B | null;
}

/**
 * One feed event as a board page hears it: applied to the board it draws by `apply`, when the page
 * draws the whole board and has it, the board after it shown; then the page under it read again
 * (`refresh`) when `moves` says the event moves what it shows. `false` when `apply` cannot read the
 * event, which loses the stream, with nothing shown or read again.
 */
export function hearBoard<B extends PageBoard>(
  delta: unknown,
  board: { current: B | null },
  show: (board: B) => void,
  apply: ((board: B, delta: unknown) => B | null) | undefined,
  moves: ((delta: unknown) => boolean) | undefined,
  refresh: () => void,
): boolean {
  if (apply && board.current) {
    const next = apply(board.current, delta);
    if (next === null) return false;
    show(next);
  }
  if (moves?.(delta)) refresh();
  return true;
}

/**
 * The board this device kept for `item`, as the page reads it, drawn while the server's is read:
 * never over a board the page drew by then, nor once the page is gone. `freshen` says it is drawn
 * from this device, and since when.
 */
export async function drawKept<B extends PageBoard>(
  item: Item,
  read: (value: unknown) => B | null,
  board: { current: B | null },
  show: (board: B) => void,
  freshen: (change: (f: Freshness) => Freshness) => void,
  signal: AbortSignal,
): Promise<void> {
  const copy = await readKept(item, BOARD, read);
  if (copy && !board.current && !signal.aborted) {
    show(copy.value);
    freshen((f) => drawn(f, BOARD, copy.saved));
  }
}

/**
 * A page that follows the board: it draws what this device kept, reads the board, follows its
 * feed, and shows what it heard in `children`, under its notices.
 */
export function BoardPage<B extends PageBoard = PageBoard>({
  scope,
  kept,
  children,
  moves,
  read = readPageBoard as (value: unknown) => B | null,
  apply,
}: BoardPageProps<B>) {
  const frame = useFrame();
  const board = useRef<B | null>(null);
  const [drawnBoard, setDrawnBoard] = useState<B | null>(null);
  const [stale, setStale] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const [freshness, setFreshness] = useState<Freshness>(FRESH);
  const [refreshed, setRefreshed] = useState(0);
  const [retry] = useState(() => new Retry());
  const movesRef = useRef(moves);
  movesRef.current = moves;
  const applyRef = useRef(apply);
  applyRef.current = apply;
  const readRef = useRef(read);
  readRef.current = read;
  useEffect(() => {
    const controller = new AbortController();
    const signal = controller.signal;
    const item: Item | null = kept ? boardItem(scope) : null;
    const show = (read: B) => {
      board.current = read;
      setDrawnBoard(read);
    };
    if (item) {
      visitLater(item);
      void drawKept(item, readRef.current, board, show, setFreshness, signal);
    }
    const views: Views = {
      freshen: (change) => setFreshness(change),
      read: () => setRefreshed((count) => count + 1),
      heard: (delta) =>
        hearBoard(delta, board, show, applyRef.current, movesRef.current, () =>
          setRefreshed((count) => count + 1),
        ),
      frame,
    };
    const run = { board, show, setStale, setTrouble, retry, views, read: readRef.current };
    void keepBoard(scope, item, run, signal);
    return () => controller.abort();
  }, [scope, kept, frame, retry]);
  const onPage = drawnBoard !== null;
  useEffect(() => {
    if (onPage) markBoardReady();
  }, [onPage]);
  const device = shown(freshness);
  return (
    <FreshnessContext.Provider value={setFreshness}>
      <main
        data-ready={onPage ? "board" : ready(false, trouble !== null)}
        data-feed={stale || device ? "stale" : "live"}
        data-cache={device ? "device" : undefined}
      >
        {drawnBoard !== null ? (
          children(refreshed, drawnBoard)
        ) : trouble !== null ? (
          <Trouble
            title="The board could not be loaded"
            text={failedRead(trouble, true)}
            retry={() => {
              retry.current += 1;
            }}
          />
        ) : (
          <Skeleton lines={8} label="Loading the board…" />
        )}
        <PageNotices reconnecting={stale && onPage} device={device} />
      </main>
    </FreshnessContext.Provider>
  );
}
