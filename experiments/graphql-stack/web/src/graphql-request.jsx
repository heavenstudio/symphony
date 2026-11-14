// GraphQL with the smallest common client and no cache: one query, its answer kept in state.
import { request } from "graphql-request";
import { useEffect, useState } from "react";
import { Column, mount, QUERY } from "./shared";

function Page() {
  const [state, setState] = useState({});
  useEffect(() => {
    let live = true;
    request("/graphql", QUERY, { status: "ready" }).then(
      (data) => live && setState({ data }),
      (error) => live && setState({ error }),
    );
    return () => {
      live = false;
    };
  }, []);
  return <Column {...state} />;
}

mount(<Page />);
