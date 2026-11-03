"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { Loader2 } from "lucide-react";

/**
 * 根页面：桌面版没有登录与初始化流程，直接进入面板。
 * 保留这一层是为了让静态导出的根路径也有一个明确的入口。
 */
export default function Home() {
  const router = useRouter();

  useEffect(() => {
    router.replace("/dashboard/");
  }, [router]);

  return (
    <div className="flex min-h-screen items-center justify-center bg-background">
      <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
    </div>
  );
}
