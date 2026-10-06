import { getVersion } from "@tauri-apps/api/app";

const NATIVE_OPENAI_FORK_VERSION_MARKER = "-native-openai.";

export function isNativeOpenAiForkVersion(version: string): boolean {
  return version.includes(NATIVE_OPENAI_FORK_VERSION_MARKER);
}

export class ForkUpdateBlockedError extends Error {
  constructor() {
    super("Official updates cannot replace the native OpenAI fork build.");
    this.name = "ForkUpdateBlockedError";
  }
}

/** Check the running bundle's version before any official installer is started. */
export async function assertOfficialUpdateAllowed(): Promise<void> {
  // Do not use a display/cache fallback: failure to read the running version must
  // stop installation, rather than accidentally overwrite a fork with upstream.
  if (isNativeOpenAiForkVersion(await getVersion())) {
    throw new ForkUpdateBlockedError();
  }
}

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

export async function checkForUpdate(
  opts: CheckOptions = {},
): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  // 动态引入，避免在未安装插件时导致打包期问题
  const { check } = await import("@tauri-apps/plugin-updater");

  const currentVersion = await getCurrentVersion();
  const update = await check({ timeout: opts.timeout ?? 30000 } as any);

  if (!update) {
    return { status: "up-to-date" };
  }

  const info: UpdateInfo = {
    currentVersion,
    availableVersion: (update as any).version ?? "",
    notes: (update as any).notes,
    pubDate: (update as any).date,
  };

  return { status: "available", info };
}
