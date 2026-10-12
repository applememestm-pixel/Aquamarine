#!/usr/bin/env bash
# Bake the "eltaller47" reaction pack into the app build so every copy of
# Aquamarine has it - no Telegram token needed on other PCs.
#
# 1. In Aquamarine: Emoji panel -> "+" -> "Get El Taller reactions" (once).
# 2. Run this script:  tools/bundle-reactions.sh [pack-name]
# 3. Rebuild the app.
set -euo pipefail
REPO="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
PACK="${1:-eltaller47}"
DATA="${AQUA_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/app.aquamarine.desktop}"
command -v node >/dev/null || { echo "Node.js is needed (sudo pacman -S nodejs)"; exit 1; }

node - "$REPO" "$PACK" "$DATA" <<'JS'
const fs = require('fs'), path = require('path');
const [repo, pack, data] = process.argv.slice(2);
const stickers = path.join(data, 'stickers');
if (!fs.existsSync(stickers)) { console.log(`No sticker library at ${stickers} - start Aquamarine once first.`); process.exit(1); }
const src = fs.readdirSync(stickers, { withFileTypes: true }).filter(d => d.isDirectory()).find(d => {
  try { return JSON.parse(fs.readFileSync(path.join(stickers, d.name, 'pack.json'), 'utf8')).name === pack; } catch { return false; }
});
if (!src) {
  console.log(`Pack '${pack}' is not imported yet.`);
  console.log("In Aquamarine open the emoji panel -> '+' -> 'Get El Taller reactions', then run this again.");
  process.exit(1);
}
const dest = path.join(repo, 'crates/aqua-client-ui/ui/public/reactions', pack);
fs.mkdirSync(dest, { recursive: true });
for (const f of fs.readdirSync(dest, { withFileTypes: true })) if (f.isFile()) fs.unlinkSync(path.join(dest, f.name));
const mimes = { '.tgs': 'application/x-tgsticker', '.webm': 'video/webm', '.webp': 'image/webp', '.png': 'image/png', '.gif': 'image/gif' };
const files = [];
for (const f of fs.readdirSync(path.join(stickers, src.name), { withFileTypes: true })) {
  const mime = f.isFile() && mimes[path.extname(f.name).toLowerCase()];
  if (!mime) continue;
  fs.copyFileSync(path.join(stickers, src.name, f.name), path.join(dest, f.name));
  const i = f.name.indexOf('_');
  files.push({ file: f.name, emoji: i >= 0 ? f.name.slice(0, i) : '', mime });
}
fs.writeFileSync(path.join(dest, 'manifest.json'), JSON.stringify({ pack, title: src.name, files }, null, 2));
console.log(`Bundled ${files.length} reactions from '${src.name}' into:\n  ${dest}\nNow rebuild Aquamarine - the pack ships with the app.`);
JS
