import { Suspense } from "react";
import EpisodeDetail from "./EpisodeDetail";

async function EpisodeContent({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { id } = await params;
  const sp = await searchParams;
  // ?t=<seconds> deep link (INV-1914): seek there once the player is ready.
  const raw = Array.isArray(sp?.t) ? sp?.t[0] : sp?.t;
  const t = Number.parseFloat(typeof raw === "string" ? raw : "");
  return <EpisodeDetail id={id} initialTime={Number.isFinite(t) && t > 0 ? t : undefined} />;
}

export default function EpisodePage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  return (
    <Suspense
      fallback={
        <div className="min-h-screen flex items-center justify-center text-zinc-400">
          Loading...
        </div>
      }
    >
      <EpisodeContent params={params} searchParams={searchParams} />
    </Suspense>
  );
}
