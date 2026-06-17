import { fetchRegistry } from '@/lib/registry';

export default async function DirectoryPage() {
  const mods = await fetchRegistry();

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold">Curated Mod Directory</h2>
        <p className="text-gray-600 dark:text-gray-400">
          Community-vetted mods, packs, and assets.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        {mods.map((mod) => (
          <a
            key={mod.id}
            href={`/mod/${mod.id}`}
            className="rounded-xl border bg-white p-4 shadow-sm transition hover:shadow-md dark:border-gray-700 dark:bg-gray-800"
          >
            <div className="flex items-center justify-between">
              <h3 className="font-semibold">{mod.name}</h3>
              <span className="rounded-full bg-brand-100 px-2 py-0.5 text-xs font-medium text-brand-700 dark:bg-brand-900 dark:text-brand-200">
                {mod.content_type}
              </span>
            </div>
            <p className="mt-2 line-clamp-2 text-sm text-gray-600 dark:text-gray-400">
              {mod.curator_note}
            </p>
            <div className="mt-3 flex items-center gap-4 text-sm">
              <span className="text-green-600 dark:text-green-400">Score: {mod.net_score}</span>
              <span className="text-gray-500">{mod.download_strategy}</span>
            </div>
          </a>
        ))}
      </div>
    </div>
  );
}
