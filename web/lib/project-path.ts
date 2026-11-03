"use client";

import { useMemo } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

/**
 * 桌面端使用静态导出（`output: "export"`），动态路由段无法在构建期枚举，
 * 因此项目详情页改为 `/projects/detail/<子页>?id=<项目ID>` 的形态。
 *
 * 为了让业务代码里的 `/projects/${id}/xxx` 写法保持可读，这里统一提供：
 * - `useProjectId()`：读取当前项目 ID
 * - `projectHref()`：拼装项目内链接
 * - `useProjectRouter()`：自动把 `/projects/<id>/...` 重写为静态路由的 router
 */

const PROJECT_PATH_RE = /^\/projects\/(\d+)(\/[^?#]*)?(\?.*)?$/;

/** 把 `/projects/12/scripts?x=1` 重写为 `/projects/detail/scripts?x=1&id=12`。 */
export function rewriteProjectPath(path: string): string {
  const matched = PROJECT_PATH_RE.exec(path);
  if (!matched) return path;
  const [, id, sub = "", query = ""] = matched;
  const search = query ? `${query}&id=${id}` : `?id=${id}`;
  return `/projects/detail${sub}${search}`;
}

/** 拼装项目内子页面地址，`sub` 形如 `/scripts`。 */
export function projectHref(id: number | string, sub = ""): string {
  return `/projects/detail${sub}?id=${id}`;
}

/** 读取当前项目 ID；不在项目详情页时返回 0。 */
export function useProjectId(): number {
  const searchParams = useSearchParams();
  const raw = searchParams.get("id");
  const id = Number(raw);
  return Number.isFinite(id) && id > 0 ? id : 0;
}

export function useProjectRouter() {
  const router = useRouter();
  return useMemo(
    () => ({
      push: (href: string, options?: Parameters<typeof router.push>[1]) =>
        router.push(rewriteProjectPath(href), options),
      replace: (href: string, options?: Parameters<typeof router.replace>[1]) =>
        router.replace(rewriteProjectPath(href), options),
      back: () => router.back(),
      forward: () => router.forward(),
      refresh: () => router.refresh(),
      prefetch: (href: string) => router.prefetch(rewriteProjectPath(href)),
    }),
    [router]
  );
}

/**
 * 兼容旧路径（`/projects/<id>`）与新路径（`/projects/detail?id=<id>`）的项目 ID 解析。
 * 供侧边栏、面包屑等"全局壳层"使用。
 */
export function useProjectScopeId(): number | null {
  const pathname = usePathname();
  const searchParams = useSearchParams();
  if (pathname.startsWith("/projects/detail")) {
    const id = Number(searchParams.get("id"));
    return Number.isFinite(id) && id > 0 ? id : null;
  }
  const matched = /^\/projects\/(\d+)/.exec(pathname);
  return matched ? Number(matched[1]) : null;
}
