import type { MetaFunction } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const meta: MetaFunction = () => {
  return [{ title: "New Remix App" }];
};

export function loader() {
  return { message: "Hello from loader!" };
}

// This is a comment
export default function Index() {
  const { message } = useLoaderData<typeof loader>();
  return <h1>{message}</h1>;
}
