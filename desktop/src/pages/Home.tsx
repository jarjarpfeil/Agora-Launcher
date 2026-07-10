import { useRegistryState } from '../lib/useRegistryState';
import { RegistryStatusView } from '../components/registry-status-view';

export function Home() {
  const { state, status, error, hasCachedDb, actions } = useRegistryState();

  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-2xl font-bold mb-2">Home</h2>
        <p className="text-muted-foreground">
          Featured & trending curated packs and mods.
        </p>
      </section>

      {/* Registry status card — shared component handles all states */}
      <RegistryStatusView
        variant="card"
        state={state}
        status={status}
        error={error}
        actions={actions}
        title="Registry Status"
      />

      {/* Update available banner (ready state only) */}
      {state === 'ready' && status?.update_available && (
        <div className="rounded-md bg-amber-50 dark:bg-amber-900/20 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
          An update is available. Click &quot;Check for Updates&quot; to download.
        </div>
      )}

      {/* Error detail for missing state (not shown by card component) */}
      {state === 'missing' && error && (
        <div className="rounded-lg border border-destructive bg-destructive/10 p-3 text-xs text-destructive">
          {error}
        </div>
      )}

      {/* Empty state / recovery prompt */}
      <div className="rounded-xl p-6 border border-dashed border-border text-center">
        <p className="text-muted-foreground">No featured items yet.</p>
        <p className="text-sm text-muted-foreground mt-2">
          {hasCachedDb
            ? 'Browse the catalog to discover curated mods and packs.'
            : 'Download the registry to browse curated content.'}
        </p>
      </div>
    </div>
  );
}
