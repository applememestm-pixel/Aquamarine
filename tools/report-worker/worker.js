// Aquamarine error reports → the developer's Telegram, within a second.
//
// A Cloudflare Worker: free, always on, independent of any Aquamarine
// relay (reports still arrive when the developer's own server is down).
// Apps POST /report with JSON (see crates/aqua-server/src/report.rs):
//   { app, ver, os, kind, code, msg, detail, server, id }
// No message text is ever in a report — only the failure, version and device.
//
// Secrets (wrangler secret put …): TG_TOKEN — a bot from @BotFather,
// TG_CHAT — your chat id with that bot (send it /start, then open
// https://api.telegram.org/bot<TOKEN>/getUpdates and copy "chat":{"id":…}).

const MAX = 16 * 1024;
const seen = new Map();   // same failure → one message per 10 minutes
const perIp = new Map();  // ≤ 30 reports a minute from one address

const clip = (s, n) => String(s ?? "").slice(0, n);
const esc = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    if (req.method === "GET" && url.pathname === "/") return new Response("aquamarine reports: ok");
    // Telegram Bot API forwarding for relays in countries where Telegram is
    // blocked: Aquamarine Server → Stickers → Telegram network → Bot API
    // address = https://<worker>/tg/<TG_RELAY_KEY>. Off unless the secret
    // TG_RELAY_KEY is set (so nobody else proxies their bots through you).
    if (url.pathname.startsWith("/tg/")) {
      const [, , key, ...rest] = url.pathname.split("/");
      if (!env.TG_RELAY_KEY || key !== env.TG_RELAY_KEY || !rest.length) return new Response("not found", { status: 404 });
      const target = "https://api.telegram.org/" + rest.join("/") + url.search;
      const init = { method: req.method, headers: { "content-type": req.headers.get("content-type") || "application/json" } };
      if (req.method !== "GET" && req.method !== "HEAD") init.body = await req.arrayBuffer();
      return fetch(target, init);
    }
    if (req.method !== "POST" || url.pathname !== "/report") return new Response("not found", { status: 404 });
    const body = await req.text();
    if (body.length > MAX) return new Response("too big", { status: 413 });
    let r;
    try { r = JSON.parse(body); } catch { return new Response("bad json", { status: 400 }); }

    const now = Date.now();
    const ip = req.headers.get("cf-connecting-ip") || "?";
    const w = (perIp.get(ip) || []).filter((t) => now - t < 60_000);
    if (w.length >= 30) return new Response("slow down", { status: 429 });
    w.push(now); perIp.set(ip, w);

    const key = [r.app, r.ver, r.code, clip(r.msg, 200)].join("|");
    const prev = seen.get(key);
    if (prev && now - prev.at < 600_000) { prev.n++; return new Response("dup"); }
    seen.set(key, { at: now, n: 1 });
    if (seen.size > 5000) seen.clear();

    const lines = [
      `${r.kind === "server" ? "🖥" : r.kind === "panic" ? "💥" : "⚠️"} <b>${esc(clip(r.app, 40))} ${esc(clip(r.ver, 24))}</b> · ${esc(clip(r.os, 40))}`,
      r.code ? `<code>${esc(clip(r.code, 40))}</code>` : "",
      esc(clip(r.msg, 600)),
      r.detail ? `<pre>${esc(clip(r.detail, 2500))}</pre>` : "",
      [r.server && `server ${esc(clip(r.server, 80))}`, r.id && `id ${esc(clip(r.id, 16))}`].filter(Boolean).join(" · "),
    ].filter(Boolean);
    const tg = await fetch(`https://api.telegram.org/bot${env.TG_TOKEN}/sendMessage`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ chat_id: env.TG_CHAT, text: lines.join("\n").slice(0, 4000), parse_mode: "HTML", disable_web_page_preview: true }),
    });
    return new Response(tg.ok ? "ok" : "telegram " + tg.status, { status: tg.ok ? 200 : 502 });
  },
};
