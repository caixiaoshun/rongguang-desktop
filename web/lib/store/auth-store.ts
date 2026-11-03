import { create } from "zustand";
import { persist } from "zustand/middleware";
import { http } from "@/lib/api/client";
import type { UserRespVO } from "@/lib/api/types";

/**
 * 本机用户信息。
 *
 * 桌面版取消了登录体系：没有令牌、没有登出入口，只保留当前用户资料
 * （昵称、头像等），供顶部栏与个人设置页展示与修改。
 */
interface LocalUserState {
  user: UserRespVO | null;
  loaded: boolean;
  fetchUserInfo: () => Promise<void>;
  setUser: (user: UserRespVO | null) => void;
  reset: () => void;
}

export const useAuthStore = create<LocalUserState>()(
  persist(
    (set) => ({
      user: null,
      loaded: false,

      fetchUserInfo: async () => {
        const user = await http.get<never, UserRespVO>("/api/auth/user-info");
        set({ user, loaded: true });
      },

      setUser: (user) => set({ user, loaded: true }),

      reset: () => set({ user: null, loaded: false }),
    }),
    {
      name: "local-user-storage",
      partialize: (state) => ({ user: state.user }),
    }
  )
);
