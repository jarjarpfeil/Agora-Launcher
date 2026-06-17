export function Governance() {
  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Community Governance</h2>
        <p className="text-[rgb(var(--muted))]">
          Active triage polls, recent resolutions, and the transparency log.
        </p>
      </section>

      <div className="rounded-xl p-6 border border-dashed border-gray-300 dark:border-gray-600 text-center">
        <p className="text-[rgb(var(--muted))]">No active polls or resolutions.</p>
        <p className="text-sm text-[rgb(var(--muted))] mt-2">
          TODO: Query under_review items and live GitHub Discussions poll data.
        </p>
      </div>
    </div>
  );
}
