import { fetchRegistry } from '@/lib/registry';

interface ModDetailPageProps {
  params: { id: string };
}

export async function generateStaticParams() {
  const mods = await fetchRegistry();
  return mods.map((mod) => ({ id: mod.id }));
}

export default async function ModDetailPage({ params }: ModDetailPageProps) {
  const mods = await fetchRegistry();
  const mod = mods.find((m) => m.id === params.id);

  if (!mod) {
    return (
      <div className="rounded-xl border border-red-200 bg-red-50 p-6 dark:border-red-900 dark:bg-red-900/20">
        <h2 className="text-xl font-bold text-red-700 dark:text-red-300">Mod not found</h2>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold">{mod.name}</h2>
        <p className="text-gray-600 dark:text-gray-400">
          {mod.content_type} · {mod.download_strategy}
        </p>
      </div>

      <div className="rounded-xl border bg-white p-6 dark:border-gray-700 dark:bg-gray-800">
        <h3 className="mb-2 font-semibold">Curator Note</h3>
        <p className="whitespace-pre-line text-gray-700 dark:text-gray-300">
          {mod.curator_note}
        </p>
      </div>

      <div className="flex gap-4 text-sm">
        <span className="rounded-lg bg-green-100 px-3 py-1 font-medium text-green-800 dark:bg-green-900 dark:text-green-200">
          Net score: {mod.net_score}
        </span>
      </div>

      <p className="text-sm text-gray-500">
        TODO: Load full details from registry.db once the nightly compiler is available.
      </p>
    </div>
  );
}
