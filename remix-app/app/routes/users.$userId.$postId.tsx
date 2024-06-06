import { LoaderFunctionArgs, MetaArgs } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const meta = ({ params }: MetaArgs) => [
  { title: `Post ${params.postId}` },
];

export function loader({ params }: LoaderFunctionArgs) {
  const { userId } = params;
  return {
    userId,
    postId: params.postId,
  };
}

export default function Splat() {
  const data = useLoaderData<typeof loader>();
  return (
    <h1>
      Post: {data.postId} by {data.userId}
    </h1>
  );
}
