export interface RegistryMod {
  id: string;
  name: string;
  content_type: string;
  download_strategy: 'github_release' | 'modrinth_id' | 'direct_hash' | 'curated_pack';
  source_identifier: string;
  curator_note: string;
  net_score: number;
}

/**
 * Fetch the curated registry.
 *
 * TODO: At build time, download `registry.db` from the latest GitHub Release
 * asset URL (e.g. `https://github.com/<org>/<repo>/releases/download/registry-YYYY-MM-DD/registry.db`)
 * and query it for public mod metadata.
 */
export async function fetchRegistry(): Promise<RegistryMod[]> {
  // Placeholder demo data until the compiler is wired up.
  return [
    {
      id: 'sodium',
      name: 'Sodium',
      content_type: 'mod',
      download_strategy: 'github_release',
      source_identifier: 'CaffeineMC/sodium',
      curator_note:
        'Essential rendering engine replacing legacy OpenGL pipelines. Significantly boosts framerates on nearly all hardware.',
      net_score: 124,
    },
    {
      id: 'iris',
      name: 'Iris',
      content_type: 'mod',
      download_strategy: 'github_release',
      source_identifier: 'IrisShaders/Iris',
      curator_note:
        'Modern shader mod designed for compatibility and performance. Use with Sodium for the best experience.',
      net_score: 98,
    },
  ];
}
