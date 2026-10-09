<div align="center">

# 💠 Aquamarine

**End-to-end encrypted messenger — Telegram's chats and channels meet Discord's servers and calls.**

[**Website**](https://applememestm-pixel.github.io/aquamarine/) ·
[**Download**](https://github.com/applememestm-pixel/aquamarine/releases/latest) ·
[Progress log](PROGRESS.md) ·
[Network plan](docs/NETWORK_PLAN.md)

Rust · Tauri 2 · Svelte 5 · axum

</div>

---

> 🇷🇺 Aquamarine — десктопный мессенджер со сквозным шифрованием: чаты, группы,
> каналы и звонки в стиле Telegram + Discord. Сервер — только «почтальон»: он
> пересылает зашифрованные конверты и никогда не видит текст сообщений.

## Features

| | |
|---|---|
| 🔐 **E2EE by default** | X3DH key agreement + Double Ratchet. The relay sees only routing metadata and ciphertext. |
| 💬 **Chats, groups, supergroups, channels** | Replies, edits, deletes, reactions, polls, articles, pinned & archived chats, member tags, admin roles. |
| 📞 **Calls** | 1:1 and group / channel voice & video (WebRTC mesh), screen share, listener mode for channels. |
| 🖼️ **Media** | Gallery with zoom, built-in photo editor (draw, crop, text), music player with queue, file panel with previews. |
| 🕶️ **Privacy** | Per-user privacy rules, session passcode with auto-lock, block list, ghost mode (keep deleted / edited messages locally). |
| 🌐 **Proxy** | Route all traffic through any SOCKS5 proxy you run (`socks5://host:port`). |
| 🛰️ **Self-hostable relay** | One small binary, SQLite directory, email sign-up, web admin panel with invite-only (solo) mode. |
| 🌍 **RU / EN** | Full Russian and English UI. |

## Download

Grab the latest build from [**Releases**](https://github.com/applememestm-pixel/aquamarine/releases/latest):

| Platform | Client | Relay server |
|---|---|---|
| Windows 10/11 x64 | `Aquamarine-Setup.exe` | `aqua-server-windows-x64.exe` |
| macOS Apple Silicon | `Aquamarine.dmg` | `aqua-server-macos-arm64` |
| macOS Intel | `Aquamarine-Intel.dmg` | — |
| Linux x64 | `Aquamarine.AppImage` / `.deb` / `.rpm` | `aqua-server-linux-x64` |
| Linux ARM64 (Raspberry Pi…) | — | `aqua-server-linux-arm64` |

Releases are built by [`release.yml`](.github/workflows/release.yml) on every `v*` tag.

## Run your own relay

```bash
AQUA_LISTEN=0.0.0.0:7777 \
AQUA_ADMIN_TOKEN=change-me \
./aqua-server-linux-x64
```

Then open `http://<server>:7777/admin` for live stats and access control.

| Variable | Meaning |
|---|---|
| `AQUA_LISTEN` | bind address (default `127.0.0.1:7777`) |
| `AQUA_DIR_DB` | path to the SQLite directory |
| `AQUA_ADMIN_TOKEN` | enables the `/admin` panel (404 without it) |
| `AQUA_OPEN_SIGNUP` | allow sign-up without email confirmation |
| `AQUA_SMTP_HOST` / `_PORT` / `_USER` / `_PASS` / `_FROM` | SMTP for email codes |
| `AQUA_DEV_CODES` | dev only: print email codes to the log |

Put it behind any TLS reverse proxy (Caddy, nginx) for `wss://`.

## Build from source

Requirements: Rust stable, Node.js 22+, Tauri CLI (`cargo install tauri-cli --version '^2'`).
Linux also needs `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf`.

```bash
# everything at once → ../Aquamarine-apps
build-all.bat        # Windows
./build-all.sh       # macOS / Linux

# or by hand
cargo test
cargo build --release -p aqua-server
cd crates/aqua-client-ui/ui && npm ci && npm run build && cd ..
cargo tauri build
```

Dev mode: `cd crates/aqua-client-ui && cargo tauri dev`.

## Layout

```
crates/
  aqua-core/          crypto + protocol (X3DH, Double Ratchet, wire types)
  aqua-server/        axum relay, SQLite directory, admin panel
  aqua-client-core/   transport, session store, sync loop
  aqua-client-ui/     Tauri 2 shell + Svelte 5 UI (ui/)
site/                 landing page (GitHub Pages)
docs/                 design notes, network plan
```

## Roadmap

- [x] E2EE 1:1, groups, channels, calls, media, admin panel, multi-platform builds
- [ ] Android / iOS
- [ ] Server federation: shared mode, replication, coordinator election ([plan](docs/NETWORK_PLAN.md))
- [ ] Own mesh / DHT between relays

## License

MIT OR Apache-2.0.
