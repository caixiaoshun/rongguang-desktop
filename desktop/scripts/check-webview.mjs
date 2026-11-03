// Used only by the Windows installer smoke test; the product never enables CDP.
import { writeFile } from "node:fs/promises";

const [port, mode, screenshot] = process.argv.slice(2);
const deadline = Date.now() + 20000;
let lastError;
while (Date.now() < deadline) {
  let socket;
  try {
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) })).json();
    const target = targets.find((t) => t.type === "page" && t.webSocketDebuggerUrl &&
      (mode === "error" || new URL(t.url).pathname === "/dashboard/"));
    if (!target) throw new Error("WebView target not ready");
    socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    let nextId = 0;
    const call = (method, params = {}) => new Promise((resolve, reject) => {
      const id = ++nextId;
      const timer = setTimeout(() => { socket.removeEventListener("message", receive); reject(new Error(`CDP timeout: ${method}`)); }, 3000);
      function receive(event) {
        const data = JSON.parse(event.data);
        if (data.id !== id) return;
        clearTimeout(timer);
        socket.removeEventListener("message", receive);
        if (data.error) reject(new Error(JSON.stringify(data.error))); else resolve(data.result);
      }
      socket.addEventListener("message", receive);
      socket.send(JSON.stringify({ id, method, params }));
    });
    const result = await call("Runtime.evaluate", {
      expression: "JSON.stringify({url:location.href,title:document.title,text:document.body?.innerText || ''})",
      returnByValue: true,
    });
    const page = JSON.parse(result.result.value);
    const valid = mode === "error"
      ? page.text.includes("融光未能启动") && page.text.includes("10048")
      : page.text.includes("项目") && page.text.includes("素材");
    if (!valid) throw new Error(`Unexpected page content: ${JSON.stringify(page).slice(0,1500)}`);
    const capture = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(screenshot, Buffer.from(capture.data, "base64"));
    console.log(`PASS: WebView DOM ${mode}: ${page.url.slice(0,200)}, ${page.text.length} visible characters`);
    socket.close();
    process.exit(0);
  } catch (error) {
    lastError = error;
    socket?.close();
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}
console.error(lastError);
process.exit(1);
