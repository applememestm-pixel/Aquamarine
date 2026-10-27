#!/usr/bin/env bash
# Положить на сайт настоящие стикеры и эмодзи из наборов, которые уже
# скачал ретранслятор (папка aqua-packs в папке данных сервера).
#
#   tools/site-packs.sh                       # папка данных сервера по умолчанию
#   tools/site-packs.sh "/путь/к/Aquamarine Server"
#   N=12 tools/site-packs.sh                  # сколько файлов брать из набора
#
# Наборы по умолчанию — ниже, в PACKS (эмодзи — eltaller47 и CeZeOsNaVeKa).
# Сайт берёт site/assets/packs/manifest.json и показывает эти файлы вместо
# заглушек; .tgs пропускаются (сайту нужны webp/png/gif/webm).
# Потом: git add site/assets/packs && git commit.
set -euo pipefail
REPO="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
DATA="${1:-}"
if [ -z "$DATA" ]; then
  f="${XDG_CONFIG_HOME:-$HOME/.config}/aquamarine/server-data.txt"
  [ -f "$f" ] && DATA="$(cat "$f")" || DATA="$HOME/Aquamarine Server"
fi
SRC="$DATA/aqua-packs"
[ -d "$SRC" ] || { echo "Нет папки $SRC — укажите папку данных сервера первым аргументом."; exit 1; }
N="${N:-12}"
command -v node >/dev/null || { echo "Нужен Node.js"; exit 1; }
node - "$SRC" "$REPO/site/assets/packs" "$N" <<'JS'
const fs = require("fs"), path = require("path");
const [src, dest, n] = process.argv.slice(2);
// [набор, вид]: sticker — стикеры, emoji — эмодзи.
const PACKS = [
  ["eltaller47", "emoji"], ["CeZeOsNaVeKa", "emoji"],
  ["CeZeOs_NaVeKa", "sticker"], ["ThatsLife7", "sticker"], ["LuckyDegradationVPST2k25", "sticker"],
  ["Nabor_2306", "sticker"], ["SticpackForAdminsVPST2k25", "sticker"],
];
const OK = new Set([".webp", ".png", ".gif", ".webm"]);
fs.rmSync(dest, { recursive: true, force: true });
fs.mkdirSync(dest, { recursive: true });
const out = { sticker: [], emoji: [], packs: [] };
for (const [name, kind] of PACKS) {
  const dir = path.join(src, name);
  if (!fs.existsSync(dir)) { console.log(`- ${name}: нет на сервере (импортируйте его в приложении по ссылке)`); continue; }
  let m = {}; try { m = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8")); } catch {}
  const files = (m.files || []).map((f) => f.file).filter((f) => OK.has(path.extname(f).toLowerCase())).slice(0, Number(n));
  if (!files.length) { console.log(`- ${name}: нет подходящих файлов (только .tgs?)`); continue; }
  fs.mkdirSync(path.join(dest, name), { recursive: true });
  for (const f of files) {
    const safe = `${out[kind].length}${path.extname(f).toLowerCase()}`;
    fs.copyFileSync(path.join(dir, f), path.join(dest, name, safe));
    out[kind].push(`assets/packs/${name}/${safe}`);
  }
  out.packs.push({ name, title: m.title || name, kind, count: files.length });
  console.log(`+ ${name}: ${files.length}`);
}
fs.writeFileSync(path.join(dest, "manifest.json"), JSON.stringify(out, null, 1));
console.log(`Готово: ${dest}`);
JS
