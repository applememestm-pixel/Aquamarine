# Aquamarine — Remaining-work checklist

A living plan derived from the whole build so far. `[x]` done & compiling,
`[~]` partial / v1 only, `[ ]` not started. Work the **Next up** section
top-to-bottom in following sessions.

_Last reviewed: 2026-10-07 (Session 38)._

---

## Done

- [x] Telegram sticker + premium/custom-emoji import (bot API)
- [x] Inline media players (image/video/audio), voice waveform, background top player
- [x] Discord-like shell: no Chromium menus/shortcuts, custom context menu
- [x] Theme editor (full colours, custom fonts, wallpapers), emoji-free settings
- [x] Telegram-style drag-select, message selection, forwarding
- [x] Avatar / profile cards
- [x] Inline custom emoji in message text
- [x] Polls, reactions, pinned messages, read receipts (1:1)
- [x] Quoted replies (synced)
- [x] Voice recording (cancel/stop/pause/delete), composer redesign
- [x] Fast file loading (Tauri asset protocol)
- [x] Listening presence chip + login window (RegistrationWizard)
- [x] 1:1 calls: voice + video + screen-share (WebRTC, perfect-negotiation)
- [x] Discord-grade calls (Session 37): Go Live 720p/1080p/1440p/Source × 15/30/60 fps,
      motion/detail modes, system audio, live quality change, camera + stream together,
      HW-H.264-first codec order, fast bitrate ramp, Opus 96 kbps FEC, devices, gain,
      noise gate, PTT, deafen, per-user volume, speaking rings, stats overlay,
      ICE restart on network blips, ringtone, TURN config
- [x] Public avatar + banner on the server, profile extras, avatars everywhere (Session 38)
- [x] Discord user panel + status picker; activity detection (games / music / video) (Session 38)
- [x] Telegram-style emoji / sticker / GIF panel, animated .tgs/.webm, save-to-collection (Session 38)
- [x] Reactions from the eltaller47 pack (bundle with tools/bundle-reactions.bat) (Session 38)
- [x] Keyboard control + cheat-sheet; Telegram-mobile settings (Session 38)
- [x] Avatar / banner cropper before upload (Session 39)
- [x] Rich composer: custom emoji shown inline as pictures (Session 39)
- [ ] Global push-to-talk (works only while the window is focused; needs tauri global-shortcut)
- [ ] Bundled TURN relay in aqua-server (today: user-configured coturn)
- [x] Registration gate fix (account.json; no stale auto-login) + Log out
- [x] Landing site + per-platform downloads + build-all.bat + server GUI
- [x] Telegram-tight message spacing (selection-mode grid fix)
- [x] Groups / supergroups / channels (client-side fan-out) + settings/admin panel
- [x] **Mandatory registration enforced server-side** (directory-backed; no ephemeral handles)
- [x] **Register-before-connect** client flow
- [x] **Server stats endpoint `/v1/stats` + live counter in the server GUI**
- [x] **Clean-test tooling**: reset-testdata.bat, test-client-A/B.bat
- [x] **Login/register window shown on every launch** + one-click "Continue as @me"
      (no silent auto-enter; registration always testable)
- [x] **Telegram-style message grouping** — tight within a minute-block, clear gap
      between blocks (sender change or new minute)
- [x] **In-chat message search** — header icon opens a search bar; match count,
      ↑/↓ cycle, scroll-to + highlight
- [x] **Compact Telegram-height composer** (slimmer input + 36px buttons)
- [x] **Edit & delete messages** — context-menu Edit/Delete on own messages;
      `[edit:v1]` / `[del:v1]` controls relayed to the peer; matched by exact text
- [x] **Live typing indicators** — throttled `[typing:v1]` on input; "typing…" in
      the 1:1 header, auto-clears after 4s

- [x] **Session 40** — Telegram group/channel settings, member tags, Discord server
      tags, per-user privacy + server photo ACL, archive/mute, quiz/public polls,
      articles in channels, gallery / photo editor / music player, RU/EN, own title bar

- [x] **Session 42** — cross-platform build matrix (Win/macOS/Linux, client + server),
      server admin panel (neon, invite-only/allowlist), "connect through a server" SOCKS5 proxy

## Next up (priority order)

1. [ ] **Build + smoke-test the real binaries** — run `build-all.bat` on Windows
       (produces the NSIS installer wizard), then `reset-testdata.bat`, then
       register two fresh accounts via `test-client-A/B.bat` against the GUI server.
       **Always rebuild after pulling UI changes — launching the old
       `Aquamarine-apps\Aquamarine.exe` shows the previous UI.**
2. [x] **Group media / voice / files** (Session 41) — — groups currently fan out text, replies and
       inline emoji only. Add per-member file fan-out (or a shared blob + per-member
       key) so stickers/images/voice work in groups.
3. [x] **Group edit/delete & typing scoped by group id** (Session 41) — — reactions and poll
       votes now fan out via `[grpctl:v1]` (Session 40); edit/delete/typing are still 1:1-only.
4. [ ] **Message history persistence for normal chats** — verify/implement: today only
       Saved Messages is persisted locally; peer chats rehydrate sessions but not logs.

## Later / needs foundation

- [ ] **Sender Keys** for large channels (replace linear fan-out)
- [ ] **Group calls (SFU)** — only 1:1 calls exist
- [ ] **Real Spotify / YouTube presence** via OAuth (currently manual status)
- [ ] **Android / mobile build** + responsive polish (`cargo tauri android build`)
- [ ] **Multi-device** (same account, 2+ devices) — server already fans out per-device tx
- [ ] **Real SMTP email** in production (wired via `AQUA_SMTP_*`; dev-codes by default)
- [ ] **Notifications** polish (sound/desktop), per-chat mute

## Testing notes

- Server runs with mandatory registration by default. For a raw/open relay set
  `AQUA_OPEN_SIGNUP=1`. Dev email codes show in the server GUI log (`AQUA_DEV_CODES=1`).
- `reset-testdata.bat` wipes the server DB + all local vaults (incl.
  `%APPDATA%\app.aquamarine.desktop`) so testing starts with zero accounts.
