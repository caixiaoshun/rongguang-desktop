import { http } from "./client";

export interface SystemRuntimeVersionInfo {
  currentVersion: string;
  currentVersionDisplay: string;
  comparisonEnabled: boolean;
  developmentBuild: boolean;
  buildProfile: string;
  message: string | null;
}

export interface SystemVersionInfo {
  currentVersion: string;
  currentVersionDisplay: string;
  latestVersion: string | null;
  latestVersionDisplay: string;
  latestReleaseVersion: string | null;
  latestReleaseVersionDisplay: string;
  latestReleaseUrl: string;
  latestReleasePublishedAt: string | null;
  latestReleaseDockerReady: boolean;
  updateAvailable: boolean;
  comparisonEnabled: boolean;
  developmentBuild: boolean;
  buildProfile: string;
  versionRelation: "behind" | "same" | "ahead" | "incomparable";
  releaseUrl: string;
  tagUrl: string;
  publishedAt: string | null;
  checkedAt: string;
  source: string;
  checkSucceeded: boolean;
  message: string | null;
  /** 当前平台对应的安装包（用于一键下载） */
  assetName: string | null;
  assetUrl: string | null;
  assetSize: number | null;
}

export function getSystemRuntimeVersion(): Promise<SystemRuntimeVersionInfo> {
  return http.get("/api/system/version/runtime");
}

export function getSystemVersion(force = false): Promise<SystemVersionInfo> {
  return http.get("/api/system/version", {
    params: force ? { force: true } : undefined,
  });
}
// ========== 检查更新（下载 / 安装） ==========

export interface UpdateDownloadProgress {
  status: "idle" | "downloading" | "done" | "failed";
  received: number;
  total: number;
  path: string | null;
  error: string | null;
}

export interface UpdateInstallResult {
  launched: boolean;
  message: string;
}

/** 触发安装包下载。已下载过会直接复用本地文件。 */
export function downloadUpdate(payload: {
  url: string;
  name?: string;
}): Promise<UpdateDownloadProgress> {
  return http.post<never, UpdateDownloadProgress>("/api/system/update/download", payload);
}

export function getUpdateProgress(): Promise<UpdateDownloadProgress> {
  return http.get<never, UpdateDownloadProgress>("/api/system/update/progress");
}

/** 拉起系统安装器。Windows 直接启动 setup.exe，macOS 打开 dmg，Linux 打开下载目录。 */
export function installUpdate(payload: {
  path?: string;
}): Promise<UpdateInstallResult> {
  return http.post<never, UpdateInstallResult>("/api/system/update/install", payload);
}
