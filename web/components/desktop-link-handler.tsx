"use client";

import { useEffect } from "react";
import { toast } from "sonner";
import { isDesktop, openExternalUrl } from "@/lib/desktop";

/** 桌面中的外链交给系统浏览器；站内导航和下载保持原有行为。 */
export function DesktopLinkHandler() {
  useEffect(() => {
    if (!isDesktop()) return;

    const handleLink = (event: MouseEvent) => {
      if (event.defaultPrevented || !(event.target instanceof Element)) return;
      if (event.type === "click" && event.button !== 0) return;
      if (event.type === "auxclick" && event.button !== 1) return;
      const anchor = event.target.closest<HTMLAnchorElement>("a[href]");
      if (!anchor || anchor.hasAttribute("download")) return;
      const url = new URL(anchor.href, window.location.href);
      if (url.protocol !== "https:" && url.protocol !== "http:") return;
      if (url.origin === window.location.origin && anchor.target !== "_blank") return;

      event.preventDefault();
      event.stopPropagation();
      void openExternalUrl(url.href).catch((error: unknown) => {
        toast.error("无法打开链接", {
          description: error instanceof Error ? error.message : String(error),
        });
      });
    };
    document.addEventListener("click", handleLink, true);
    document.addEventListener("auxclick", handleLink, true);
    return () => {
      document.removeEventListener("click", handleLink, true);
      document.removeEventListener("auxclick", handleLink, true);
    };
  }, []);
  return null;
}
