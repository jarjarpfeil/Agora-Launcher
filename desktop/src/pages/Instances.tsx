export function Instances() {
  return (
    <div className="space-y-6">
      <section className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold mb-2">My Instances</h2>
          <p className="text-[rgb(var(--muted))]">
            Isolated modpack profiles, custom instances, and launch history.
          </p>
        </div>
        <button className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-medium text-white hover:bg-brand-700">
          + Create Instance
        </button>
      </section>

      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No instances yet.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          TODO: Load instances from local_state.db and render instance_manifest details.
        </p>
      </div>
    </div>
  );
}
