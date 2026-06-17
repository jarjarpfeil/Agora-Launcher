export function Home() {
  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Home</h2>
        <p className="text-[rgb(var(--muted))]">
          Featured & trending curated packs and mods.
        </p>
      </section>

      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No featured items yet.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          TODO: Load featured items from registry.db via tauri-plugin-sql.
        </p>
      </div>
    </div>
  );
}
