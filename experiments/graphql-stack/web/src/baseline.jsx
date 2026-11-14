// The web client's way today: React and a hand-written read (fetch, then state), no data library.
import { useEffect, useState } from "react";
import { Column, mount } from "./shared";

function Page() {
  const [state, setState] = useState({});
  useEffect(() => {
    let live = true;
    Promise.all([
      fetch("/api/board").then((r) => r.json()),
      fetch("/api/task-board?status=ready").then((r) => r.json()),
    ]).then(
      ([board, column]) => live && setState({ data: { board, column: column.tasks } }),
      (error) => live && setState({ error }),
    );
    return () => {
      live = false;
    };
  }, []);
  return <Column {...state} />;
}

mount(<Page />);
