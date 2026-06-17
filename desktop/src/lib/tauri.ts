import { invoke } from '@tauri-apps/api/core';

export interface InstanceRow {
  instance_id: string;
  name: string;
  minecraft_version: string;
  loader: string;
  loader_version: string;
  is_modpack: boolean;
  is_locked: boolean;
  last_launched_at: string | null;
  jvm_memory_mb: number;
  jvm_gc: string;
  jvm_custom_args: string;
  created_at: string;
}

export interface InstalledMod {
  filename: string;
  registry_id: string | null;
  modrinth_id: string | null;
  source: string;
  version: string | null;
  sha256: string;
  installed_at: string;
}

export interface InstanceManifest {
  instance_id: string;
  name: string;
  created_from_pack: string | null;
  minecraft_version: string;
  loader: string;
  loader_version: string;
  is_locked: boolean;
  mods: InstalledMod[];
  user_preferences: Record<string, unknown>;
}

export interface InstanceDetail {
  row: InstanceRow;
  manifest: InstanceManifest | null;
}

export interface LoaderVersionSummary {
  loader: string;
  mc_version: string;
  loader_version: string;
  file_type: string;
}

export interface RegistryItem {
  id: string;
  name: string;
  content_type: string;
  download_strategy: string;
  source_identifier: string;
  sha256: string;
  upvotes: number;
  downvotes: number;
  net_score: number;
  velocity: number;
  status: string;
  is_immune: boolean;
  immunity_reason: string | null;
  allow_comments: boolean;
  icon_url: string | null;
  gallery_urls_json: string | null;
  date_added: string | null;
  compatible_versions_json: string | null;
}

export interface CategoryInfo {
  id: string;
  display_name: string;
  is_community: boolean;
}

export type SortOption = 'net_score' | 'velocity' | 'most_downvoted' | 'newest' | 'most_upvoted';

export interface RegistryStatus {
  has_cached_db: boolean;
  cached_tag: string | null;
  cached_schema_version: number | null;
  latest_tag: string | null;
  update_available: boolean;
  checked: boolean;
  message: string;
}

export interface ExtractionResult {
  extracted: string[];
  skipped: string[];
  total_bytes_written: number;
}

export interface CreateInstanceRequest {
  name: string;
  instance_id: string;
  minecraft_version: string;
  loader: string;
  loader_version: string;
  jvm_memory_mb?: number;
  jvm_gc?: string;
  jvm_custom_args?: string;
}

export const listInstances = () => invoke<InstanceRow[]>('list_instances');
export const getInstanceDetail = (instanceId: string) =>
  invoke<InstanceDetail | null>('get_instance_detail', { instanceId });
export const createInstance = (request: CreateInstanceRequest) =>
  invoke<InstanceRow>('create_instance', { request });
export const deleteInstance = (instanceId: string) =>
  invoke<void>('delete_instance', { instanceId });
export const launchInstance = (instanceId: string) =>
  invoke<void>('launch_instance', { instanceId });
export const listLoaderVersions = (loader: string, mcVersion: string) =>
  invoke<LoaderVersionSummary[]>('list_loader_versions', {
    loader,
    mcVersion,
  });
export const browseItems = (
  contentType?: string,
  category?: string,
  sort?: SortOption,
  modrinthEnabled?: boolean,
  limit?: number,
) =>
  invoke<RegistryItem[]>('browse_items', {
    contentType,
    category,
    sort,
    modrinthEnabled,
    limit,
  });
export const getRegistryItem = (itemId: string) =>
  invoke<RegistryItem | null>('get_registry_item', { itemId });
export const listCategories = () => invoke<CategoryInfo[]>('list_categories');
export const checkRegistryUpdate = (force?: boolean) =>
  invoke<RegistryStatus>('check_registry_update', { force });
export const getRegistryStatus = () => invoke<RegistryStatus>('get_registry_status');
export const extractOverrides = (zipPath: string, instanceId: string) =>
  invoke<ExtractionResult>('extract_overrides', { zipPath, instanceId });
export const getSetting = (key: string) =>
  invoke<unknown | null>('get_setting', { key });
export const setSetting = (key: string, value: unknown) =>
  invoke<void>('set_setting', { key, value });
