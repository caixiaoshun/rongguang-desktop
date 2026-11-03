"use client";

/**
 * 首次启动引导。
 *
 * 安装包既不内置 API Key，也不预置服务地址，所以新装用户第一次进来时两项都是空的。
 * 这里给一条明确提示，避免用户面对空白界面不知道下一步做什么。
 *
 * 判定：只要有一个启用中的供应商同时具备「服务地址 + Key」就不再提示。
 * 地址为空也要提示——只有 Key 没有地址同样生成不了，报错还更靠后才出现。
 */

import { useEffect, useState } from "react";
import Link from "next/link";
import { KeyRound, X } from "lucide-react";
import { apiConfigApi } from "@/lib/api/ai-model";

const DISMISS_KEY = "fusion-api-key-notice-dismissed";

type Gap = "none" | "key" | "url" | "both";

export function ApiKeyNotice() {
  const [gap, setGap] = useState<Gap>("none");
  const [dismissed, setDismissed] = useState(true);

  useEffect(() => {
    // 记住用户主动关掉过，避免每次刷新都弹
    if (typeof window !== "undefined" && window.sessionStorage.getItem(DISMISS_KEY) === "1") {
      setDismissed(true);
    } else {
      setDismissed(false);
    }

    let cancelled = false;
    (async () => {
      try {
        const list = await apiConfigApi.list();
        const usable = list.filter((c) => c.status !== 0);
        const hasKey = (c: { apiKey?: string | null }) => (c.apiKey ?? "").trim().length > 0;
        // comfyui / sonilo 之类不走文本协议的配置，地址规则不同，但它们也都有地址字段
        const hasUrl = (c: { apiUrl?: string | null }) => (c.apiUrl ?? "").trim().length > 0;

        if (usable.some((c) => hasKey(c) && hasUrl(c))) {
          if (!cancelled) setGap("none");
          return;
        }
        const anyKey = usable.some(hasKey);
        const anyUrl = usable.some(hasUrl);
        if (!cancelled) {
          setGap(anyKey ? "url" : anyUrl ? "key" : "both");
        }
      } catch {
        // 读取失败就不打扰用户，当作已配置
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  if (gap === "none" || dismissed) {
    return null;
  }

  const handleDismiss = () => {
    setDismissed(true);
    if (typeof window !== "undefined") {
      window.sessionStorage.setItem(DISMISS_KEY, "1");
    }
  };

  const title =
    gap === "url"
      ? "还缺服务地址"
      : gap === "key"
        ? "还缺 API Key"
        : "还没有配置 API Key 与服务地址";

  const detail =
    gap === "url"
      ? "安装包不预置任何服务地址，需要你填上自己的 Grok 服务地址才能开始生成。"
      : gap === "key"
        ? "安装包不内置任何密钥，需要你自己填一个 Grok Key 才能开始生成。"
        : "安装包既不内置密钥、也不预置服务地址，两项都填上才能开始生成。";

  return (
    <div className="mb-6 flex items-start gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3">
      <KeyRound className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-amber-700 dark:text-amber-500">{title}</p>
        <p className="mt-1 text-xs leading-relaxed text-amber-700/80 dark:text-amber-500/80">
          {detail}
          <Link
            href="/settings/ai-models"
            className="ml-1 font-medium underline underline-offset-2 hover:opacity-80"
          >
            去配置
          </Link>
        </p>
      </div>
      <button
        type="button"
        onClick={handleDismiss}
        aria-label="关闭提示"
        className="rounded-md p-1 text-amber-600/70 transition-colors hover:bg-amber-500/10 hover:text-amber-700"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
