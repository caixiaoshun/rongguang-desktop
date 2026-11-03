/**
 * 桌面端能力探测。
 *
 * 桌面版由 Tauri 承载，页面本身就运行在 `http://127.0.0.1:<port>` 上，
 * 所以接口请求无需区分环境；这里只负责识别「是否运行在桌面壳里」，
 * 用于显示打开数据目录、查看版本号等仅桌面可用的入口。
 */

interface TauriGlobals {
  __TAURI_INTERNALS__?: { invoke?: (...args: unknown[]) => unknown };
}

type AppInfo = {
  version: string;
  dataDir: string;
  port: number;
  platform: string;
};

let cached: Promise<AppInfo | null> | null = null;

function tauriInvoke(): ((cmd: string, args?: unknown) => Promise<unknown>) | null {
  if (typeof window === "undefined") return null;
  const internals = (window as unknown as TauriGlobals).__TAURI_INTERNALS__;
  const invoke = internals?.invoke;
  // 只有真正跑在 Tauri WebView 里才具备该函数。
  return typeof invoke === "function"
    ? (invoke as (cmd: string, args?: unknown) => Promise<unknown>).bind(internals)
    : null;
}

/** 是否运行在桌面壳内。 */
export function isDesktop(): boolean {
  return tauriInvoke() !== null;
}

/** 读取桌面端应用信息；网页端返回 null。 */
export function getAppInfo(): Promise<AppInfo | null> {
  if (!cached) {
    const invoke = tauriInvoke();
    cached = invoke
      ? (invoke("app_info") as Promise<AppInfo>).catch(() => null)
      : Promise.resolve(null);
  }
  return cached;
}

/** 在系统文件管理器中打开数据目录；网页端返回 false。 */
export async function openDataDir(): Promise<boolean> {
  const invoke = tauriInvoke();
  if (!invoke) return false;
  try {
    await invoke("open_data_dir");
    return true;
  } catch {
    return false;
  }
}

/** 用系统默认浏览器打开外链；网页端退化为 window.open。 */
export async function openExternalUrl(url: string): Promise<void> {
  const invoke = tauriInvoke();
  if (invoke) {
    try {
      await invoke("open_url", { url });
      return;
    } catch {
      // 落回浏览器行为
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}
