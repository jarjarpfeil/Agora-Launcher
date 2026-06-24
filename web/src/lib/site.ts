export const GITHUB_REPO_URL =
  process.env.NEXT_PUBLIC_GITHUB_REPO_URL || 'https://github.com/jarjarpfeil/Agora-Minecraft-Mod-Loader';

export const GITHUB_RELEASES_URL = `${GITHUB_REPO_URL}/releases`;

export const GITHUB_API_LATEST_RELEASE =
  `https://api.github.com/repos/${GITHUB_REPO_URL.replace('https://github.com/', '')}/releases/latest`;
