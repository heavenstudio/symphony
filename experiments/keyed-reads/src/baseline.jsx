// The page as a whole: one effect reads everything the page draws, and nothing is drawn until every
// read has answered. Each read still draws its kept copy first and names its tag, and the feed
// reads both again, as the client's pages do today.
import { useEffect, useState } from "react";
import {
  COLUMN,
  COUNTS,
  Column,
  Counts,
  getIf,
  keepLater,
  mount,
  openFeed,
  readKept,
} from "./shared";

const KEYS = [COUNTS, COLUMN];

function Page() {
  const [state, setState] = useState({});
  useEffect(() => {
    let live = true;
    const tags = {};
    const read = async () => {
      const answers = await Promise.all(KEYS.map((key) => getIf(key, tags[key])));
      if (!live) return;
      setState((shown) => {
        const next = { ...shown, stale: false };
        KEYS.forEach((key, at) => {
          const answer = answers[at];
          if (answer.same) return;
          tags[key] = answer.etag;
          keepLater(key, answer.etag, answer.value);
          next[key] = answer.value;
        });
        return next;
      });
    };
    Promise.all(KEYS.map(readKept)).then((kept) => {
      if (!live || kept.some((copy) => copy === null)) return;
      KEYS.forEach((key, at) => {
        tags[key] = kept[at].etag;
      });
      setState((shown) =>
        shown[COUNTS] ? shown : { [COUNTS]: kept[0].value, [COLUMN]: kept[1].value, stale: true },
      );
    });
    read().catch((error) => live && setState((shown) => ({ ...shown, error })));
    const close = openFeed(() => void read());
    return () => {
      live = false;
      close();
    };
  }, []);
  if (!state[COUNTS] || !state[COLUMN]) return <Column error={state.error} />;
  return (
    <>
      <Counts data={state[COUNTS]} stale={state.stale} />
      <Column data={state[COLUMN]} stale={state.stale} error={state.error} />
    </>
  );
}

mount(<Page />);
