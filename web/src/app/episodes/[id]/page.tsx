import { Suspense } from "react";
import EpisodeDetail from "./EpisodeDetail";

async function EpisodeContent({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <EpisodeDetail id={id} />;
}

export default function EpisodePage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  return (
    <Suspense
      fallback={
        <div className="min-h-screen flex items-center justify-center text-zinc-400">
          Loading...
        </div>
      }
    >
      <EpisodeContent params={params} />
    </Suspense>
  );
}
