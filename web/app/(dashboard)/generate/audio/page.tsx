"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { aiModelApi, type AiModel } from "@/lib/api/ai-model";
import { http, resolveMediaUrl } from "@/lib/api/client";
import { toastApiError } from "@/lib/api/toast-api-error";

export default function AudioPage() {
  const [models, setModels] = useState<AiModel[]>([]);
  const [speechId, setSpeechId] = useState("");
  const [transcribeId, setTranscribeId] = useState("");
  const [input, setInput] = useState("");
  const [voice, setVoice] = useState("ara");
  const [audio, setAudio] = useState<string | null>(null);
  const [file, setFile] = useState<File | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => {
    aiModelApi.list().then(items => {
      setModels(items);
      setSpeechId(String(items.find(m => m.modelType === 4)?.id ?? ""));
      setTranscribeId(String(items.find(m => m.modelType === 5)?.id ?? ""));
    }).catch(error => toastApiError(error, "加载语音模型失败"));
  }, []);
  async function synthesize() {
    setBusy("speech");
    try {
      const url = await http.post<never, string>("/api/generation/audio/speech", { modelId: Number(speechId), input, voice }, { timeout: 600000 });
      setAudio(resolveMediaUrl(url));
    } catch (error) { toastApiError(error, "配音失败"); }
    finally { setBusy(null); }
  }
  async function transcribe() {
    if (!file) return;
    if (file.size > 25 * 1024 * 1024) { toastApiError(new Error("请选择 25MB 以内的音频文件")); return; }
    setBusy("transcribe");
    try {
      const form = new FormData(); form.append("modelId", transcribeId); form.append("file", file);
      setText(await http.post<never, string>("/api/generation/audio/transcribe", form, { timeout: 600000 }));
    } catch (error) { toastApiError(error, "转写失败"); }
    finally { setBusy(null); }
  }
  const field = "w-full rounded-lg border bg-background p-3";
  const button = "rounded-lg bg-primary px-5 py-2 text-primary-foreground disabled:opacity-50";
  return <main className="mx-auto w-full max-w-4xl space-y-8 overflow-auto p-6">
    <header><h1 className="text-2xl font-semibold">配音与转写</h1><p className="mt-2 text-muted-foreground">为短剧台词生成配音，或将已有音频转成文字。</p></header>
    <section className="space-y-4 rounded-xl border p-5">
      <h2 className="text-lg font-medium">台词配音</h2>
      <label className="block">语音模型<select className={field} value={speechId} onChange={e => setSpeechId(e.target.value)}>{models.filter(m => m.modelType === 4).map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
      {!speechId && <Link href="/settings/ai-models" className="underline">前往设置启用语音合成模型</Link>}
      <label className="block">音色<select className={field} value={voice} onChange={e => setVoice(e.target.value)}>{["ara", "eve", "sal", "leo", "rex"].map(v => <option key={v}>{v}</option>)}</select></label>
      <label className="block">配音文本<textarea className={field} rows={6} maxLength={10000} value={input} onChange={e => setInput(e.target.value)} placeholder="输入角色台词或旁白" /></label>
      <button className={button} disabled={!!busy || !speechId || !input.trim()} onClick={synthesize}>{busy === "speech" ? "正在生成配音…" : "生成配音"}</button>
      {audio && <div className="space-y-2"><audio controls src={audio} className="w-full" /><a href={audio} download="配音.mp3" className="underline">下载配音</a></div>}
    </section>
    <section className="space-y-4 rounded-xl border p-5">
      <h2 className="text-lg font-medium">音频转写</h2>
      <label className="block">识别模型<select className={field} value={transcribeId} onChange={e => setTranscribeId(e.target.value)}>{models.filter(m => m.modelType === 5).map(m => <option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
      {!transcribeId && <Link href="/settings/ai-models" className="underline">前往设置启用语音识别模型</Link>}
      <label className="block">音频文件（最大 25MB）<input className={field} type="file" accept="audio/*" onChange={e => setFile(e.target.files?.[0] ?? null)} /></label>
      <button className={button} disabled={!!busy || !transcribeId || !file} onClick={transcribe}>{busy === "transcribe" ? "正在识别…" : "开始转写"}</button>
      <label className="block">转写结果<textarea className={field} rows={6} value={text} onChange={e => setText(e.target.value)} /></label>
    </section>
  </main>;
}
