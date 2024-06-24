import { LinksFunction, LoaderFunctionArgs } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const links: LinksFunction = () => [{ rel: "stylesheet", href: "/styles.css" }];

export function loader({ params }: LoaderFunctionArgs) {
  const splat = params["*"];
  return { splat };
}

export default function Splat() {
  const { splat } = useLoaderData<typeof loader>();
  return <h1>Not found: {splat}</h1>;
}
