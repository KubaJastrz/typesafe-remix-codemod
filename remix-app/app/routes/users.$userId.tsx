import { ActionFunctionArgs, LoaderFunctionArgs } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const meta = () => [{ title }];

const title = "User page";

export async function action({ params }: ActionFunctionArgs) {
  return {
    success: true,
  };
}

export const loader = async ({ params }: LoaderFunctionArgs) => {
  const { userId } = params;
  return { userId };
};

export default function Splat() {
  const data = useLoaderData<typeof loader>();
  return <h1>User: {data.userId}</h1>;
}
