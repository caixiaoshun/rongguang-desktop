import axios from "axios";
import type { CommonResult } from "./types";
import { getApiPayloadMessage, toApiError } from "./api-error";
import { getApiBaseUrl } from "@/lib/runtime-config";

// 默认同源；Docker 可在容器启动时通过 PUBLIC_API_URL 注入分域后端地址。
const API_BASE_URL = getApiBaseUrl();

// 创建 axios 实例
const http = axios.create({
  baseURL: API_BASE_URL,
  timeout: 30000,
});

// 响应拦截器：统一解包 `{ code, msg, data }` 契约。
// 桌面版单机单用户，没有登录/令牌刷新流程，业务错误直接抛出 msg。
http.interceptors.response.use(
  (response) => {
    const result = response.data as CommonResult<unknown>;
    if (result && typeof result === "object" && "code" in result && result.code !== 0) {
      const message = getApiPayloadMessage(result);
      return Promise.reject(new Error(message || "请求失败"));
    }
    return (result ? result.data : response.data) as never;
  },
  (error) => Promise.reject(toApiError(error))
);

export { http, API_BASE_URL };

/**
 * 解析媒体资源 URL
 *
 * - 以 http:// 或 https:// 开头的完整 URL（如 OSS 直链）=> 原样返回
 * - 以 / 开头的相对路径（如 /media/images/xxx.png）=> 拼接后端 API_BASE_URL
 * - 空值 => 返回 null
 */
export function resolveMediaUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  if (url.startsWith("data:")) return url;
  if (url.startsWith("http://") || url.startsWith("https://")) return url;
  if (url.startsWith("/")) return `${API_BASE_URL}${url}`;
  return url;
}
