// Инструменты студии для разработчика (внешний Claude): вызов MCP-сервера запущенной студии
// (`npm run tauri dev`) по ключу из mcp-dev.json. Видны инструменты Claude студии и dev_*.
//
//   node scripts/dev-mcp.mjs list
//   node scripts/dev-mcp.mjs dev_chat_send '{"text":"разрежь клип пополам"}'
//   node scripts/dev-mcp.mjs dev_chat_wait
//   node scripts/dev-mcp.mjs dev_screenshot

import { readFileSync } from "node:fs";
import { join } from "node:path";

const file = join(process.env.APPDATA ?? "", "app.montagestudio.desktop", "mcp-dev.json");
const { url, token } = JSON.parse(readFileSync(file, "utf8"));
const [tool = "list", raw = "{}"] = process.argv.slice(2);

async function rpc(method, params) {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return (await r.json()).result;
}

if (tool === "list") {
  const { tools } = await rpc("tools/list", {});
  for (const t of tools) console.log(`${t.name} — ${t.description}`);
} else {
  const res = await rpc("tools/call", { name: tool, arguments: JSON.parse(raw) });
  for (const c of res.content ?? []) console.log(c.text);
  if (res.isError) process.exitCode = 1;
}
