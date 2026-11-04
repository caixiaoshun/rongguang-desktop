import type { NextConfig } from "next";

/**
 * 桌面端构建（`FUSION_DESKTOP_BUILD=1`）使用静态导出：产物是纯 `out/` 目录，
 * 由 Tauri 壳内的 Rust 服务以同源方式托管，不需要 Node 运行时。
 * 网页版构建保持原有的 standalone 输出与开发期代理。
 */
const isDesktopBuild = process.env.FUSION_DESKTOP_BUILD === "1";
const developmentBackendUrl = process.env.DEV_BACKEND_URL?.trim().replace(/\/+$/, "");

if (!isDesktopBuild && process.env.NODE_ENV === "development" && !developmentBackendUrl) {
  throw new Error(
    "本地开发必须配置 DEV_BACKEND_URL，请检查 web/.env.development",
  );
}

const baseConfig: NextConfig = {
  reactStrictMode: true,
  output: isDesktopBuild ? "export" : "standalone",
  // 静态导出以目录形式生成 index.html，便于内嵌服务按路径直接命中
  trailingSlash: isDesktopBuild,
  images: {
    // 静态导出没有图片优化服务，直接使用原始地址
    unoptimized: isDesktopBuild,
  },
  // Development API rewrites proxy SSE through Next.js. Disable compression
  // there so incremental events are flushed instead of buffered by gzip.
  compress: process.env.NODE_ENV !== "development",
};

// 静态导出不支持自定义路由与响应头，桌面端完全交由内嵌 Rust 服务处理。
const webOnlyConfig: NextConfig = {
  async rewrites() {
    if (process.env.NODE_ENV !== "development") {
      return [];
    }
    return [
      {
        source: "/api/:path*",
        destination: `${developmentBackendUrl!}/api/:path*`,
      },
      {
        source: "/media/:path*",
        destination: `${developmentBackendUrl!}/media/:path*`,
      },
      {
        source: "/uploads/:path*",
        destination: `${developmentBackendUrl!}/uploads/:path*`,
      },
    ];
  },
  async headers() {
    return [
      {
        source: "/runtime-config.js",
        headers: [
          {
            key: "Cache-Control",
            value: "no-store, max-age=0",
          },
        ],
      },
    ];
  },
};

const nextConfig: NextConfig = isDesktopBuild
  ? baseConfig
  : { ...baseConfig, ...webOnlyConfig };

export default nextConfig;
