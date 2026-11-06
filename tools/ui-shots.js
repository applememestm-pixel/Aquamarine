// Screenshots of the built UI with a mocked Tauri — to see how it looks from
// a session that can't build the UI itself. CI prints them into the job log
// as base64 JPEG between "=====SHOT <name>" and "=====END".
//   node tools/ui-shots.js <ui/dist> <outDir> [card colour]
const { chromium } = require('playwright');
const http = require('http'), fs = require('fs'), path = require('path');
const [dist, out, card = '#F2EEE6'] = process.argv.slice(2);
fs.mkdirSync(out, { recursive: true });
const MIME = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json', '.webp': 'image/webp' };
const srv = http.createServer((q, r) => {
  let p = path.join(dist, decodeURIComponent(q.url.split('?')[0]));
  if (!fs.existsSync(p) || fs.statSync(p).isDirectory()) p = path.join(dist, 'index.html');
  r.writeHead(200, { 'content-type': MIME[path.extname(p)] || 'application/octet-stream' });
  fs.createReadStream(p).pipe(r);
}).listen(5199);

const mock = (card) => `
localStorage.setItem("aqua.lang", "ru");
localStorage.setItem("aqua-profile", JSON.stringify({ display_name: "apple", about: "Привет!", phone: "+7 999 123-45-67", birthday: "25 сентября", favourite_music: "",
  banner_a: "#ffffff", banner_b: "#ffffff", card: "${card}", card_i: 62, card_g: 60, pattern: "🌸", estatus: "⭐", status: "" }));
let n = 0;
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
  transformCallback: (cb) => { const id = ++n; window["_" + id] = cb; return id; },
  convertFileSrc: (p) => p,
  invoke: async (cmd) => {
    if (cmd === "init_app") return { has_identity: true, registered: true, username: "apple", session_fingerprint: "ab12 cd34", opk_available: 10, spk_rotates_in_days: 5, connected: true, server_url: "http://127.0.0.1:7777", uid: "A1B2C3D4" };
    if (cmd === "list_chats") return [{ id: "__saved__", name: "Saved Messages", letter: "☁", gradient: "linear-gradient(135deg,#7B5CFF,#00E5C7)", preview: "Note to self — only you see this", preview_is_typing: false, preview_by_me: true, has_lock: true, time: "", unread: 0, online: true, verified: true, channel: false, live: true }];
    if (cmd === "list_local_media") return { packs: [], gifs: [], stickers_dir: "", gifs_dir: "" };
    if (cmd.startsWith("plugin:event|listen")) return ++n;
    if (/^(get_messages|list_)/.test(cmd)) return [];
    return null;
  },
};`;

(async () => {
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1500, height: 880 }, locale: 'ru-RU' });
  const errs = [];
  p.on('pageerror', (e) => errs.push(e.message));
  const shot = (name) => p.screenshot({ path: path.join(out, name + '.jpg'), type: 'jpeg', quality: 72 });
  await p.addInitScript(mock(card));
  await p.goto('http://127.0.0.1:5199/');
  await p.waitForTimeout(2500);
  // First-start offers (passcode…): "Not now".
  for (let i = 0; i < 3; i++) {
    const later = p.getByText(/^(Не сейчас|Not now)$/).first();
    if (!(await later.count())) break;
    await later.click().catch(() => {}); await p.waitForTimeout(600);
  }
  await shot('1-main');
  // Bottom strip: the account card and the message bar should line up.
  console.log('boxes', JSON.stringify(await p.evaluate(() => Object.fromEntries(['.user-panel', '.composer', '.composer .input-wrap'].map((q) => {
    const r = document.querySelector(q)?.getBoundingClientRect(); return [q, r && [r.top, r.bottom, r.height]]; })))));
  const steps = [
    ['2-panel', async () => { await p.locator('.user-panel .who, .user-panel button').first().click(); }],
    ['3-view', async () => { await p.getByText(/^(Мой профиль|Edit profile)$/).first().click(); }],
    ['4-edit', async () => { await p.getByText(/^(Редактировать профиль|Edit my profile)$/).first().click(); }],
    ['5-edit-down', async () => { await p.locator('.ed-left').evaluate((e) => e.scrollTo(0, 9999)); }],
    ['6-pick', async () => { await p.locator('.ed-left').evaluate((e) => e.scrollTo(0, 0)); await p.locator('.es-cur').click(); }],
    ['7-date', async () => { await p.keyboard.press('Escape'); await p.locator('.bd-btn').click(); }],
  ];
  for (const [name, fn] of steps) {
    try { await fn(); await p.waitForTimeout(700); await shot(name); }
    catch (e) { errs.push(name + ': ' + e.message.split('\n')[0]); break; }
  }
  // The same main screen in the light theme: nothing may stay dark-on-dark.
  try {
    await p.keyboard.press('Escape'); await p.keyboard.press('Escape');
    await p.evaluate(() => { const s = JSON.parse(localStorage.getItem('aqua-settings') || '{}'); s.mode = 'light'; localStorage.setItem('aqua-settings', JSON.stringify(s)); });
    await p.reload(); await p.waitForTimeout(2500);
    for (let i = 0; i < 3; i++) { const l = p.getByText(/^(Не сейчас|Not now)$/).first(); if (!(await l.count())) break; await l.click().catch(() => {}); await p.waitForTimeout(500); }
    await shot('8-light');
  } catch (e) { errs.push('8-light: ' + e.message.split('\n')[0]); }
  console.log(errs.join('\n') || 'no errors');
  for (const f of fs.readdirSync(out).filter((f) => f.endsWith('.jpg')).sort()) {
    console.log('=====SHOT ' + f);
    console.log(fs.readFileSync(path.join(out, f)).toString('base64'));
    console.log('=====END');
  }
  await b.close(); srv.close();
})();
