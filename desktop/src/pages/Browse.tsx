import { useState } from 'react';

const SORTS = [
  { label: 'Net Score', value: 'net_score' },
  { label: 'Trending', value: 'velocity' },
  { label: 'Newest', value: 'newest' },
  { label: 'Most Upvoted', value: 'upvotes' },
  { label: 'Most Downvoted', value: 'downvotes' },
];

const CATEGORIES = ['All', 'Optimization', 'Rendering', 'Magic', 'Tech', 'QoL'];

export function Browse() {
  const [sort, setSort] = useState('net_score');
  const [category, setCategory] = useState('All');
  const [query, setQuery] = useState('');

  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Browse</h2>
        <p className="text-[rgb(var(--muted))]">
          Curated mods, packs, shaders, resource packs, and more.
        </p>
      </section>

      <div className="flex flex-col lg:flex-row gap-4">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search mods..."
          className="flex-1 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent px-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-brand-500"
        />
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value)}
          className="rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent px-3 py-2 text-sm"
        >
          {SORTS.map((s) => (
            <option key={s.value} value={s.value}>{s.label}</option>
          ))}
        </select>
      </div>

      <div className="flex flex-wrap gap-2">
        {CATEGORIES.map((c) => (
          <button
            key={c}
            onClick={() => setCategory(c)}
            className={[
              'px-3 py-1 rounded-full text-sm border transition-colors',
              category === c
                ? 'bg-brand-600 text-white border-brand-600'
                : 'border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-800',
            ].join(' ')}
          >
            {c}
          </button>
        ))}
      </div>

      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No curated items to display.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          TODO: Fetch real mods from registry_items with dynamic category chips and Modrinth filter.
        </p>
      </div>
    </div>
  );
}
