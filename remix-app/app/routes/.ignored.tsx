// This file should be ignored in the codemod (see `ignoredRouteFiles` in Vite config)

import { useLoaderData } from "@remix-run/react";
import { createElement } from "react";

export function loader() {
  return Math.random();
}

export default function Ignored() {
  const number = useLoaderData<typeof loader>();
  return createElement("div", null, `Ignored: ${number}`);
}
