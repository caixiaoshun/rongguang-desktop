"use client";

import { useState } from "react";
import {
  User,
  Loader2,
  Check,
  Mail,
  Phone,
} from "lucide-react";
import { motion } from "framer-motion";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/lib/store/auth-store";
import { http } from "@/lib/api/client";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { containerVariants, itemVariants, settingsTypography } from "../_shared";

// ============================================================
// 个人信息编辑 Dialog
// ============================================================

interface ProfileDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  user: { nickname: string; email: string; phone: string };
  onSaved: () => void;
}

function ProfileDialog({ open, onOpenChange, user, onSaved }: ProfileDialogProps) {
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState({ nickname: "", email: "", phone: "" });
  const [successMsg, setSuccessMsg] = useState("");
  const [errorMsg, setErrorMsg] = useState("");

  // 打开时同步当前用户信息
  const handleOpenChange = (nextOpen: boolean) => {
    if (nextOpen) {
      setForm({
        nickname: user.nickname || "",
        email: user.email || "",
        phone: user.phone || "",
      });
      setSuccessMsg("");
      setErrorMsg("");
    }
    onOpenChange(nextOpen);
  };

  const handleSave = async () => {
    setSaving(true);
    setSuccessMsg("");
    setErrorMsg("");
    try {
      await http.put<never, boolean>("/api/auth/profile", {
        nickname: form.nickname || undefined,
        email: form.email || undefined,
        phone: form.phone || undefined,
      });
      setSuccessMsg("保存成功");
      onSaved();
      // 延迟关闭，让用户看到成功提示
      setTimeout(() => onOpenChange(false), 600);
    } catch (err: unknown) {
      setErrorMsg(err instanceof Error ? err.message : "保存失败");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-md max-h-[calc(100vh-2rem)] flex flex-col overflow-hidden">
        <DialogHeader className="shrink-0">
          <DialogTitle>编辑个人信息</DialogTitle>
          <DialogDescription>
            修改你的昵称、邮箱和手机号
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 overflow-y-auto min-h-0 px-1 -mx-1">
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">昵称</Label>
            <Input
              placeholder="请输入昵称"
              value={form.nickname}
              onChange={e => setForm(prev => ({ ...prev, nickname: e.target.value }))}
              className="text-sm"
            />
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">邮箱地址</Label>
            <Input
              type="email"
              placeholder="请输入邮箱"
              value={form.email}
              onChange={e => setForm(prev => ({ ...prev, email: e.target.value }))}
              className="text-sm"
            />
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">手机号码</Label>
            <Input
              type="tel"
              placeholder="请输入手机号"
              value={form.phone}
              onChange={e => setForm(prev => ({ ...prev, phone: e.target.value }))}
              className="text-sm"
            />
          </div>
          {/* 反馈消息 */}
          {successMsg && (
            <div className="flex items-center gap-1.5 text-xs text-green-500">
              <Check className="h-3.5 w-3.5" />
              {successMsg}
            </div>
          )}
          {errorMsg && (
            <p className="text-xs text-destructive">{errorMsg}</p>
          )}
        </div>

        <DialogFooter className="shrink-0">
          <DialogClose render={<Button variant="outline" size="sm" />}>
            取消
          </DialogClose>
          <Button size="sm" onClick={handleSave} disabled={saving}>
            {saving && <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />}
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ============================================================
// 个人设置页面
// ============================================================

export default function ProfileSettingsPage() {
  const user = useAuthStore((s) => s.user);
  const fetchUserInfo = useAuthStore((s) => s.fetchUserInfo);

  // 个人信息 Dialog
  const [profileDialogOpen, setProfileDialogOpen] = useState(false);

  // 用户名首字母（用于头像占位）
  const avatarInitial = (user?.nickname || user?.username || "U")
    .charAt(0)
    .toUpperCase();

  return (
    <motion.div
      className="w-full"
      variants={containerVariants}
      initial={false}
      animate="visible"
    >
      {/* 页面标题 */}
      <motion.div variants={itemVariants} className="mb-8">
        <h1 className={settingsTypography.pageTitle}>个人设置</h1>
        <p className={settingsTypography.pageDescription}>
          管理你的个人信息和账户安全
        </p>
      </motion.div>

      {/* ========== 个人信息卡片 ========== */}
      <motion.div variants={itemVariants} className="mb-8">
        <h3 className={cn(settingsTypography.sectionTitle, "mb-3 px-1")}>
          个人信息
        </h3>
        <div
          className={cn(
            "rounded-xl border border-border/30 overflow-hidden",
            "bg-card/50 backdrop-blur-sm"
          )}
        >
          {/* 头部：头像 + 基本信息 */}
          <div className="flex items-center gap-4 px-6 py-5">
            <div className="h-16 w-16 rounded-2xl bg-linear-to-br from-blue-500 via-purple-500 to-pink-500 flex items-center justify-center text-white text-xl font-bold shadow-lg shrink-0">
              {avatarInitial}
            </div>
            <div className="flex-1 min-w-0">
              <h2 className="text-lg font-semibold">
                {user?.nickname || user?.username || "用户"}
              </h2>
              <p className="text-sm text-muted-foreground">
                @{user?.username || "unknown"}
              </p>
            </div>
            <button
              onClick={() => setProfileDialogOpen(true)}
              className={cn(
                "px-4 py-2 rounded-xl text-sm font-medium",
                "border border-border/30 bg-white/5",
                "text-muted-foreground hover:text-foreground hover:bg-white/10",
                "transition-all duration-200"
              )}
            >
              编辑资料
            </button>
          </div>

          {/* 详细信息条目 */}
          <div className="border-t border-border/20">
            {/* 昵称 */}
            <div className="flex items-center gap-3 px-6 py-3.5 border-b border-border/10">
              <div className="h-8 w-8 rounded-lg bg-blue-500/10 flex items-center justify-center shrink-0">
                <User className="h-4 w-4 text-blue-400" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-xs text-muted-foreground">昵称</p>
                <p className="text-sm font-medium">{user?.nickname || "未设置"}</p>
              </div>
            </div>

            {/* 邮箱 */}
            <div className="flex items-center gap-3 px-6 py-3.5 border-b border-border/10">
              <div className="h-8 w-8 rounded-lg bg-green-500/10 flex items-center justify-center shrink-0">
                <Mail className="h-4 w-4 text-green-400" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-xs text-muted-foreground">邮箱地址</p>
                <p className="text-sm font-medium">{user?.email || "未设置"}</p>
              </div>
            </div>

            {/* 手机号 */}
            <div className="flex items-center gap-3 px-6 py-3.5">
              <div className="h-8 w-8 rounded-lg bg-amber-500/10 flex items-center justify-center shrink-0">
                <Phone className="h-4 w-4 text-amber-400" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-xs text-muted-foreground">手机号码</p>
                <p className="text-sm font-medium">{user?.phone || "未设置"}</p>
              </div>
            </div>
          </div>
        </div>
      </motion.div>

      {/* ========== 账户信息卡片 ========== */}
      <motion.div variants={itemVariants}>
        <h3 className={cn(settingsTypography.sectionTitle, "mb-3 px-1")}>
          账户信息
        </h3>
        <div
          className={cn(
            "rounded-xl border border-border/30 overflow-hidden",
            "bg-card/50 backdrop-blur-sm"
          )}
        >
          <div className="divide-y divide-border/10">
            <div className="flex items-center justify-between px-6 py-3.5">
              <span className="text-xs text-muted-foreground">用户名</span>
              <span className="text-sm font-mono">{user?.username || "—"}</span>
            </div>
            <div className="flex items-center justify-between px-6 py-3.5">
              <span className="text-xs text-muted-foreground">用户 ID</span>
              <span className="text-sm font-mono">{user?.id || "—"}</span>
            </div>
            <div className="flex items-center justify-between px-6 py-3.5">
              <span className="text-xs text-muted-foreground">角色</span>
              <div className="flex items-center gap-1.5">
                {user?.roles?.map((role) => (
                  <span
                    key={role}
                    className="px-2 py-0.5 rounded-full bg-primary/10 text-[10px] text-primary font-medium"
                  >
                    {role}
                  </span>
                )) || <span className="text-sm text-muted-foreground">—</span>}
              </div>
            </div>
            <div className="flex items-center justify-between px-6 py-3.5">
              <span className="text-xs text-muted-foreground">注册时间</span>
              <span className="text-sm font-mono">
                {user?.createTime
                  ? new Date(user.createTime).toLocaleDateString("zh-CN", {
                      year: "numeric",
                      month: "2-digit",
                      day: "2-digit",
                    })
                  : "—"}
              </span>
            </div>
          </div>
        </div>
      </motion.div>

      {/* ========== 个人信息编辑 Dialog ========== */}
      <ProfileDialog
        open={profileDialogOpen}
        onOpenChange={setProfileDialogOpen}
        user={{
          nickname: user?.nickname || "",
          email: user?.email || "",
          phone: user?.phone || "",
        }}
        onSaved={() => fetchUserInfo()}
      />
    </motion.div>
  );
}
