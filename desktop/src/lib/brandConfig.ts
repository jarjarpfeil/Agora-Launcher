const repositorySlug = import.meta.env.VITE_AGORA_REPOSITORY?.trim();

export const agoraRepositoryUrl = repositorySlug && /^[^/\s]+\/[^/\s]+$/.test(repositorySlug)
  ? `https://github.com/${repositorySlug}`
  : null;
