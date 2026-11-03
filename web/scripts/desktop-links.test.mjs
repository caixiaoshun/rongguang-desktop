import { readFile } from "node:fs/promises";
import assert from "node:assert/strict";
import test from "node:test";
import ts from "typescript";

const source = await readFile(new URL("../lib/desktop.ts", import.meta.url), "utf8");
let sequence = 0;
async function loadDesktop(windowStub) {
  globalThis.window = windowStub;
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  });
  return import("data:text/javascript;base64," + Buffer.from(outputText + `\n// ${++sequence}`).toString("base64"));
}

test("desktop links invoke the native browser opener once", async () => {
  const calls = [];
  const desktop = await loadDesktop({
    location: { href: "http://127.0.0.1:4321/dashboard/" },
    __TAURI_INTERNALS__: { invoke: async (...args) => calls.push(args) },
    open: () => assert.fail("Desktop must not open a WebView popup"),
  });
  await desktop.openExternalUrl("https://github.com/caixiaoshun/rongguang-desktop");
  assert.deepEqual(calls, [["open_url", { url: "https://github.com/caixiaoshun/rongguang-desktop" }]]);
});

test("a native opener failure is reported without retrying as a popup", async () => {
  const error = new Error("Native opener unavailable");
  const desktop = await loadDesktop({
    location: { href: "http://127.0.0.1:4321/" },
    __TAURI_INTERNALS__: { invoke: async () => { throw error; } },
    open: () => assert.fail("A failed native call must not fall back to window.open"),
  });
  await assert.rejects(desktop.openExternalUrl("https://github.com/"), error);
});

test("a normal browser retains normal new-tab behavior", async () => {
  const calls = [];
  const desktop = await loadDesktop({
    location: { href: "https://example.org/dashboard/" },
    open: (...args) => calls.push(args),
  });
  await desktop.openExternalUrl("/help");
  assert.deepEqual(calls, [["https://example.org/help", "_blank", "noopener,noreferrer"]]);
});

test("non-web schemes cannot reach either opener", async () => {
  const desktop = await loadDesktop({
    location: { href: "http://127.0.0.1:4321/" },
    __TAURI_INTERNALS__: { invoke: () => assert.fail("Invalid URL reached native opener") },
    open: () => assert.fail("Invalid URL reached browser opener"),
  });
  for (const url of ["javascript:alert(1)", "file:///C:/Windows", "data:text/html,hi"]) {
    await assert.rejects(desktop.openExternalUrl(url), /http\/https/);
  }
});
