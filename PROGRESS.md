# Aquamarine — Progress Log

Chronological record of what has been built, decisions made, and where to
resume. Newest entries at the top.

---

## 2026-10-12 — Session 45: article reader perf, videos in carousels, music attached to articles

- **Reader lag**: article photos are data: URLs; every `<img src>` / CSS
  `url()` re-parsed and re-decoded the whole string on each render. Now
  `lib/media.ts::objectUrl()` turns them into blob: URLs once (memoised).
  Carousels render only the current slide ±1, thumbnails lazy/async;
  `content-visibility:auto` on article blocks; scroll progress via rAF;
  the chat behind the reader gets `visibility:hidden` (`body.reader-open`);
  no entry animation on WebKitGTK.
- **Videos in carousels**: `[[gallery:0,v0,1]]` — `vN` = article `media[N]`.
  Videos/music are uploaded like file offers (encrypted blob on the relay,
  `commands::upload_attachment`) and the article carries `media[]`
  `{kind, cid, key_b64, nonce_b64, name, size, mime, poster, duration}`;
  receivers download+decrypt once into `<data>/media-cache`
  (`fetch_attachment`). Rust keeps only sane refs (hex CID / `local-…`,
  data:image posters). Saved Messages stay local (`local-` CIDs). Carousel
  slides: poster + ▶ → loads and plays in place, pauses on swipe, ⤢ full screen.
- **Music**: `ArticleMusic.svelte` playlist in the chat card and at the top
  of the reader; plays through the app-wide player (keeps playing after the
  reader closes); click a playing track to pause.
- Composer: «Фото / видео» (one carousel in pick order), «Музыка»; posters
  and durations probed locally (`imageTools::probeMedia`,
  `allow_media_preview` adds the picked file to the asset scope); uploads
  happen on publish with progress; drafts keep paths; orphans appended.
  The data dir is added to the asset scope at start (test clients on other drives).
- Verified in the mocked-bridge harness: mixed carousel, poster frame (WebM;
  headless Chromium lacks H.264), music queue → top player, reader hides the
  chat, composer publish payload.

## 2026-10-12 — Session 44: Telegraph-style articles, carousels, Telegram formatting, / commands, link & video previews

- **Formatting everywhere** (`lib/markup.ts` → `Markup.svelte`, no `{@html}`):
  `**bold**`, `__italic__`, `++underline++`, `~~strike~~`, `||spoiler||`,
  `` `code` ``, ```` ```lang ```` blocks, `> quote`, `[text](url)`, bare links,
  `@mentions` (open the profile), `#hashtags`, custom emoji. Articles add
  `# / ##` headings, lists, `---`, `>> pull quote`, `*italic*` and
  `[[gallery:0,1,2|caption]]`. Escapes only where a marker could start
  (`\**`), so `¯\_(ツ)_/¯` survives. Plain messages keep the old fast path.
- **Composer**: floating format bar on selected text + Telegram keys
  (Ctrl+B/I/U, Ctrl+Shift+X/P/M/N, Ctrl+Shift+., Ctrl+K). `shortcuts.ts`
  yields those keys to text fields when text is selected. The bar is
  portalled to `<body>` (the composer's `backdrop-filter` traps `position:fixed`).
- **`/` commands**: /article /poll /file /gif /sticker /video /call /search
  /dice /coin /shrug /tableflip /unflip /lenny /spoiler /help — ↑↓, Enter/Tab, Esc.
- **Articles v2** (still `[article:v1]`, new optional `images[]` + `subtitle`;
  old clients ignore them, Rust keeps only `data:image/…`, ≤60): toolbar,
  photo tray (reorder / remove with index remap, orphans appended), paste
  images, per-chat draft in localStorage, size guard (warn 10 MB, max 24 MB),
  photos shrunk to 1600 px WebP (JPEG on WebKit, which can't encode WebP).
- **Carousel.svelte**: swipe / drag, arrows, keys, dots or counter, thumbnails;
  click → gallery viewer. Used in the chat card, the reader and Telegraph pages.
- **Reader (Instant View)** `ArticleReader.svelte` + `lib/reader.ts`: full
  screen, progress bar, A−/A+ (Ctrl ±), Esc. Article bubble is now a card
  (cover or carousel, title, subtitle, excerpt, «Читать статью»).
- **Telegraph**: telegra.ph / graph.org links open in the reader via
  `api.telegra.ph/getPage` (`web::telegraph_page`); nodes → blocks, runs of
  figures merge into one carousel, YouTube embeds become preview cards.
- **Link previews** (`web.rs`, through the Settings proxy): OpenGraph/Twitter
  meta (head only, ≤768 KB), YouTube via oEmbed + thumbnail and an inline
  youtube-nocookie player (CSP `frame-src`), direct video links / `og:video`
  files downloaded to `<data>/webcache` (auto ≤60 MB when auto-download is on,
  button otherwise, ≤500 MB) and played inline; images too. Toggle:
  Settings → Behaviour → Link previews. `open_external` opens other links in
  the system browser. `transport::http_client()` is now `pub`.
- Verified: cargo tests (web parsers, article wire compat), svelte-check
  0 errors, rendered `ui/dist` in headless Chromium with a mocked Tauri bridge
  (formatting, quote, code, previews, YouTube iframe, inline video, carousel
  swipe, spoiler, external link, / menu, format bar, composer, both readers).

## 2026-10-12 — Session 43: v0.1.1 — moved to Arch Linux; Linux client fixes, iOS build, SMTP

- **Linux client (WebKitGTK)** — the dev machine is now Arch, and the Linux
  build behaved very differently from Windows (WebView2):
  - *Lag*: the ambient orbs (`body::before/after`, 60vmax, `blur(100px)`,
    animated forever) and every `backdrop-filter` are repainted on the CPU by
    WebKitGTK each frame. `main.ts` tags WebKitGTK (Linux UA without
    Chrome/Android) with `html.wk-lowfx`; `app.css` drops the blur, the drift
    and backdrop blurs there. Other platforms unchanged. Confirmed smooth on Arch.
  - *Mic test `NotAllowedError`*: WebKitGTK denies permission requests nobody
    answers → `Builder::on_permission_request` grants mic / camera / screen /
    notifications / clipboard-read (Linux only).
  - *Calls*: WebKitGTK ships with WebRTC off → `linux_webview::configure`
    enables WebRTC, media streams and MSE on the main webview
    (`webkit2gtk` crate, `v2_38`).
  - `tools/aquamarine.sh` launcher sets `WEBKIT_DISABLE_DMABUF_RENDERER=1`
    (blank/flickering window on NVIDIA + Wayland otherwise).
- **iOS CI finally produces an .ipa** (6 MB, unsigned, for AltStore/SideStore):
  signing is switched off inside the generated `project.pbxproj`; the
  `cargo tauri ios build -- KEY=VALUE` args were removed (Tauri forwards them
  to `cargo build`, which failed); packaging now rejects an empty `.app`
  (it once shipped a 300-byte .ipa while the job stayed green).
- **Relay SMTP**: app passwords are pasted as `abcd efgh ijkl mnop`; spaces are
  stripped before AUTH (Gmail answered 535 BadCredentials). Note: VPNs often
  block outbound 587/465 — route the SMTP host around the tunnel.
- **Linux/macOS scripts**: `.sh` versions of every Windows `.bat`/`.ps1`
  (root, `tools/`, `android/`). `android/env.sh` finds a *Linux* SDK/NDK/JDK
  (prefers JDK 17–21; installs SDK+NDK into `~/Android/Sdk` if missing);
  `patch-project.sh` rewrites Windows paths left in `gen/android`.
  `build-all.sh` runs `npm ci` (and reinstalls Windows-made `node_modules`).
- Version → 0.1.1 (client, site label).

---

## 2026-10-08 — Session 42: cross-platform builds, server admin panel, proxy ("connect through a server")

- **Build on all platforms** — `.github/workflows/release.yml`: a matrix that
  builds the Tauri client on Windows / macOS (arm64 + x64) / Linux and the
  relay server on Windows / macOS / Linux (x64 + arm64). Client bundles:
  NSIS/MSI, DMG, AppImage/deb/rpm (`tauri.conf.json` targets widened). On a
  `v*` tag everything attaches to the GitHub Release. `build-all.sh` mirrors
  `build-all.bat` for local macOS/Linux builds.
- **Connect through a server (proxy)** — optional SOCKS5 proxy for all client
  traffic (HTTP + relay WS). `aqua-client-core::transport` gained `set_proxy`/
  `proxy()`, a shared `http_client()` (reqwest `socks` feature) and `open_ws()`
  that dials the relay through `tokio-socks` when a proxy is set (TLS via
  `client_async_tls` either way). Tauri command `set_network_proxy`, persisted
  to `proxy.txt`, applied on boot; field in Settings → Account. Aquamarine does
  NOT run the tunnel — it points at a SOCKS5 proxy the user runs (e.g. OpenFlux).
- **Server admin panel** (`/admin`, neon style matching the client) — gated by
  `AQUA_ADMIN_TOKEN` (unset ⇒ disabled). Live stats (accounts, online, bots,
  blobs, storage, uptime) and access control: an **invite-only (solo) mode**
  with an email allowlist that gates registration (`email_request` returns 403
  for non-listed emails). `GET /v1/admin/stats`, `POST /v1/admin/config`;
  policy persisted to `aqua-admin.json`. E2EE unchanged — the relay still only
  forwards encrypted envelopes. First brick of the Solo mode in NETWORK_PLAN.
- `docs/NETWORK_PLAN.md`: documented the proxy flow (incl. OpenFlux commands),
  the admin panel and the build matrix. `tools/copy-pipo-server.bat` from
  session 41 still available for the old Java server sources.

Note on OpenFlux: it's a traffic-masquerading/circumvention tool. Aquamarine
integrates with it only as an external SOCKS5 proxy (a standard client
feature); the tunnel itself is run and configured by the user via OpenFlux.

---

## 2026-10-08 — Session 41: group calls, files in groups, accounts, passcode, ghost mode

- **Group / supergroup voice & video chats, channel live streams** (`groupCall.ts`,
  `GroupCallPanel.svelte`): full-mesh WebRTC over `[gcall:v1]` (broadcast via
  `group_control`, targeted via new `group_control_to`). Join bar in the chat,
  Discord-style stage (speaking rings, video/screen tiles, mute/deafen/camera/
  screen/leave), mini pill when you're in another chat. Channels: admins speak,
  subscribers listen and only connect to speakers. Harness: 3 browsers connect,
  video propagates.
- **Files / voice / edited photos in groups**: `attach_file_impl` uploads once and
  fans the offer out via `group_send` (fixes "no session for peer").
- **Edit / delete / typing in groups** via `[grpctl]`; admins with the right can
  delete others' messages (`can_delete_others`); own group edits now match (author
  "me" ↔ username).
- **Block users** (profile button, Settings → Privacy list): Rust drops their 1:1
  traffic and calls; presence/privacy treat them as "nobody".
- **Ghost / spy mode (AyuGram-style)**: no read receipts, no typing, invisible;
  keep deleted messages (marked) and edit history (`MsgMeta` on ChatMessage).
- **Accounts like Discord/Spotify**: sign-up = email + password + code + profile;
  **sign in on a new device** = email + password + code → server moves the
  username to the new identity (`POST /v1/users/login`, PBKDF2-SHA256 passwords,
  single-use tickets). Real emails when SMTP env vars are set.
- **Passcode lock** (`lock.ts`, `LockScreen.svelte`): required after sign-in,
  auto-lock 1/5/15/60 min, Ctrl+L, "forgot → sign out".
- Files panel: thumbnails, opens the gallery / audio queue. Context menus stay on
  screen (`use:keepOnScreen`). Browser context menu disabled outside text fields.
- `build-all.bat`: keeps going when only the NSIS installer step fails and closes
  a running Aquamarine.exe before copying (that's why the old exe kept launching).
- `docs/NETWORK_PLAN.md`: draft for solo/shared servers, server-0 succession,
  mesh, whitelist relays. `tools\copy-pipo-server.bat` for the old Java server.

---

## 2026-10-08 — Session 40: Telegram group settings, server tags, privacy, polls, gallery/editor/player, RU/EN

- **Own title bar** (`TitleBar.svelte`, `decorations: false`): neon brand, drag
  region, min / max-restore / close. `--tb-h` CSS var keeps full-screen
  overlays (gallery, photo editor) below it.
- **Clickable chat header** → group/channel settings or the person's profile.
- **Groups (Telegram parity)** — `GroupPanel.svelte` rewrite: info, edit, type &
  link, 15 member permissions + slow mode, reactions (all/some/none), admins
  with per-right toggles + custom titles, members (tags, restrict, ban),
  removed users, join requests, server tag editor, recent actions log.
  Backend: `extra` maps on Group/Settings/Member (UI-owned fields survive),
  `accept_group_snapshot` (only admins change metadata; members may only add
  people when allowed), `may_post` (author = session sender; mute/ban/channel/
  polls rules), `[grpself]` own tag, `[grpjoin]` join request, `[grpleave]`,
  `[grpctl]` group control fan-out (poll votes / closes / reactions now work in
  groups). Chat UI enforces the same: member tags + admin titles on authors,
  protected content (no forward/copy/save), allowed reactions, slow-mode
  countdown, link/media/poll/voice permissions, channel "Mute" button.
  Invite links `aqua://join/<gid>?admin=…` pasted into search → request card;
  public groups without approval auto-accept.
- **Server tags (Discord)** — ≤4 chars + badge + colour per group; members wear
  it (`profile.extra.clan`), shown next to names in chat list, header, authors,
  profile; click → "Open" or "Request to join".
- **Privacy (Telegram + Telegram X per-user)** — `privacy.ts`: last seen,
  photos, bio, phone, birthday, activity, calls, voice messages, invites, each
  Everybody / My contacts / Nobody + always/never lists. Profile → "Privacy for
  this person" writes the same exceptions. Enforced: per-peer presence (hidden
  online/activity, private fields via `pp`), restricted fields kept off the
  public card, **server-side photo ACL** (`POST /v1/users/:u/media_acl`,
  authenticated media fetch, 404 for others), calls auto-declined, voice/invite
  flags respected by senders, unwanted group adds left automatically.
- **Archive + mute** (right-click a chat): "Archived chats" row, unmuted
  archived chats pop back on new messages, muted chats are silent.
- **Polls**: per-voter state (change / retract, exact group tallies), quiz mode
  with right answer + explanation, public voters with timestamps, multiple
  choice with "Vote", stop poll, auto-close timer; new composer.
- **Articles publish in groups/channels** (`group_send`), cover is cropped to
  16:9 WEBP so messages stay small.
- **Ported from PipoHuy** (restyled): gallery viewer (zoom at cursor, pan,
  rotate, 1:1/fit, thumbnails, save as, copy), photo editor (crop w/ aspects,
  brush/eraser, text, adjust, 8 filters, rotate/flip, undo/redo), Spotify-style
  music player (waveform, ±10 s, speed, loop, queue of the chat's tracks).
- **Language RU/EN** (`i18n.ts` + `i18n.ru.ts`, ~700 strings, Russian plurals);
  Settings → Language; default follows the system language.
- Tests: rust group-rule tests (4), directory photo-ACL test; harness ui7/ui8.

---

## 2026-10-07 — Session 39: build fix, cropper, rich composer

- **Build fix**: `npm install` in `beforeBuildCommand` crashed on the user's PC
  ("Class extends value undefined" — a broken global npm). The Lottie player is
  now vendored as `ui/public/vendor/lottie_light_canvas.min.js` and loaded as a
  script on first use; `lottie-web` removed from package.json, the build is
  back to `npm --prefix ui run build` (no new npm deps).
- **ImageCropper.svelte**: drag / wheel / slider / arrow keys, round mask for
  avatars (512² WEBP), 3:1 for banners (1500 px); GIFs upload uncropped so
  they stay animated.
- **RichComposer.svelte** replaces the textarea: contenteditable that keeps
  the same text value with `[ce:…]` tokens but draws custom emoji as images
  (.tgs → first-frame snapshot), inserts at the caret (saved on input/blur),
  Shift+Enter line breaks (tail-<br> handling), plain-text paste, copy keeps
  tokens. Verified: type-to-compose, emoji at caret, multi-line send, clear
  after send, ↑ edit-last, drafts.
- Sync now copies with fresh mtimes (cp, not copy2) so cargo always sees changes.

---

## 2026-10-07 — Session 38: profiles for everyone, activity, Telegram panel, keyboard, new settings

**Server** — public profile media: `POST/GET/DELETE /v1/users/:u/media/{avatar|banner}`
(owner-only writes, anyone reads, PNG/JPEG/WEBP/GIF ≤ 8 MB, cached by version),
`extra` JSON on the profile card (banner colours, status, pronouns, birthday…),
`avatar_ver`/`banner_ver` on cards. SQLite migrates in place (ALTER TABLE).
Unit + HTTP e2e tests (16 total).

**Client (Rust)** — `publish_profile_full`, `set_profile_image(_bytes)`,
`clear_profile_image`, `profile_media_path` (disk cache per user/version);
media library: `.tgs` / `.webm` stickers & emoji, `pack.json` (emoji vs sticker
pack, Telegram source), GIF dir accepts mp4/webm, `add_media_files`,
`delete_media_item`; Telegram importer writes `pack.json`.
`activity.rs`: Discord-style rich presence from visible windows (Win32
EnumWindows + process image path): games (known exes + steamapps/Epic/Riot
folders), Spotify/AIMP/foobar/Yandex Music, YouTube/Twitch/Netflix/VLC…;
15 s watcher emits `aqua://activity`. Windows code type-checked on Linux via
`RUSTFLAGS="--cfg aqua_wincheck" cargo check`. Tests for classifier + media.

**UI**
- Discord user panel (bottom-left): avatar + status dot, name, activity line,
  mic / deafen (global, drive calls) / settings; status picker (online / idle /
  DND / invisible), custom status, share-activity toggle.
- Avatars + banners from the server everywhere (chat list, header, messages,
  profile); old local data-URL avatar is migrated up on connect; peers refetch
  when presence `pv` changes. Activity shown in chat list / header / profile.
- Telegram-style Emoji · Stickers · GIFs panel: Unicode emoji (1 870, EN+RU
  search, lazy chunk), custom packs, recents, section bar, animated .tgs via
  lottie (lazy chunk) and .webm; drop files onto the panel to add; import from
  Telegram; right-click a sticker/GIF/emoji message → save to collection.
  Inserts at the caret; preview of custom emoji above the composer.
- Reactions use the eltaller47 pack: bundled (`ui/public/reactions/` via
  `tools/bundle-reactions.bat`) → imported → auto-import with saved token;
  tokens `ce:eltaller47/<file>|<emoji>` fall back to plain emoji for others.
- Keyboard: Ctrl+K, ↑/↓/Enter in search, Alt+↑/↓, Ctrl+Tab, Ctrl+1…9/0,
  Alt+1…4, Ctrl+N/F/E/P/,/End, ↑ edits last, Ctrl+↑/↓ reply, type-to-compose,
  Ctrl+Shift+M/D/C, cheat-sheet Ctrl+/ (F1).
- Settings redesigned like Telegram mobile (profile card, grouped coloured
  rows, card sections) + new Privacy & Security and Stickers & Emoji sections.
- Fixes: media coordinator ignores muted loops; composer insert loop
  (`untrack`); settings above call overlay.

`beforeBuildCommand` now runs `npm install` first (lottie-web was added).

---

## 2026-10-07 — Session 37: Discord-grade calls & screen share

**Real bug found first:** App.svelte's "only one media plays at a time"
coordinator paused EVERY other <audio>/<video> when one started — so in a call,
starting a video tile (or the peer's stream) paused the peer's voice element
and other tiles. Call media is now tagged `data-call-media` and exempt.

**New call engine (`CallModal.svelte`, rewritten):**
- Four fixed transceivers negotiated up front (mic, camera, screen, screen
  audio) → camera/stream on/off is `replaceTrack`, no renegotiation, and
  camera + stream run at the same time. Explicit `media` signal carries
  mute/deafen/cam/stream state.
- Go Live picker: 720p/1080p/1440p/Source × 15/30/60 fps, Motion vs
  Text&detail (contentHint + degradationPreference), system audio, quality
  change mid-stream via applyConstraints + setParameters.
- `rtcQuality.ts`: bitrate from real frame size (1080p60 ≈ 7.2 Mbps, 1440p60
  ≈ 11 Mbps), sender params (maxBitrate/maxFramerate/priority high), codec
  order H.264 High → CB → VP9 → VP8 → AV1 (hardware encode on Windows), SDP
  tuning: Opus FEC/no-DTX 96 kbps voice, stereo 192 kbps stream audio,
  x-google start/min/max bitrate so video starts sharp instead of ramping up
  from ~300 kbps.
- Audio: device pickers, input gain, noise gate, PTT (window-focused),
  Ctrl+Shift+M / Ctrl+Shift+D, deafen, per-user voice/stream volume, output
  device (setSinkId), speaking rings (analyser + RTP audio level).
- Resilience: ICE restart on disconnected/failed instead of instant hang-up,
  30 s give-up, 45 s ring timeout, busy reject, serialized signal handling,
  early ICE candidates queued. TURN relay configurable.
- UI: compact card ↔ stage view (focus tile + strip, fullscreen, stats
  overlay: res/fps/bitrate/codec/HW-SW/ping/loss/route), network bars,
  generated ringtone/ringback and join/leave blips.
- Settings → Voice & Video (`VoiceVideoSettings.svelte`, `callSettings.ts`).
- capabilities: `core:window:allow-set-fullscreen`, `allow-is-fullscreen`.

**Verified** in Chromium with two app instances calling each other (mocked
Tauri bridge relaying signals): stream 1920×1080 received, 720p at 56–57 fps
with software VP9 on a 2-core box, voice target 96 kbps, camera + stream
simultaneously, live 1080p60→720p30 switch, mute badge sync, hang-up both
sides, all media elements playing, no page errors. Unit tests for SDP tuning
/ codec order / bitrates pass. UI regression QA passes.

---

## 2026-10-06 — Session 36: real UI bugs found by rendering the built bundle

The "build is stale" theory was wrong — the bugs were in the code. Rendered the
real `ui/dist` in headless Chromium with a mocked Tauri bridge and measured.
Fixed in `ChatWindow.svelte`:

- **Huge gaps between own messages**: CSS grid auto-placement. For `.msg.out`
  the avatar had `grid-column:2` and came first in the DOM, so the bubble
  (auto column) wrapped to row 2 → an empty 34px row + 10px gap per message.
  Fix: `grid-row:1` on all children + explicit columns. 46px → 2px.
- **Composer grows on first keystroke**: textarea was content-box, so
  autoresize (height = scrollHeight) doubled the padding (32 → 44px). Fix:
  `box-sizing:border-box`, `min-height:32px`.
- **Incoming bubbles stretched to ~640px**: `.msg-col` now `align-items:flex-start`.
- **Avatar letter hardcoded "V"**: `firstLetter` uses the real username/peer.
- **State leaking between chats** (search bar, "Editing" bar, selection, reply
  followed you into the next chat): reset on chat switch.
- Telegram layout: avatar at the bottom of an incoming block, none on own msgs.

QA flows passing: login gate, context menu, Edit (prefills text), search,
plus menu, group chat, group settings; no page errors.

---

## 2026-10-06 — Session 35: server crash diagnosed (port-in-use), launcher hardened

Drove the rebuild/clean on the user's PC via computer use (double-clicking
`rebuild-clean.bat`): old data wiped, client rebuilt fresh. The "server keeps
crashing / old sources" report turned out to be:

- **Not stale code.** Captured the server's real startup to a log
  (`diag-server.bat` → `server-run.log`): it printed `aqua-server listening
  addr=0.0.0.0:7777` and opened the directory — i.e. it starts fine. Then hit
  `GET http://127.0.0.1:7777/v1/stats` from the built-in browser and got
  `{"ok":true,"users":1,"bots":0,"online":0,"require_registration":true}` —
  proving the running exe has the latest server code (stats endpoint +
  mandatory registration). cargo didn't restamp aqua-server.exe simply because
  the source was unchanged since its last compile.
- **The crash was port 7777 already in use.** A leftover server instance held
  the port, so the next launch failed to bind and the window closed. Fixed
  `Aquamarine Server.bat` to `taskkill /f /im aqua-server.exe` before starting.
  Also added `rebuild-server.bat` (force-clean server rebuild) as a reassurance
  tool.

Net: server runs reliably now; client is the fresh build with all UI fixes.

---

## 2026-10-06 — Session 34: server launcher + https footgun fixes

Test-blocking bugs found from the user's screenshots:

1. **Server GUI/bat "just crashed".** The launcher ran `start /b powershell …`
   then `exit` — `/b` keeps the child attached to the cmd console, so `exit`
   killed it instantly. Replaced the primary **"Aquamarine Server.bat"** with a
   reliable **console** launcher that runs `aqua-server.exe` with
   `AQUA_LISTEN=0.0.0.0:7777`, `AQUA_DEV_CODES=1`, `RUST_LOG=info` and stays
   open showing logs + email codes. The windowed panel moved to **"Aquamarine
   Server (panel).bat"** (launched detached, `-STA`); `server-gui.ps1` now traps
   errors to `server-gui-error.log`. Pushed straight into `Aquamarine-apps` too
   so the server works without a rebuild.
2. **"Could not send code … https://127.0.0.1:7777".** The client hit **https**
   against the plain-http relay (TLS handshake fails). `normalizeServer` already
   downgrades https→http for loopback/LAN and is applied in `sendCode` /
   `continueAs` — the user's build predated it, so a client rebuild fixes it.
   (Removed an accidental duplicate normaliser.)
3. **"Continue as @vpst" failing** (log: "username is not registered" /
   "bound to different identity"): expected when the account isn't in the current
   server DB. `continueAs` now uses `connect()` and shows a clear "register below"
   message instead of a vague error.

Verified: `svelte-check` (0 errors), `npm run build` clean.

---

## 2026-10-05 — Session 33: composer height, edit/delete, typing

**Main fix.** Composer slimmed to Telegram height: `.composer` padding
8/10, `.input-wrap` padding 2px, `.composer-input` padding 6px / min-height
20px, attach + send buttons 36px (were 40). `autoresize` now also runs on
mount so the field starts at one line. Message grouping from S32 is correct
(2px within a minute-block, 10px between) — the user's giant gaps were a
**stale compiled exe**; rebuilding (`build-all.bat`) picks up the new UI.
(Confirmed the source CSS is tight via a render harness before shipping.)

**Edit & delete messages** (checklist). Context-menu **Edit** (own text/reply)
and **Delete** (own messages). New Rust `edit_message` / `delete_message`
commands mutate the local book (and persist Saved Messages); `apply_edit` /
`apply_delete` match the most recent message from an author by exact text.
New control prefixes `[edit:v1]{old,new}` and `[del:v1]{text}` intercepted in
`on_incoming_envelope` and applied to the peer's book; the frontend relays
them via `sendControl` for live 1:1 chats. Edit prefills the composer with an
"Editing message" bar; submit routes to edit instead of send.

**Typing indicators** (checklist). `[typing:v1]` control, throttled to once
per 3s on composer input (1:1 live chats only). Receiver shows "typing…" in
the header, auto-clearing after 4s. New `onTyping` event.

Verified: `cargo check` (client), `svelte-check` (0 errors), `npm run build`.

### Still open
See CHECKLIST.md — next: build+smoke-test, group media/voice, gid-scoped
group reactions/edit/typing, history persistence.

---

## 2026-10-05 — Session 32: login window every launch, TG grouping, in-chat search

**1. Login/register window on every launch.** The gate no longer keys off
`registered`; it opens the window whenever the client isn't in a live session
(guarded by `everConnected` so a mid-session drop doesn't pop it over the chat).
Silent auto-reconnect removed. A registered user gets a one-click **"Continue as
@me"** at the top of the window (`quickConnect`, exported from stores); they can
also register/log in as someone else. Registration is now always visible/testable.
(App.svelte, stores.ts, RegistrationWizard.svelte)

**2. Telegram-style message spacing.** `computeRows` now groups by sender **and
minute**: a run of messages from the same sender within the same `HH:MM` sticks
together (2px), and a sender change or a new minute starts a block with a 10px
gap. CSS: `.msg + .msg { margin-top: 2px }`, `.msg.first { margin-top: 10px }`.
(The selection-mode grid fix from S29 is already in source; the user's old build
predated it — a rebuild clears the giant gaps.)

**3. In-chat search** (checklist #5). The header search icon opens a search bar:
live match count, ↑/↓ (Enter / Shift+Enter) to cycle, smooth scroll-to-match and
accent highlight of hits + current hit. Frontend-only, no protocol. (ChatWindow)

Verified: `svelte-check` (0 errors), `npm run build` clean.

### Still open
See CHECKLIST.md — next: build+smoke-test, group media/voice, gid-scoped group
reactions/polls, edit/delete, typing indicators.

---

## 2026-10-05 — Session 31: mandatory registration, server DB auth, server GUI stats, clean-test

The server already had real crypto auth (signed challenge→verify + HMAC bearer
tokens) and a persistent SQLite **directory** (email-code → ticket → username↔
identity binding, profiles, bots). What was missing was making registration
**mandatory** and the onboarding order.

**Mandatory registration (server).**
- `AppState.require_registration` (default **on**; `AQUA_OPEN_SIGNUP=1` opts into
  an open relay). `AppState::new_open()` for tests/open relays.
- `claim_or_check` now refuses any username that isn't in the directory with
  `AuthError::NotRegistered` (403) — no more ephemeral unregistered handles
  (this is what let "впст" appear). Registered usernames still enforced to their
  owner identity across restarts; bots unaffected (they're directory rows).
- e2e test switched to `new_open()` (it exercises the raw relay). All 11 unit +
  3 e2e tests pass.

**Register-before-connect (client).**
- `Transport::register_account` — HTTP sign-in (challenge/verify) + `/v1/users/
  register`, no WebSocket. New `register_account` command + `tauri.ts` binding.
- `RegistrationWizard.finish()` reordered: register the account first, then
  `connect` (bundle upload + WS) — required now that the server gates on
  registration.

**Server GUI + stats.**
- New unauthenticated `GET /v1/stats` → `{users, bots, online, require_registration}`
  (counts only, no PII); `Directory::counts()`.
- `tools/server-gui.ps1` polls it every 3s and shows "Registered / Online /
  Bots" live; sets a stable working dir so the DB lands next to the exe.

**Clean testable version.**
- `reset-testdata.bat` — wipes the server DB + all local vaults incl.
  `%APPDATA%\app.aquamarine.desktop`, so testing starts with zero accounts (no
  personal data).
- `test-client-A.bat` / `test-client-B.bat` — launch two clients with separate
  `AQUA_DATA_DIR` vaults → two fresh identities/accounts on one machine.

**Checklist.** `CHECKLIST.md` added — full done/partial/todo review of the whole
build to drive the next sessions.

Build of the actual installers still runs on the user's Windows machine via
`build-all.bat` (can't cross-build Tauri/Windows from the Linux container).

Verified: `cargo check` (server + client), `cargo test -p aqua-server` (14
passing), `svelte-check` (0 errors), `npm run build` clean.

### Still open
See CHECKLIST.md — next up: build+smoke-test binaries, group media/voice,
gid-scoped group reactions/polls, edit/delete, in-chat search, typing.

---

## 2026-10-04 — Session 30: groups / supergroups / channels

Telegram-style groups, supergroups and channels, with per-group settings.

**Architecture — client-side fan-out (no server changes).** A group is a
client concept layered over the existing 1:1 Signal sessions: to post, the
sender encrypts once per member and relays N sealed envelopes. The relay
never learns the group exists. Membership/roles/settings are the group's own
state, distributed to members as `[grpinfo:v1]` snapshots (versioned for
convergence) and persisted in `groups.json`. Messages travel as
`[grpmsg:v1]{gid,author,body}` where `body` is the usual tagged text — so
text, quoted replies and inline custom emoji all work in groups for free.
A group payload can ride as the first X3DH message, so a brand-new member
establishes a session and learns the group in one shot.

**Rust.** `state.rs`: `Group`/`GroupMember`/`GroupSettings`, `groups` on
`AppState`, load/save + boot wiring, `group_summary`. `commands.rs`:
`list_groups`, `create_group`, `update_group`, `leave_group`, `group_send`
(with post-permission checks for channels/admins-only), `deliver_to_member`
(fan-out helper that establishes sessions on demand), and
`route_group_payload` hooked into both the envelope and the initial-message
incoming paths. `lib.rs`: commands registered.

**Frontend.** `types.ts` group types; `tauri.ts` bindings + `groups_updated`
event; new `groups.ts` store (live-refreshed). `stores.ts`: `sendCurrent`
routes `grp_*` ids through `group_send`; groups loaded on bootstrap.
`ChatWindow.svelte`: outgoing detection by username (not just "me"),
per-sender author names + distinct avatar colours in groups, group header
(member/subscriber count, settings gear, tap-to-open panel), read-only
composer notice when a channel/admins-only group forbids posting.
`ChatList.svelte`: the "+" is now a menu — New chat / New group / New
channel. New `NewGroupWizard.svelte` (type toggle, name, description, member
picker) and `GroupPanel.svelte` (info, full TG permission matrix, slow mode,
sign-messages, public/private, history visibility, member list with
promote/demote/remove + add, leave/delete). Mounted in `App.svelte`.

v1 scope: group **text / replies / inline emoji** fan out and sync. Media,
voice and cross-member reactions/poll tallies in groups are still 1:1-only
(need per-member file fan-out / gid-scoped control) — next. Large channels
fan out linearly (fine for self-hosted scale; Sender Keys later).

Verified: `cargo check`, `svelte-check` (0 errors), `npm run build` clean.

### Still open
- Group media/voice fan-out · gid-scoped group reactions & poll tallies ·
  Sender Keys for big channels · group calls (SFU) · Spotify/YT presence.

---

## 2026-10-04 — Session 29: TG spacing, direct downloads, one-click build

**Message spacing (looked huge in selection mode).** In selection mode a 3rd
grid child (the checkbox) was added to a 2-column `.msg` grid, so the bubble
wrapped to a new row and the vertical gaps blew open. Fixed by widening the
grid to a real 3-column template while selecting (`24px 34px 1fr`, mirrored
for `.out`) with explicit column placement — spacing is now tight/TG-like in
both normal and selection modes. (`ChatWindow.svelte`)

**Direct downloads from the site.** `site/index.html` buttons now point at the
site's own `site/downloads/<file>` with a `download` attribute (not GitHub) —
self-hosting the `site` folder serves the installers directly. Fixed filenames
(`Aquamarine-Setup.exe`, `Aquamarine.dmg`, `Aquamarine.AppImage`,
`Aquamarine.apk`) so nothing needs editing per release. REPO set to the real
repo. (NB: in the artifact *preview* the sandbox blocks downloads; they work
on the self-hosted site.)

**One-click build + server app.**
- `build-all.bat` (repo root): builds the relay (`cargo build --release -p
  aqua-server`) and the desktop client (`cargo tauri build`), installs the
  Tauri CLI if missing, then drops double-clickable apps into a NEIGHBORING
  `..\Aquamarine-apps\` folder (`Aquamarine.exe`, `aqua-server.exe`,
  `Aquamarine Server.bat`) and copies the NSIS installer into
  `site\downloads\Aquamarine-Setup.exe`.
- `tools/server-gui.ps1` + `tools/Aquamarine Server.bat`: a small WinForms
  control panel for the relay — Start/Stop, shows the LAN connect address,
  copy button, live log (binds `0.0.0.0:7777`, `AQUA_DEV_CODES=1` so email
  codes show in the log). Double-click, no console.
- `tauri.conf.json`: `bundle.active=true`, `targets=["nsis"]` so the build
  actually produces an installer.

Build still runs on the user's own machine (can't cross-build Tauri/Windows
from the Linux container). macOS/Linux/Android installers only build on those
OSes.

Verified: `svelte-check` (0 errors), `npm run build` clean.

### Still open
- Group calls (SFU) · channels/groups (Sender Keys) · real Spotify/YT
  presence (OAuth) · run `build-all.bat` once to populate the apps + downloads.

---

## 2026-10-04 — Session 28: login-gate fix + landing site

**Bug: never saw the login window; auto-entered a handle I never made.**
Root cause: the entry gate and auto-reconnect keyed off `username`, which is
loaded from `connection.json` — a convenience auto-reconnect *hint* written
the first time `connect_to_server` ever ran (during dev testing). A stale
hint ("впст") therefore looked like a logged-in account forever.

Fix — a real account record, separate from the connection hint:
- `state.rs`: new `account.json` (`registered`, `username`, `email`) via
  `load_account`/`save_account`/`clear_account_file`. `AppState` gains
  `registered` + `email`. On boot, `registered` comes only from
  account.json; if not registered, the stale hint username is ignored
  entirely. `InitInfo` gains `registered` + `email`. New methods
  `mark_registered()` and `log_out()` (clears account + connection hint +
  transport; keeps the identity/vault).
- `commands.rs`: `complete_registration(username, email)` and `log_out()`.
- `lib.rs`: both registered.
- `App.svelte`: entry gate now opens on `!registered` (not `!username`).
- `stores.ts`: auto-reconnect only when `registered && server_url &&
  username`; dropped the old `openFirstLaunchWizard` fallback (the
  RegistrationWizard is the single entry path now).
- `RegistrationWizard.svelte`: `finish()` calls `completeRegistration` and
  refreshes `info` so the gate closes immediately.
- `ProfileModal.svelte`: a **Log out** button on your own card → clears the
  account and reopens the login window.
- `types.ts` / `tauri.ts`: `registered`/`email` + `completeRegistration` /
  `logOut` bindings.

Net: first launch (no account.json) always shows the registration/login
window; the stale впст handle is gone; logout brings the window back.

**Landing site.** New `site/index.html` — a standalone, self-contained
landing page in the app's neo-style (teal/violet on void, aurora glow,
Sora/IBM Plex/JetBrains type). Hero with a live chat mockup + session-
fingerprint detail, feature grid, per-platform download rack (Windows /
macOS / Linux / Android) with OS auto-detection highlighting the right
build, a self-host section, and a note on why there is deliberately no web
client (keys must stay native — no messenger embed, for privacy). Release
wiring is a single editable `REPO` + `VERSION` block at the top of the
page's script; buttons point at `/releases/latest`. Also published as a
shareable artifact.

Verified: `cargo check`, `svelte-check` (0 errors), `npm run build` clean.

### Still open
- Group calls (SFU) · channels/groups (Sender Keys) · real Spotify/YT
  presence (OAuth) · build + host the release binaries · set `REPO`/`VERSION`
  in the site once releases exist.

---

## 2026-10-04 — Session 27: inline custom emoji

Custom (incl. Telegram-imported premium) emoji can now be dropped inline
into message text and render on both sides — no backend/shared-id work
needed, since the reference travels inside the text itself.
- Token format `[ce:<packSlug>/<filename>]` rides in the plain message text
  (so it syncs like any character) and renders as the matching image from
  the local sticker/emoji library; if the pack isn't present on a device it
  degrades to `:packSlug/filename:`.
- `composerBus.ts` (new): tiny store so the picker can push an emoji token
  into the active composer.
- `ChatWindow.svelte`: `richParts()` splits text on `[ce:…]`, `{#snippet
  richText}` renders text + `<img class="inline-emoji">`; used in the text
  bubble and in both reply-quote lines. `emojiMap` (slug/filename → path)
  built from `list_local_media`. An `$effect` appends pushed tokens to the
  composer, re-focuses, and refreshes the map.
- `MediaPicker.svelte`: new **Emoji** tab with a pack rail + compact grid;
  clicking inserts the token via `insertEmoji` and keeps the picker open
  (unlike stickers/GIFs which send-and-close).

Verified: `svelte-check` (0 errors), `npm run build` clean.

### Still open
- Group calls (SFU) · channels/groups (Sender Keys) · real Spotify/YT
  presence (OAuth) · build the clients.

---

## 2026-10-04 — Session 26: quoted replies (synced)

Proper Telegram-style replies. New `MessageKind::Reply { quoted_author,
quoted_text, text }` travelling as a `[reply:v1]{json}` plain-text payload
(same envelope trick as polls/articles), so the quoted snippet rides along
and both sides render the reply header — no shared message id needed.
- Rust: `state.rs` Reply variant + `[reply:v1]` classify + both preview arms.
- `types.ts`: reply kind.
- `ChatWindow.svelte`: context-menu **Reply** sets a reply target (no longer
  just quoting into the composer); a reply bar shows above the composer with
  a cancel; `submit()` wraps the outgoing text as `[reply:v1]`; messages of
  kind `reply` render a quoted header (author + snippet) above the text.

Verified: `cargo check`, `svelte-check` (0 errors), `npm run build` clean.

### Still open
- Group calls (SFU) · channels/groups (Sender Keys) · inline custom emoji ·
  real Spotify/YT presence (OAuth) · build the clients.

---

## 2026-10-04 — Session 25: synced reactions + video / screen-share calls

### (3) Cross-peer reactions (synced, no risky id rewrite)

Instead of threading a shared message id through the whole encrypt/relay
path, reactions are keyed by a **content key both peers compute the same
way**: `<authorUsername>:<fnv1a-hash(content)>` (author = my username for my
own messages, the chat id for the peer's). Toggling a reaction sends a
`[react:v1]{key,emoji,add}` control message; the peer applies it to the same
key, so a 1:1 chat shows merged reaction chips with counts (`mine` highlighted).
- Rust: `REACT_PREFIX` + `aqua://reaction` intercept (`commands.rs`).
- `messageMeta.ts`: new `{mine, peer}` reaction model + `hashContent`,
  `toggleMyReaction`, `applyPeerReaction`, `reactionChips`.
- `ChatWindow.svelte`: `reactionKey`/`reactionContent`, send+listen, chips
  with per-emoji counts.

### (2) Video + screen-share in 1:1 calls

Added live **camera** and **screen-share** to the existing audio call, with
proper **renegotiation** (perfect-negotiation: callee = polite, caller =
impolite; glare-guarded). `CallModal.svelte`:
- `ontrack` splits audio vs video → remote video element; local PiP preview.
- `startVideo("camera"|"screen")` / `stopVideo()` add/remove a video track
  then renegotiate; mid-call offers are answered on both sides.
- Call card grows to a 16:9 video view with camera / screen / mute / hangup
  buttons; the browser's native "Stop sharing" also ends the share.

### Verified

- `cargo check` clean. `svelte-check` 0 errors. `npm run build` clean.

### Still open

- Group calls (SFU) · channels/groups with admin panels (Sender Keys) ·
  inline custom emoji in text · real Spotify/YT presence (OAuth) ·
  build the clients (user's machine).

---

## 2026-10-04 — Session 24: listening presence + login panel

### Done

- **Listening / status presence (synced)**: you set a "Now playing"
  (title / artist / optional Spotify-YouTube link) and a custom status in
  your profile; it broadcasts over the E2EE control channel
  (`[status:v1]`) to every live chat. Peers see it live as a ♪ chip under
  the name in the chat header and as a "Listening to" card on the profile.
  No OAuth — the source is manual (clean, honest; a real Spotify/YT hook can
  layer on later). New `presence.ts`, `aqua://presence` event + Rust
  intercept, profile `listening` field + editor.
- **Login panel (Bug 6)**: the entry window is now a proper two-pane login —
  a brand hero (logo, tagline, feature bullets) beside the email-registration
  steps; collapses to a single column on narrow/mobile widths.

### Files

- Rust: `commands.rs` (`STATUS_PREFIX` + `aqua://presence` intercept).
- NEW `ui/src/lib/presence.ts`.
- `ui/src/lib/profile.ts` (`listening`), `ui/src/lib/tauri.ts` (`onPresence`),
  `ui/src/lib/components/ChatWindow.svelte` (listen / broadcast / header chip),
  `ui/src/lib/components/ProfileModal.svelte` (now-playing edit + card),
  `ui/src/lib/components/RegistrationWizard.svelte` (two-pane login).

### Verified

- `cargo check` clean. `svelte-check` 0 errors. `npm run build` clean.

### Remaining — need foundational work (not rushed)

- **Group calls / screen-share**: 1:1 WebRTC calls exist; groups need an SFU,
  screen-share needs getDisplayMedia track swap in CallModal (doable next,
  1:1 first).
- **Channels / groups with admin panels**: needs multi-party crypto
  (Sender Keys) + server group model — a dedicated effort.
- **Cross-peer reactions / inline custom emoji**: need a shared per-message
  id carried on the wire (message-format change).
- **Build the clients** (desktop + Android): on the user's machine.

---

## 2026-10-04 — Session 23: fast file loading (Tauri asset protocol)

### Done

- **Fast inline media** — switched every inline preview from the slow
  "read `Vec<u8>` over IPC → base64 data URL" path to Tauri's **asset
  protocol** (`convertFileSrc`): media now streams straight from disk with
  no IPC byte-copy and no base64 inflation. Huge speed/memory win for
  images, video, audio, stickers and GIFs, and real seeking for video.
  - `tauri.conf.json`: enabled `security.assetProtocol` (scope
    `$APPDATA`, `$APPLOCALDATA`, `$DOWNLOAD`, `$HOME`) and added
    `asset: http://asset.localhost` to the CSP img/media/connect sources.
  - `Cargo.toml`: added the `protocol-asset` feature to `tauri`.
  - `ui/src/lib/tauri.ts`: `assetUrl(path)` helper.
  - `ChatWindow`, `FilesPanel`, `SharedMediaPanel`, `MediaPicker`: inline
    URLs / thumbnails / background-player sources now use `assetUrl` (the
    old `read_media_bytes` stays as a fallback command).

### Verified

- `cargo check` clean (pulls `protocol-asset`). `svelte-check` 0 errors.
  `npm run build` clean.

### Backlog still open

1. Bug 6 — old login-panel look. 2. Music/listening presence (OAuth).
3. Discord-style group calls / screen-share. 4. Cross-peer reactions,
   inline custom emoji, channel/group admin panels. 5. Build the clients
   (desktop + Android, on the user's machine).

---

## 2026-10-04 — Session 22: full theme editor + voice waveform

### Done

- **Theme editor fullness** (was promised): a **live preview card** at the top
  of the Theme-editor section (mock header + incoming/outgoing bubbles +
  composer) that re-colours as you edit, and a **per-parameter description**
  on every colour — a `*` marker with a hover tooltip (plus the label's
  `title`) explaining exactly what each colour drives. (`theme.ts` gained a
  `desc` per `EditableColor`.)
- **Voice waveform** (ported from the old WaveformPanel): voice notes now
  decode once (WebAudio) into ~48 amplitude bars with a click-to-seek track;
  played bars fill with the accent colour. Falls back to the slider if decode
  fails. In `MediaPlayer.svelte`.

### Files

- `ui/src/lib/theme.ts`, `ui/src/lib/components/SettingsPanel.svelte`,
  `ui/src/lib/components/MediaPlayer.svelte`.

### Verified

- `svelte-check` 0 errors. `npm run build` clean.

### Backlog still open (reviewed 2026-10-04)

1. **Bug 6** — port the old login panel's look (entry is the email wizard now).
2. **Music/listening presence** (Spotify/YouTube "now playing") — needs OAuth
   integrations + backend; biggest remaining item.
3. **Discord-style calls** — a CallModal + voice signalling already exist;
   remaining is group calls / screen-share / Discord-like UI.
4. **Fast file loading** — inline media currently base64-dataURLs the whole
   file; wants a Tauri asset-protocol / blob-URL streaming path (backend).
5. **Cross-peer reactions**, **inline custom emoji in text**, **group/channel
   features** (admin panels, subscribers — see TG screenshots).
6. **Build the clients** — desktop + Android, on the user's machine.

---

## 2026-10-04 — Session 21: composer redesign + chat UX bug fixes

### Fixed

- **Textarea growth** — the composer now resets its height after send (reset
  deferred to a microtask so it measures the cleared value, not the old one).
- **Selection on click** — drag-to-select no longer triggers on a plain
  click. It arms on pointer-down in the gutter and only begins once the
  pointer actually drags onto another message / past an 8px threshold. A
  plain click selects nothing.
- **Single media playback** — a global coordinator (document-level `play`
  capture) pauses every other `<audio>`/`<video>` when one starts, so opening
  a file in the lightbox stops the inline preview and only one thing ever
  plays in the background (fixes the "preview + viewer" and "several at once"
  bugs).
- **Voice recorder controls** — the recording bar now has delete (trash),
  pause/resume, and send; the timer freezes while paused.

### Composer redesign

- The four composer buttons collapsed into two: a **paperclip** that opens an
  upward popup (Media & files / Article / Poll), and a **sticker/emoji**
  button that opens the media picker (stickers + GIFs).
- **One mic/camera button**: a single tap switches it between voice and
  round-video modes; **hold** records (release sends a voice note); dragging
  up while holding "locks" hands-free recording so the pause/delete/send bar
  stays up.

### Files

- `ui/src/App.svelte` — single-media `play` coordinator.
- `ui/src/lib/components/ChatWindow.svelte` — all of the above.

### Verified

- `svelte-check` 0 errors (23 cosmetic warnings). `npm run build` clean.

### Still open (next batch — big items)

- **Bug 6**: port the old login panel look (current entry is the email wizard).
- **Theme editor fullness**: live preview pane + per-parameter tooltips.
- **Music presence** (show what a user is listening to — Spotify/YouTube):
  needs OAuth integrations — design + backend work.
- **Port Discord-style calls**; **port the old players**; **fast file
  loading**; finish porting the rest of the old project.
- **Build the clients** — must run on the user's machine (can't cross-build a
  Windows .exe here): desktop `cargo tauri build` in `crates/aqua-client-ui`;
  Android `cargo tauri android init` then `… android build`.

---

## 2026-10-04 — Session 20: polls, reactions, pinned messages, read receipts

### Headline

- **Polls** (synced): new `MessageKind::Poll`, travels as a `[poll:v1]{json}`
  text payload (same backwards-compatible envelope trick as articles), so a
  poll is a real E2EE message that syncs + persists. Composer dialog
  (question + 2–10 options + multi-answer). Votes travel as `[pollvote:v1]`
  **control messages** keyed by the poll's stable id — intercepted on
  receive (new `send_control` command + `aqua://poll_vote` event), so a 1:1
  chat tallies both sides live. Telegram-style bars + %, your choice marked.
  Tallies persisted per device (`polls.ts`).
- **Read receipts** (synced, chat-level): opening a live chat with incoming
  messages sends `[seen:v1]`; the peer's outgoing ticks flip from a single
  check (sent) to a double check in the accent colour (read). Sending a new
  message resets it. New `aqua://seen` event; a `statusTick` snippet renders
  the right tick everywhere.
- **Reactions** (local per device): context-menu emoji bar (👍❤️🔥😂…),
  reaction chips under the bubble, toggle to remove. Stored in
  `messageMeta.ts`. (Cross-peer sync deferred — needs a shared message id.)
- **Pinned messages** (local per chat): Pin/Unpin in the context menu, a
  pinned bar under the header (matches the Telegram screenshot) that scrolls
  to the message; stored in `messageMeta.ts`.

### Files

- Rust: `state.rs` (Poll kind + classify `[poll:v1]` + previews),
  `commands.rs` (`send_control`, POLLVOTE/SEEN prefixes + intercepts),
  `lib.rs` (register `send_control`).
- NEW `ui/src/lib/polls.ts`, `ui/src/lib/messageMeta.ts`,
  `ui/src/lib/components/PollComposer.svelte`.
- `ui/src/lib/types.ts` (Poll kind), `ui/src/lib/tauri.ts` (sendControl +
  onPollVote/onSeen), `ui/src/lib/components/ChatWindow.svelte` (poll render
  + vote, reactions, pins, read-receipt tick, poll button), `App.svelte`
  (mount PollComposer).

### Verified

- `cargo check` (aqua-client-ui) clean. `svelte-check` 0 errors (23 cosmetic
  warnings). `npm run build` clean.

### Sync notes / limitations

- Poll votes + read receipts sync in **1:1** chats (both via the E2EE
  control channel). Group aggregation and cross-peer **reactions** need a
  shared per-message id / server aggregation — deferred.
- Polls are also usable in Saved Messages (local voting).

### Next: build + test all clients (desktop, then Android via `cargo tauri android`).

---

## 2026-10-04 — Session 19: drag-select, entry/email-registration gate, mobile pass

### Headline

- **Telegram-style drag-to-select** (`ChatWindow`): press beside a message
  (in the row gutter, not on the bubble text/controls) and drag up/down —
  every message the pointer passes over joins the selection (range select
  via `elementFromPoint` → `data-id` → index range). Text selection is
  suppressed mid-drag; the trailing click is swallowed so it doesn't toggle
  the start row off. Existing checkbox/context-menu select still works.
- **Removed the custom middle-click autoscroll** — scrolling is left to the
  webview (Tauri built-in) as requested.
- **Entry / email-registration gate**: the registration window now opens
  automatically whenever the user is not connected and has no username, and
  closes once connected (`App.svelte` effect + `registrationDismissed`
  flag for an explicit Skip). Restyled full-screen with a branded gradient
  backdrop in the new style. The 3-step email flow (email → 6-digit code →
  @username + profile) was already wired to the server
  (request_email_code / verify_email_code / register_username).
- **Mobile pass (basic)**: at ≤720px the side rail becomes a bottom tab bar
  (icons + labels), single-column list↔chat flow via `body.chat-open`
  (set in `selectChat`, cleared by a new mobile back button + `backToList`),
  shared-media panel hidden. Desktop layout untouched (all changes scoped to
  the media query / an ignored body class).

### Files

- `lib/components/ChatWindow.svelte` — drag-select, autoscroll removed,
  mobile back button.
- `App.svelte` — registration entry-gate effect.
- `lib/components/RegistrationWizard.svelte` — full-screen styling,
  `registrationDismissed`.
- `lib/stores.ts` — `chat-open` body class in `selectChat`, `backToList`.
- `app.css` — ≤720px single-column + bottom-bar rules.
- `lib/components/NavRail.svelte` — mobile bottom-bar layout + labels.

### Verified

- `svelte-check` → 0 errors (23 cosmetic warnings). `npm run build` clean.

### Phone client — how to actually build it

The UI is now responsive, but a real Android/iOS binary needs the Tauri
mobile tooling on the dev machine (can't run in this container):
1. Install Android Studio + SDK/NDK, set ANDROID_HOME / NDK_HOME.
2. From `crates/aqua-client-ui`: `cargo tauri android init` (generates the
   Gradle project), then `cargo tauri android dev` (emulator/device) or
   `cargo tauri android build` (APK/AAB).
3. iOS: `cargo tauri ios init` / `dev` / `build` on macOS with Xcode.
The Rust core + Svelte UI are shared as-is; only signing + the mobile
targets differ.

### Deferred

- Polls, reactions, pinned messages, read receipts (further old-code ports).
- Mobile grid height polish (list cell sizing) if a real device shows gaps.

---

## 2026-10-03 — Session 18: background player, chat folders, shared-media panel

### Context

User: add a top media player that keeps playing in the background when you
switch chats/tabs; add a side panel with files (Telegram "shared media",
not a Discord-style panel); add chat folders; port the best of the old code
(no economy), near-old style; Android project for concept.

### Headline

- **Background top player** (`player.ts` + `TopPlayer.svelte`): one
  persistent `<audio>` mounted in App, above the shell. Plays music in the
  background across chat/tab switches (the element never unmounts). Full
  transport: play/pause, seek, time, speed (1–2×), volume, close. Audio/
  music messages in chat and in the Files tab now play through it (a "Play
  in background" button); voice messages stay inline (short).
- **Chat folders** (`folders.ts` + `FolderManager.svelte`): Telegram-style
  folder strip above the chat list ("All" + custom folders with counts, +
  to manage). FolderManager: create/rename/delete folders and tick which
  chats belong. List filters to the active folder. Stored in localStorage.
- **Shared-media side panel** (`SharedMediaPanel.svelte`): right-side panel
  toggled by the chat header's info button. Scans the active chat for
  Media (image/video grid → lightbox), Files, Voice/Audio (→ background
  player) and Links (extracted from message text). Near-Telegram style.

### Files

- NEW `lib/player.ts`, `lib/components/TopPlayer.svelte`.
- NEW `lib/folders.ts`, `lib/components/FolderManager.svelte`.
- NEW `lib/components/SharedMediaPanel.svelte`.
- `App.svelte` — root is now a column (TopPlayer on top); chat + shared-
  media live in a `.main-pane` flex row; mounts FolderManager.
- `lib/components/ChatWindow.svelte` — audio → background player; header
  info button toggles shared-media panel.
- `lib/components/ChatList.svelte` — folder strip + folder filtering.
- `lib/components/FilesPanel.svelte` — audio → background player.

### Verified

- `svelte-check` → 0 errors (22 cosmetic warnings). `npm run build` clean.

### Next / deferred (more old-code ports)

- Per-chat folder assignment via right-click (now via FolderManager only).
- Polls, message reactions, pinned messages, read receipts — further
  old-code features still to port.

---

## 2026-10-03 — Session 17: Discord/Telegram overhaul — de-Chromium, theme editor, chat UX, profile

### Context

User wants the product to feel like Discord/Telegram: no Chromium-native
popups/menus/players, strict laconic settings (no emoji), maximum user
customisation (theme editor + fonts from system folders + wallpapers),
proper message scrolling (wheel + middle-click autoscroll), Telegram-style
message selection + forwarding, rich Discord-style avatar/profile card, and
Telegram premium/custom emoji without premium. "Посмотри прошлый код."

Note: the old desktop PipoHuy sources (`D:\IdeaProjects\…`) are 8 folders
deep — past the bridge's 7-folder staging limit — and the Windows shell is
click-only under computer use (no copy/paste), so they couldn't be pulled in
directly. Built natively from the existing Aquamarine theme system + the
already-read Android PipoHuy references + Discord/Telegram conventions.

### Headline

- **De-Chromium hardening** (`App.svelte`): block browser shortcuts
  (print/save/find/reload/zoom/view-source/downloads, F5, Alt+←/→),
  Ctrl+wheel zoom, file drag-drop onto the window; context menu already
  suppressed. Removed the last native `controls` player (CircleRecorder
  review) — now click-to-play, no Chromium chrome anywhere.
- **Theme editor** (`theme.ts` + Settings): new "Theme editor" section with
  colour pickers for accent ×2, surfaces (app/rail/list/input/bubble) and
  text (fg/muted/dim) — overrides win over palette/mode and apply live;
  accent soft/glow auto-derived. **Custom UI font** loaded from any
  .ttf/.otf/.woff/.woff2 file via a new `read_user_font` command +
  FontFace API → `--font-ui`.
- **Strict settings**: removed all emoji + the icon column; plain text nav,
  added a dedicated "Theme editor" entry.
- **Chat scrolling**: middle-click autoscroll (Windows-style, custom anchor
  puck, speed ramps with distance; Esc/click/blur stops) + Ctrl-wheel zoom
  blocked. Wheel scrolling works via overflow.
- **Message selection + forward** (Telegram-style): custom (non-Chromium)
  right-click menu per message (Reply / Forward / Copy / Select); selection
  mode with a top bar (Copy / Forward / count); new `ForwardPicker.svelte`
  overlay sends selected text/media to any chat via sendMessage/attachFile.
- **Rich profile card** (`ProfileModal` + `profile.ts`): real avatar images
  (pick a file → data URL, with Change/Remove in edit), presence dot, BOT
  tag, pronouns chip, Discord-style custom status line; stripped emoji from
  labels.
- **Premium/custom emoji**: the Telegram importer already handles
  `t.me/addemoji/<name>` (custom-emoji sets via getStickerSet) — works
  without Telegram Premium; import UI now says so. Inline custom-emoji-in-
  text rendering deferred (needs wire-protocol support).

### Files

- `App.svelte` — shortcut/zoom/drag blocking; mounts `<ForwardPicker/>`.
- `lib/theme.ts` — EDITABLE_COLORS, customColors/customFont in Settings,
  hexToRgba + applyCustomFont.
- `lib/components/SettingsPanel.svelte` — no emoji, no icon column, Theme
  editor section (colour pickers + font picker).
- `lib/components/ChatWindow.svelte` — selection, custom context menu,
  middle-click autoscroll, forward wiring.
- NEW `lib/components/ForwardPicker.svelte` — forward-to-chat overlay.
- `lib/components/ProfileModal.svelte` + `lib/profile.ts` — avatar images,
  status, pronouns, presence, bot tag.
- `lib/components/CircleRecorder.svelte` — dropped native `controls`.
- `lib/components/MediaPicker.svelte` — emoji-pack import hint.
- `src/commands.rs` + `src/lib.rs` + `lib/tauri.ts` — `read_user_font`.

### Verified

- `svelte-check` → 0 errors (22 cosmetic warnings). `npm run build` clean.
- `cargo check` (aqua-client-ui) → clean.

### Next / deferred

- Inline custom-emoji in message text (wire protocol).
- "Delete message" (no backend delete command yet) — omitted deliberately.
- Theme editor border colours (rgba) not yet pickable (hex-only slots).

---

## 2026-10-03 — Session 16: Telegram sticker import + player speed (ported from PipoHuy)

### Context

User pointed at the old messenger in `D:\IdeaProjects\` (the Java/Swing
"PipoHuy" desktop app + its Android sibling). Core concept to carry over:
**open, self-hostable, connect-by-IP** — each person can run their own
server. Explicitly asked to study the **sticker-copying** and **media
player** implementations, and to ignore the economy/shop code.

Studied the old `TelegramStickerClient` (sticker copy) and `NetworkClient`
(configurable `baseUrl` = the IP concept). The connect-by-IP concept is
**already present** in Aquamarine — the registration screen has a
server-URL field (`RegistrationWizard.svelte`), so any user can point the
client at any server/IP. So this session ported the two genuinely missing
pieces.

### Headline

- **Telegram sticker-pack import** — paste a `t.me/addstickers/<name>`
  link (or `@name` / short name) + a Telegram bot token, and the pack is
  pulled straight from the Telegram Bot API into the local stickers
  library. Nothing touches the Aquamarine relay — pure client-side.
  - `getStickerSet` → per-sticker `getFile` → download
    `file/bot<token>/<path>` → write `stickers/<title>/<emoji>_<nn>.<ext>`.
  - Static `.webp` render immediately; `.webm` video + `.tgs` animated are
    saved too (tgs not rendered yet) and reported in the result.
- **Playback speed** in the media player — cycling 1× / 1.25× / 1.5× /
  1.75× / 2× button (hallmark of the old app's advanced audio/video
  players). Applies to voice, audio and video.

### Files

- NEW `crates/aqua-client-core/src/telegram.rs` — `import_pack()` +
  helpers (`extract_short_name`, `ext_from_path`, filename/folder
  sanitize, urlencode). 5 unit tests, all green. Exported from
  `lib.rs` as `import_telegram_pack` / `ImportReport`.
- `crates/aqua-client-ui/src/commands.rs` — new `import_telegram_stickers`
  command (resolves `<data_dir>/stickers`, calls the core fn).
- `crates/aqua-client-ui/src/lib.rs` — registered the command.
- `ui/src/lib/types.ts` — `StickerImportReport`.
- `ui/src/lib/tauri.ts` — `importTelegramStickers()` API.
- `ui/src/lib/components/MediaPicker.svelte` — "⬇ Telegram" button +
  inline import form (token persisted in localStorage, token field is a
  password input), jumps to the freshly imported pack, toast with counts.
- `ui/src/lib/components/MediaPlayer.svelte` — playback-speed control.

### Verified

- `cargo test -p aqua-client-core telegram::` → 5 passed.
- `cargo check` (aqua-client-ui, from its dir) → clean.
- `svelte-check` → 0 errors (21 cosmetic warnings). `npm run build` clean.

### Notes / next

- Telegram import needs a bot token (any from @BotFather) because the Bot
  API requires one; there is no tokenless public sticker endpoint.
- `.tgs` (Lottie) rendering is a possible follow-up (needs a lottie
  player). `.webm` video stickers currently thumbnail poorly in the
  picker grid — could special-case them later.
- Could not stage the desktop PipoHuy sources directly (8 folders deep,
  bridge limit is 7); studied the Android sibling's `TelegramStickerClient`
  + `NetworkClient` instead, same algorithm.

---

## 2026-10-03 — Session 15: scrolling, settings nav, custom media player, de-Chromium

### Headline

- **Chat scrolling** — themed, rounded, hover-reveal scrollbars (webkit
  + Firefox), smooth jump-to-bottom.
- **Settings reorganised into departments** — two-pane layout with a
  left nav (Appearance / Behaviour / Notifications / Wallpaper / Accent /
  Bots / Account / About); only the selected section shows. Much less
  scrolling.
- **Custom media player** (`MediaPlayer.svelte`) — our own play/pause,
  seek bar, time, volume for audio/voice/video, so the Chromium native
  overflow menu (⋮ Download / PiP / speed) never appears. Images and
  video open a fullscreen **Lightbox** (scroll-to-zoom). Used in both
  ChatWindow and FilesPanel.
- **De-Chromium** — global `contextmenu` suppressed (except in text
  fields), middle-click autoscroll blocked, native image drag ghost +
  tap-highlight + overscroll-navigation disabled, text selection
  limited to message/body/input areas.

### Files
- `ui/app.css` — new scrollbar styling + webview-chrome suppression
  reset.
- `ui/App.svelte` — global contextmenu/auxclick handlers; mounts
  `<Lightbox/>`.
- `ui/components/MediaPlayer.svelte` (new), `Lightbox.svelte` (new).
- `ui/components/ChatWindow.svelte` — audio/voice/video use MediaPlayer;
  images + circles open lightbox / toggle sound; removed native
  `controls`.
- `ui/components/FilesPanel.svelte` — same MediaPlayer + lightbox in the
  inline preview.
- `ui/components/SettingsPanel.svelte` — department nav + per-section
  rendering; reset moved into About.

### Build / verification
- `npm run build` — 168 modules, clean.
- `svelte-check` — 0 errors (cosmetic a11y/unused-CSS warnings only).

---

## 2026-10-03 — Session 14: SMTP, server profiles, real settings, registration polish

### Headline (blitz)

- **Real SMTP** — `send_email_code` now sends via `lettre` when the
  `AQUA_SMTP_*` env vars are set; otherwise stays in dev-code mode.
- **Server profile cards in ProfileModal** — opening a peer's profile
  fetches their server-published card (display name, about, phone,
  bot flag) and shows it, falling back to the local contact book.
- **Real settings** — new functional toggles: Enter-to-send,
  auto-download media, notification sound, desktop notifications — all
  wired to actually change behaviour.
- **Registration polish** — resend-code button with a 45 s cooldown.

### Details

- `aqua-server/Cargo.toml` — added `lettre` (rustls, tokio1).
- `aqua-server/directory.rs` — `SmtpConfig::from_env` (HOST/USER/PASS/
  FROM/PORT); `dev_codes` auto-off when SMTP configured (overridable
  with `AQUA_DEV_CODES=1`); `send_email_code` spawns an async
  `send_smtp` (STARTTLS relay) when configured, else logs.
- `ui/theme.ts` — settings gained `enterToSend`, `autoDownloadMedia`,
  `notifySound`, `notifyDesktop`.
- `ui/SettingsPanel.svelte` — Behaviour + Notifications sections with
  real toggle switches; desktop-notif toggle requests OS permission.
- `ui/ChatWindow.svelte` — `onKey` honours enterToSend (Enter vs
  Ctrl/⌘+Enter); auto-download effect gated on `autoDownloadMedia`.
- `ui/stores.ts` — `notifyIncoming` plays a WebAudio chime + optional
  desktop Notification for messages outside the active chat.
- `ui/ProfileModal.svelte` — fetches `fetchDirectoryProfile` on open,
  merges the server card into the view (bot profiles get an orange
  banner).
- `ui/RegistrationWizard.svelte` — resend code with cooldown.

### SMTP setup (production)

Set before launching the server:
`AQUA_SMTP_HOST`, `AQUA_SMTP_USER`, `AQUA_SMTP_PASS`, optional
`AQUA_SMTP_FROM` (defaults to USER) and `AQUA_SMTP_PORT` (default 587,
STARTTLS). With these set, verification codes go out as real email and
are no longer returned in the API response.

### Build / verification

- `cargo check` server + client — green (lettre builds).
- `cargo test -p aqua-server` — 13 tests pass.
- `npm run build` — 187 KB bundle, clean.

---

## 2026-10-03 — Session 13: Bot API + file/player fixes

### Headline

- **Bot API** — the "база для ботов на апишнике" from the plan is in.
  Owners mint bot accounts + tokens in the app; bot programs drive them
  over a simple HTTP API (`/v1/bot/updates`, `/v1/bot/send`). Bot↔user
  messaging is plaintext (bots can't do X3DH) and clearly marked
  "🤖 bot · not encrypted" in the chat header. Shipped an example
  `bots/echo_bot.py`.
- Fixed the earlier **Saved Messages file bug** (raw `[file:v1 …]`
  text instead of a player): Saved Messages now builds the File
  message directly and copies the attachment into
  `<data_dir>/saved-media/`; `read_media_bytes` was broadened and the
  MIME table expanded (m4a/wav/ogg/mov/webm/…). Media auto-downloads
  up to 25 MB so players appear without a click. Generic files show an
  extension badge + "Save as…". (That fix went out at the start of
  this session.)

### Bot API — wire + server

- `aqua-core/wire.rs` — new `PayloadKind::Plaintext` for un-encrypted
  bot traffic.
- `aqua-server/directory.rs` — `bot_tokens` table; `register_bot`
  (mints `aqbot_…` token, flags the user `is_bot=1`), `bot_by_token`,
  `is_bot`, `list_bots_for_owner`. 1 new test (6 directory tests total).
- `aqua-server/lib.rs` —
  - `AppState.bot_queues`: in-memory per-bot inbound message queues
    (`BotQueue`/`BotUpdate`), ephemeral.
  - `deliver()` routes a `Plaintext` RelayMessage addressed to a
    registered bot into that bot's queue instead of a user inbox.
  - Endpoints: `POST /v1/bot/register` (Bearer, owner) → token,
    `GET /v1/bot/list` (Bearer) → owned bots+tokens,
    `GET /v1/bot/updates?token=&after=` → queued messages,
    `POST /v1/bot/send {token,to,text}` → plaintext to a user via the
    relay.

### Bot API — client

- `aqua-client-core/transport.rs` — `Incoming::Plaintext`,
  `send_plaintext`, `register_bot`, `list_bots`; `dispatch_frame`
  handles the new payload kind.
- `aqua-client-ui/commands.rs` — `on_incoming_plaintext` (creates a
  bot chat with `has_lock=false`), `send_message` detects a bot chat
  (`live && !has_lock`) and sends plaintext, new commands
  `start_bot_chat`, `create_bot`, `list_bots`.
- `ChatList.svelte` — directory-search results tagged `is_bot` start a
  bot chat via `start_bot_chat` ("/start") instead of X3DH.
- `ChatWindow.svelte` — header shows "🤖 bot · not encrypted" for bot
  chats.
- `SettingsPanel.svelte` — new **Bots** section: create a bot
  (username + display name), list your bots, copy their tokens.
- `bots/echo_bot.py` — runnable example bot.

### Bot message flow

User → bot: client sends a `Plaintext` RelayMessage to the bot's
username; the server's `deliver()` sees it's a bot and queues it.
Bot → user: the bot program polls `GET /v1/bot/updates`, replies via
`POST /v1/bot/send`, which the server relays to the user as a
`Plaintext` message that lands in their chat. The user's chat with a
bot is marked not-encrypted throughout.

### Build / verification

- `cargo check` (server + client) — green.
- `cargo test -p aqua-server` — 13 tests pass (6 directory incl. bot,
  4 auth, 3 e2e).
- `npm run build` — 183 KB bundle, clean.

### Status vs. the user's full plan

Done: tabs, auto-connect, Discord-style profile, themes, audio/media
+ players, voice messages, circle videos, sticker editor, articles,
wallpapers, photo editor, email registration + @username + directory
search, **bot API**.

Remaining / deferred: real SMTP (dev-code path works now), group
chats (Sender Keys), group + video calls (SFU), server-published
profile cards shown in ProfileModal, chat folders / typing / presence.

### Where to resume

Two good next steps: (1) show server profile cards in ProfileModal
(`fetch_directory_profile` on open — quick, finishes the directory
work); (2) real SMTP in `send_email_code` gated on `AQUA_SMTP_*`.
After that the big one is group chats (Sender Keys).

---

## 2026-10-03 — Session 12: server user directory + email registration + search

### Headline

The big server-side foundation is in. `aqua-server` now has a
**persistent user directory** (SQLite) with **email-code
registration**, **@username reservation**, **public profile cards**,
and **user search** — the base every remaining server feature (bot
API, profile sync) builds on. The client has a proper **3-step
registration wizard** on first launch and **live directory search**
in the sidebar.

### Server (`aqua-server`)

- New `src/directory.rs` — `Directory` backed by SQLite
  (`AQUA_DIR_DB`, default `aqua-directory.sqlite`; falls back to
  in-memory if the file can't be opened). Tables: `users`
  (username PK, identity_dh, email, display_name, about, phone,
  is_bot, created_at) and `email_codes` (email PK, code, ticket,
  issued_at, verified_at).
- Registration flow:
  1. `POST /v1/auth/email/request { email }` → 6-digit code, TTL
     10 min. Dev mode (default / `AQUA_DEV_CODES=1`) logs the code
     and returns it as `dev_code`. `send_email_code` is the SMTP
     hook for later.
  2. `POST /v1/auth/email/verify { email, code }` → one-time
     `ticket` (TTL 30 min).
  3. `POST /v1/users/register` (Bearer token) `{ ticket, username,
     display_name, about, phone }` → binds `@username` to the
     caller's identity, stores the profile + verified email. Username
     validated (3–32, `a-z 0-9 _ .`), email can't be bound to two
     usernames, username can't be stolen by another identity.
- Directory endpoints:
  - `GET /v1/users/search?q=` → up to 20 profile cards (username +
    display-name substring match).
  - `GET /v1/users/:user/profile` → one card.
  - `POST /v1/users/:user/profile` (Bearer, owner only) → update
    display_name / about / phone.
- `AppState.claim_or_check` now consults the directory first, so a
  **registered username survives server restarts** and is only
  usable by its owner identity (previously bindings were in-memory
  and lost on restart).
- `Cargo.toml` — added `rusqlite` (bundled).
- 5 new unit tests in `directory.rs` (full flow, username-taken,
  wrong-code, ticket single-use, bad-username). All green; the 3
  existing e2e tests still pass.

### Client core (`aqua-client-core`)

- `transport.rs` — new `ProfileCard` type; standalone
  `request_email_code` / `verify_email_code` (no auth needed);
  `Transport` methods `search_users`, `get_profile`, `register_user`,
  `update_profile`.
- Re-exported from `lib.rs`.

### Client UI (`aqua-client-ui`)

- `src/commands.rs` — Tauri commands `request_email_code`,
  `verify_email_code`, `register_username`, `search_directory`,
  `fetch_directory_profile`, `publish_profile`; `DirProfile` DTO.
- `src/lib.rs` — registers all six.
- `ui/src/lib/components/RegistrationWizard.svelte` (new) — 3-step
  modal (email → code → @username+profile) shown on first launch.
  In dev mode it prefills the code it got back. On finish: connects
  (saving auto-reconnect hints), registers the username + profile,
  mirrors into the local profile store, publishes the profile card.
- `ui/src/lib/newChat.ts` — `openFirstLaunchWizard` now opens the
  registration wizard instead of the old 2-field dialog.
- `ui/src/lib/components/ChatList.svelte` — debounced live directory
  search (280 ms) under a "Found on the server" header; each result
  row starts a chat and saves the contact. Falls back to the
  "Start chat with <exact username>" row only when the directory has
  no match.
- `ui/src/lib/tauri.ts`, `types.ts` — `DirProfile` type + API
  wrappers.
- `ui/src/App.svelte` — mounts `<RegistrationWizard />`.

### Flow summary

First launch → wizard: enter email → server logs + returns the dev
code → enter code → verified → pick @username, display name, about,
phone → Finish. The client connects, registers the handle, and
publishes the profile. From then on it silent-reconnects. Other
users type a name/handle in the sidebar search → the client queries
`/v1/users/search` → matching people appear under "Found on the
server" → one click starts an E2EE chat.

### Build / verification

- `cargo check` (server + client) — green.
- `cargo test -p aqua-server` — 3 e2e + 5 directory tests pass.
- `npm run build` — 164 modules, 178 KB bundle, clean.

### Deferred (next session)

- **Real SMTP** — wire `lettre` into `send_email_code`, gated on
  `AQUA_SMTP_*` env vars (host, user, pass, from). Dev-code path
  stays as the fallback.
- **Profile cards in the UI ProfileModal** — when opening a peer's
  profile, call `fetch_directory_profile` to show their
  server-published card (currently ProfileModal only shows local
  contact data).
- **Bot API** — `is_bot` column already exists; add BotToken auth +
  long-poll endpoints (`/v1/bot/updates`, `/v1/bot/send`).
- **Group chats** — Sender Keys ratchet.
- **Group / video calls** — SFU.
- **Chat folders**, **typing**, **presence** — pure WS.

### Where to resume

Bot API is the next self-contained server feature now that the
`users` table + `is_bot` flag exist. Add a `bot_tokens` table, a
`POST /v1/bot/register` (owner mints a token for a bot username),
and long-poll `GET /v1/bot/updates?token=` + `POST /v1/bot/send`.
Bots relay plaintext (they can't do X3DH), so model them as a
non-E2EE channel clearly marked in the UI.

---

## 2026-10-03 — Session 11: Articles, circle videos, sticker editor

### Headline

Three big features from the roadmap landed in one go, pure UI work
on top of the existing E2EE attach pipeline:

- **Articles** — Telegram-style long-form posts. Composer with
  Edit/Preview tabs, title, cover image, markdown body (headings,
  bold, italic, inline code, lists, links). Rendered as a card
  bubble with the cover on top and the article prose below.
  Wire-level backward-compatible: `[article:v1]{json}` tag inside
  the normal text envelope.
- **Circle videos (кружки)** — 320×320 round camera preview with a
  circular progress indicator around the viewfinder, "Start / Stop"
  button, max 60s, review step with Retake/Send. Rendered in chat as
  a 220×220 round video.
- **Sticker editor** — pick an image, zoom + pan it inside a
  checkered 512×512 canvas, pick a pack name, a sticker name, and an
  emoji tag from a built-in picker. Click Save → written as
  `<data_dir>/stickers/<pack>/<emoji>_<name>.png` so the normal
  folder-based media scanner picks it up on next picker refresh.

### What changed, file by file

**Backend**
- `src/state.rs` — new `MessageKind::Article { title, body,
  cover_data_url }`; `classify_kind` parses `[article:v1]{json}`
  tag before trying the file envelope; `preview_for` returns
  `📰 <title>` for articles.
- `src/commands.rs` — new commands:
  - `send_circle_video(peer_id, name, mime, bytes)` — writes to
    `<data_dir>/circles/<name>` and attaches with
    `category = "circle"`.
  - `save_sticker(pack, name, bytes)` — writes a PNG to
    `<data_dir>/stickers/<pack>/<emoji>_<name>.png` after
    sanitising path components; returns the final path.
- `src/lib.rs` — registers the two new commands.

**Frontend — new components**
- `ui/src/lib/components/ArticleComposer.svelte` — module-scoped
  `openArticleComposer(peerId)`, Edit/Preview segmented control,
  cover image picker (`readUserImage`), word-count + estimated read
  time, Ctrl+Enter to send.
- `ui/src/lib/components/CircleRecorder.svelte` — module-scoped
  `openCircleRecorder(peerId)`, grabs camera + mic via
  `getUserMedia`, mirror-flipped selfie preview inside a circle,
  60-second auto-stop with ring progress, review stage with Retake
  or Send, cleans up MediaStream + object URLs on close.
- `ui/src/lib/components/StickerEditor.svelte` — module-scoped
  `openStickerEditor(bytes, mime, srcName)`, 512×512 canvas with
  checkered backdrop, Zoom + Offset X/Y sliders, 60 built-in emoji
  tags, pack + name fields, writes via the new `saveSticker` API.

**Frontend — changed**
- `ui/src/App.svelte` — mounts `<ArticleComposer />`,
  `<CircleRecorder />`, `<StickerEditor />`.
- `ui/src/lib/types.ts` — `MessageKind` union adds `article`.
- `ui/src/lib/tauri.ts` — new API wrappers `sendCircleVideo` and
  `saveSticker`.
- `ui/src/lib/components/ChatWindow.svelte` — new composer buttons
  (📰 article next to stickers, 🎥 round-video button appears next
  to the mic when the text field is empty), new `renderKind` return
  value `"circle"`, render branches for both circle videos and
  articles, markdown renderer (same subset as the composer), CSS
  for round video, article bubble (Telegra.ph layout) and the two
  new composer buttons.
- `ui/src/lib/components/MediaPicker.svelte` — new "+ Create
  sticker" button in the footer; picks an image via dialog, reads
  it with `readUserImage`, closes the picker, hands off to the
  sticker editor.

### Flow summaries

**Articles.** Open the article composer from the composer's 📰
button. Write a title + markdown body, optionally upload a cover.
Preview tab shows a Telegra.ph-style render. Send → the message
becomes `[article:v1]{"title":"...","body":"...","cover_data_url":...}`
on the wire; the client parses the tag back into `MessageKind::Article`
on both ends. Old clients that don't recognise the tag just see the
JSON text — still readable, no crash.

**Circles.** 🎥 button in composer (only visible when text is empty)
→ camera modal opens → mirror-flipped round preview. Start → red
ring progresses around the viewfinder, up to 60s. Stop → review with
loop playback. Send → encoded as `video/webm` (vp9/opus if
available), written to `<data_dir>/circles/` and sent through
`attach_file_impl` with `category = "circle"`. Receiver renders
220×220 round video with controls.

**Sticker editor.** Open stickers tab → footer "+ Create sticker"
button → pick an image → editor opens with the image centred on a
512×512 canvas. Zoom and pan with sliders, pick a pack name
(auto-created if missing), a sticker name, and an emoji tag. Save
→ `canvas.toBlob("image/png")` → `saveSticker(pack, name, bytes)`
→ written as `<data_dir>/stickers/<pack>/<emoji>_<name>.png`. The
emoji prefix is picked up by the existing media scanner so the
sticker appears with its emoji tag the next time the picker opens.

### Build / verification

- `cargo check` — green.
- `npm run build` — 162 modules, 170 KB bundle, clean (one pre-existing
  a11y warning on InputPrompt autofocus; one bundler note about
  newChat.ts being both static+dynamic imported, non-fatal).

### Deferred (next session)

- **Email registration** — server-side; needs SMTP + three auth
  endpoints (`/v1/auth/email-request-code`, `/v1/auth/email-verify`,
  `/v1/auth/register`) + a user wizard screen.
- **Server-side user search** — `/v1/users/search?q=foo` endpoint,
  wires into the local "Start chat with …" row.
- **Server-side profile cards** — `GET /v1/users/{u}/profile`.
- **Bot API** — HTTP endpoints with long-polling, BotToken auth.
- **Group chats** — Sender Keys ratchet.
- **Group / video calls** — SFU + screen share.
- **Chat folders**, **typing indicator**, **presence** — pure WS.

### Where to resume

Email registration is the next structural step. All the subsequent
server-side items (user search, profile cards, bot API) depend on
having a registered-username table on the server, so that's the
foundation to lay first. Draft: new `users` sqlite table
(`username PRIMARY KEY, identity_fp, email, created_at,
display_name, about, phone`), the three auth endpoints above, and
a one-shot wizard screen before the Connect dialog.

---

## 2026-10-02 — Session 10: no connect button, wallpapers, photo editor, inline previews

### Headline

- **Connect button is gone.** No more "click plug to sign in" — on
  first launch the setup dialog auto-opens, after that every launch
  silent-reconnects to the saved server with the saved @handle.
  The sidebar shows a small pulsing dot: green = online, grey =
  retrying.
- **Chat wallpapers.** Settings → *Chat wallpaper*: seven presets
  (Aurora / Sunrise / Ocean / Forest / Midnight / Parchment / Stars)
  plus "Custom…" that lets you pick any image from disk and uses it
  as a dimmed background behind the chat.
- **Simple photo editor.** When you attach a single image via the
  paperclip, a modal opens with the picture, Rotate ⟲/⟳, Brightness,
  Contrast and Saturation sliders, and two actions — "Send original"
  or "Send edited" (rasterises to a canvas and sends the PNG).
- **Audio / image / video preview inside the Files tab.** Click any
  previewable row and it expands in-place with a player or a big
  thumbnail. The arrow button on the right still jumps to the chat
  the file lives in.
- **New Rust commands:** `send_edited_image` (writes rotated/filtered
  PNG into `<data_dir>/images/` and routes through the attach
  pipeline), `read_user_image` (safe scoped read for the editor /
  custom-wallpaper picker — only image extensions, 8 MB cap).

### What changed, file by file

**Frontend — new / rewritten**
- `ui/src/lib/components/PhotoEditor.svelte` — modal with canvas-based
  export; `editAndSend({path, bytes, mime, name, peerId})` returns a
  promise that resolves `true` when the user sent the edited version.
- `ui/src/lib/theme.ts` — `WALLPAPERS` presets + `wallpaper` and
  `wallpaperCustom` fields in `Settings`; applies `--chat-wallpaper`
  CSS variable.
- `ui/src/lib/newChat.ts` — `openFirstLaunchWizard()` for fresh
  installs with no saved hints.

**Frontend — changed**
- `ui/src/App.svelte` — mounts `<PhotoEditor />` alongside the other
  modals.
- `ui/src/lib/stores.ts` — on bootstrap: silent reconnect if hints
  exist, otherwise auto-open the first-launch wizard. No manual
  Connect UI anywhere.
- `ui/src/lib/components/NavRail.svelte` — Connect button removed,
  replaced with a small 8 px pulsing dot (green online, grey
  retrying).
- `ui/src/lib/components/SettingsPanel.svelte` — new "Chat wallpaper"
  section with preset grid + Custom…; `read_user_image` is used to
  load the picked file; applies via `settings.wallpaper` /
  `settings.wallpaperCustom`.
- `ui/src/lib/components/FilesPanel.svelte` — rows are now a split
  grid: main part toggles an inline preview (audio/image/video),
  the ↗ chip jumps to the chat.
- `ui/src/lib/components/ChatWindow.svelte` — `.messages` reads
  `--chat-wallpaper`; `sendFiles` detects a single-image pick and
  routes through the photo editor first.
- `ui/src/lib/tauri.ts` — new `sendEditedImage`, `readUserImage` API
  wrappers.

**Backend**
- `src/commands.rs` — `send_edited_image` and `read_user_image`
  commands.
- `src/lib.rs` — registers both.

### Build / verification

- `cargo check` — green.
- `npm run build` — 156 modules, 150 KB bundle, clean.

### Deferred (next session)

- **Articles** (Telegram-style long-form) — a dedicated article
  composer (title + rich body + cover), new `[article:v1]{...}`
  tag in the text envelope so classify_kind renders a card. Keeps
  wire-level backwards compatibility; old peers see the raw JSON.
- **Email registration** (biggest remaining server item) — see the
  Session 9 roadmap. SMTP + `/v1/auth/register` + wizard screen.
- **Circle video messages (кружки)** — small webcam recorder with
  circular canvas, category = "circle".
- **Sticker editor** — crop + background removal + emoji tag.
- **Bot API** — HTTP endpoints with long-polling for bot accounts.
- **Group chats** — Sender Keys for multi-party ratchet.

### Where to resume

Articles are the next quick visible win — no server work needed, pure
frontend. Draft a `ArticleComposer.svelte` with title + markdown
body, encode `[article:v1]{json}` into the text payload, extend
`classify_kind` in state.rs to detect the tag and store an `Article`
MessageKind, and ChatWindow renders them as a Telegra.ph-ish card.

---

## 2026-10-02 — Session 9: Nav tabs, profile, voice messages, roadmap

### Headline

- **Working nav tabs.** The sidebar's four icons (Chats, Calls, Files,
  Contacts) now actually swap the middle panel, instead of just
  highlighting themselves. Each panel is a first-class view with its
  own header, filters, empty state and keyboard flow.
- **Voice messages.** Mic button in the composer (shown when the text
  field is empty): tap once → recording bar appears with pulsing red
  dot + MM:SS counter + Cancel/Stop; tap again (or Stop) → the WebM
  Opus blob is sent through the normal encrypted file pipeline with
  `category = "voice"` and renders as a labelled audio bubble on the
  other side.
- **Audio-file attachments** now render as an inline `<audio controls>`
  bubble with the right icon, instead of a plain file card.
- **Discord-style Profile modal** — click the avatar in the sidebar
  bottom to open your own, or click a contact to open theirs. Banner
  gradient, big avatar, display name, @username, about, phone,
  birthday, favourite music, session fingerprint. Fully editable for
  the current user; for contacts it edits the local contact book.
- Removed the extra "+" button from the chat header (there's one in
  the sidebar already — one is enough).
- **PipoHuy roadmap documented** for the big server-side items below.

### What changed, file by file

**Frontend — new files**
- `ui/src/lib/theme.ts` — kept from Session 8; CSS-variable driven
  theme + palette + density + font-scale, 6 palettes including
  "Blurple" (the PipoHuy Discord look).
- `ui/src/lib/profile.ts` — localStorage-backed current-user profile
  (display name, about, phone, birthday, favourite music, banner A/B).
- `ui/src/lib/callHistory.ts` — localStorage call log with direction,
  status, duration; written by `CallModal.endCall()`, read by `CallsPanel`.
- `ui/src/lib/components/SettingsPanel.svelte` — kept, as Session 8.
- `ui/src/lib/components/ProfileModal.svelte` — Discord-style card.
- `ui/src/lib/components/CallsPanel.svelte` — history list with
  per-row direction glyph (↗/↙), status label, click-through to the
  underlying chat.
- `ui/src/lib/components/FilesPanel.svelte` — scans every chat's
  messages for file attachments, lets you filter by
  All/Images/Videos/Audio/Docs, click row to jump to the chat.
- `ui/src/lib/components/ContactsPanel.svelte` — Contacts tab reads
  the `aqua-contacts` book, lets you search by name/phone/username,
  "Open chat" if one exists or "Start chat" otherwise.

**Frontend — changed**
- `ui/src/App.svelte` — switches the middle panel on `$activeTab`
  (chats / calls / files / contacts); mounts `ProfileModal` and
  `SettingsPanel`. `Ctrl+K` focuses search, `Ctrl+,` opens Settings.
- `ui/src/lib/stores.ts` — new `activeTab` store, `NavTab` type.
- `ui/src/lib/components/NavRail.svelte` — nav buttons drive
  `$activeTab`; avatar button at the bottom opens profile.
- `ui/src/lib/components/ChatWindow.svelte` — audio & voice bubble
  rendering, mic button in composer, recording bar overlay, jump-to-
  bottom scroll button (kept from Session 8), removed extra "+" from
  the header, `renderKind` now returns `"audio" | "voice"` too.
- `ui/src/lib/components/CallModal.svelte` — logs every call to
  `callHistory` on end, with direction / status / duration computed
  from the state transition. `connected` state now carries
  `direction` and `placed_at` for accurate history.
- `ui/src/lib/tauri.ts` — new `sendVoiceMessage` wrapper.

**Backend**
- `src/commands.rs` — new `send_voice_message(peer_id, name, mime, bytes)`
  command: writes bytes to `<data_dir>/voice/<safe_name>`, routes
  through `attach_file_impl` with `category = "voice"` and the
  recorded MIME type.
- `src/lib.rs` — registers the new command.

### How voice messages flow

1. User taps mic → `navigator.mediaDevices.getUserMedia({audio:true})`
   → `MediaRecorder` with `audio/webm;codecs=opus`.
2. Chunks accumulate in `recordedChunks[]`; a 250 ms interval ticks
   the HH:MM:SS counter in the recording bar.
3. User taps Stop → `recorder.stop()` → `onstop` concatenates the
   chunks into a Blob → `blob.arrayBuffer()` → `Uint8Array` → `number[]`
   → `api.sendVoiceMessage(peer_id, name, mime, bytes)`.
4. Rust command writes the bytes to `<data_dir>/voice/<name>` and
   calls `attach_file_impl(..., Some("voice"), Some(mime))`.
5. For Saved Messages that short-circuits locally (no network); for
   a real peer it goes through the ChaCha20-Poly1305 blob store +
   E2EE offer envelope same as a regular file.
6. Both ends render it as the audio bubble labelled "Voice message".

### Build / verification

- `cargo check` — green.
- `npm run build` — 153 modules, 139 KB bundle. Clean (one pre-existing
  a11y warning on InputPrompt autofocus).

### Scope cut this round

Big items the user listed that are out of scope for this session but
scoped out below. Everything else from the user's list ("email
registration", "circle video messages", "sticker editor", "bot API")
is in the roadmap:

### Roadmap — big items from the user's list

1. **Email registration + @username reservation** — new endpoints on
   aqua-server:
   - `POST /v1/auth/email-request-code` { email } — generates a 6-digit
     code, emails via SMTP (user mentioned `applememes` or any
     corporate address).
   - `POST /v1/auth/email-verify` { email, code } — returns a short
     registration token.
   - `POST /v1/auth/register` { token, username, bundle } — reserves
     `@username`, binds it to the identity bundle.
   - UI: a one-time registration wizard screen before the Connect
     dialog; the current username field becomes an auto-reserved
     `@handle`.
2. **Discord-style profile on the server side** — once registered,
   publish the user's profile card (display name, about, banner, phone
   if they chose to share) alongside the prekey bundle at
   `GET /v1/users/{username}/profile`. Clients cache + show in the
   same ProfileModal we have now.
3. **Server-side username search** — `GET /v1/users/search?q=foo` so
   you can find people you haven't chatted with. Wires into the
   existing "Start chat with …" row in the sidebar search.
4. **Circle video messages (кружки)** — composer button that opens a
   small 240×240 camera preview, holds-to-record, writes a webm,
   sends with `category = "circle"`. Renderer draws them circular and
   centred in the chat.
5. **Sticker editor** — in-app crop + background removal + add emoji
   tag for a sticker before dropping it into a chosen pack folder.
   Would reuse the existing picker storage.
6. **Bot API** — HTTP endpoints on aqua-server for bot accounts to
   receive and send messages via long polling, like Telegram's Bot
   API. Separate `BotToken` auth; bots can't initiate X3DH sessions
   so group them under a `channel`-style broadcast model.
7. **Call rework** — group calls, video calls, screen share.
   WebRTC foundation is already there; needs an SFU for 3+ participants.
8. **Chat folders + typing + presence** — standard features that
   PipoHuy had, pure WS additions.

### Where to resume

- Email-registration backend is the next high-leverage change —
  everything else (profiles-on-server, search, bot API) depends on
  having a server-side identity record. Draft the three endpoints
  above in `aqua-server`, back it with a tiny SQLite table
  (`users (username PRIMARY KEY, identity_fp, email, created_at)`),
  and wire the registration wizard screen in the UI.

---

## 2026-10-02 — Session 8: Saved Messages + PipoHuy study, UX polish

### Headline

- Added **Saved Messages**, a built-in self-chat that always works
  (no server, no second instance). Click once, send text / stickers /
  GIFs / drag-dropped files, everything renders inline. Persisted to
  `saved-messages.json` next to the vault.
- Finished the UX pass users were tripping on: small square `+` button
  (not a huge CTA), 4-field "Add contact" dialog with ФИО / phone,
  global search by name/phone/username, auto-reconnect on launch, big
  banner above the composer when the active chat isn't live, send
  failures restore the typed text + toast the real error.
- Studied the attached **PipoHuy** Android messenger archive —
  Discord-style, Java + Gson + OkHttp + WebSocket, server-side
  accounts, chat folders, sticker import from Telegram, profile with
  display name / about / birthday / banner colours, Discord-like
  servers + channels, in-app coins / shop. Mined its API shape for
  the Aquamarine roadmap below.

### Saved Messages — how it works

- Always pinned to the top of the chat list with the ☁ avatar and
  "end-to-end encrypted" lock (your vault IS encrypted at rest with
  ChaCha20-Poly1305, the key is in the OS keyring — nobody else sees it).
- `summary.live = true` so composer, paperclip and sticker picker
  are enabled regardless of whether you're connected to a server.
- `send_message` / `attach_file_impl` detect `peer_id ==
  SAVED_MESSAGES_ID` ("__saved__") and short-circuit through a local
  path: no X3DH, no ratchet, no blob upload. File messages set
  `saved_at = source_path` so the UI renders inline from disk directly.
- Messages persist to `<data_dir>/saved-messages.json` on every write;
  loaded back on next boot.

### What changed, file by file

- `aqua-client-ui/src/state.rs` — `SAVED_MESSAGES_ID` + `SAVED_MESSAGES_FILE`
  constants, `load_saved_messages` / `save_saved_messages` helpers,
  `one_line_preview` for chat-card previews, boot seeds the Saved
  Messages chat before everything else, `uuid_like` exposed. Welcome
  text rewritten to point at Saved Messages as the quickest way to
  try the app.
- `aqua-client-ui/src/commands.rs` — `send_message` and
  `attach_file_impl` branch on `SAVED_MESSAGES_ID` and route through
  the local-only path.
- `ui/src/lib/contacts.ts` (new) — per-peer metadata (full_name, phone)
  stored in `localStorage`.
- `ui/src/lib/newChat.ts` (new) — shared `openNewChatDialog(prefill?)`
  with 4 fields (ФИО, phone, username, first message).
- `ui/src/lib/stores.ts` — `sendCurrent` returns the real error string
  so the caller can surface it, `visibleChats` + `activeChat` enriched
  with stored contact names and matched by name/phone/username.
- `ui/src/lib/components/ChatList.svelte` — small 34×34 gradient `+`
  square next to the search bar (pulses when there are no live chats);
  "Start chat with <q>" row at the top of the list when the search
  term doesn't match. Search placeholder: "Search by name, phone or
  username".
- `ui/src/lib/components/ChatWindow.svelte` — small gradient `+` button
  in the chat header too, phone + @username shown in the peer-status
  line, large "You can't send messages here" banner above the composer
  when the active chat is a stub, `submit()` restores the typed text
  on failure and shows a toast with the real backend error, Tauri
  native drag-drop via `onDragDropEvent`.
- `ui/src/lib/components/MediaPicker.svelte` + related — unchanged
  from Session 7, still works for Saved Messages too.
- `run-second.ps1` — rewritten to launch the already-built .exe with
  `AQUA_DATA_DIR` set, instead of `cargo tauri dev` which locks
  `target/` and fights for Vite's :5173.
- `tauri.conf.json` capabilities — added `core:webview:default` for
  Tauri drag-drop events.

### PipoHuy study — what we're borrowing (roadmap)

The old Java/Android project traded E2EE for a classic Discord-style
server with central account storage. We keep the E2EE core, borrow its
UX ideas:

1. **Server-side user directory** — `/api/users/search?q=foo` on the
   server, so you can find a peer you've never chatted with. Right now
   you have to know their exact username. Would also enable "found a
   match: @ivan2027" rows in the local search.
2. **Registration-by-email on the server** — the user's "last stage"
   milestone. PipoHuy did `/api/auth/register` with username+password
   and returned access+refresh JWTs; we'll do email+code and return
   the same auth tokens the server already uses for the signed
   challenge. Identity key stays local; the email login just binds a
   human-friendly handle to a device identity on the server.
3. **Rich profile** — display name, about, birthday, avatar — stored
   on the server (unencrypted, obviously, since it's your directory
   entry) and in the local vault. Right now full_name/phone are
   `localStorage`-only.
4. **Chat folders** — organize chats into client-side folders (All,
   Work, Family, etc.). Pure UI, no server work.
5. **Typing indicator + online status via WebSocket** — PipoHuy had
   `chat.typing` and `presence` events. Our WS already carries
   envelopes; adding small control frames is a few lines.
6. **Telegram sticker import via Bot API** — PipoHuy pulled packs
   with `getStickerSet` + `getFile` from `api.telegram.org`. We'd
   fetch, download the webp files, drop them into
   `<data_dir>/stickers/<pack>/` — the user's folder-based picker
   already handles the rest.
7. **"Clear chat" + per-chat delete-media option** — right-click menu
   action to wipe a chat locally and/or on the server.
8. **Chat types** — PipoHuy had GLOBAL / PRIVATE / GROUP / SAVED /
   CHANNEL / SYSTEM. We now have SAVED; adding GROUP (shared ratchet
   via Sender Keys or sealed-sender fanout) is the next big crypto
   milestone after email registration.

### Build / verification

- `cargo check` — green.
- `cargo test -p aqua-client-ui` — 3 media tests still pass.
- `cargo test -p aqua-core` — 37 tests still pass.
- `npm run build` — 140 modules, 107 KB bundle, clean (one pre-existing
  a11y warning on InputPrompt autofocus).

### Where to resume

- Saved Messages is the simplest demo path — click ☁ in sidebar, type,
  drop files, send stickers. No server needed.
- For the server-roadmap items the first mechanical step is adding
  `/v1/users/search?q=foo` to aqua-server (it already has prekey
  bundle lookups keyed by username, so this is a thin SELECT) and
  wiring a `search_users` Tauri command + UI row in the chat-list
  "Start chat with …" area.

---

## 2026-10-02 — Session 7: stickers, GIFs, and inline media

### Headline

The app now has a Telegram-style stickers + GIFs picker, plus inline
rendering for images, GIFs, and video attachments. The user drops files
into local folders (no cloud, no DRM, no vendor lock-in — the philosophy
the project was founded on) and they appear in the picker. All media
goes through the same end-to-end-encrypted file-transfer pipeline as
regular attachments; the only new thing on the wire is an optional
`cat` (category) field in the FileEnvelope that tells the receiving UI
to render inline instead of as a file card.

### Behaviour

Media library layout, created automatically on first boot under
`<data_dir>/`:

```
stickers/
    <pack-name>/
        😀_grin.png          ← optional "emoji_" prefix tags the sticker
        sad.webp
        wave.gif             ← animated stickers are just GIFs
gifs/
    reaction.gif
    cat.gif
```

A `README.md` is written into each folder on first boot explaining the
drop-file-here workflow.

Sending a sticker or GIF reuses the attachment pipeline:
1. Click the sticker icon in the composer → MediaPicker opens.
2. Grid of thumbnails loaded on-demand via `read_media_bytes`.
3. Click → `send_sticker` / `send_gif` command → same `encrypt_file`
   path as a plain attachment, but with `category = "sticker"` or
   `"gif"` set in the envelope.
4. Receiver auto-downloads anything flagged `sticker`/`gif` or any
   attachment ≤ 500 KB, then renders inline.

Inline rendering branches on `(category, mime)`:
- `sticker` → 128×128 image, no bubble, drop-shadow.
- `gif` or `image/gif` → animated inline, full-width image bubble.
- `image/*` → inline thumbnail.
- `video/*` → `<video controls>` with poster/metadata preload.
- Anything else → the existing file-card bubble.

### Wire-level backwards compatibility

`FileEnvelope.category` is `Option<String>` with `#[serde(default)]`
and the wire name `cat`. Old peers that don't know about `cat` keep
sending plain file envelopes, and old peers that receive a message
with `cat` just ignore the field. No migration needed.

### Security

- `send_sticker` / `send_gif` only accept paths that live inside the
  stickers/ or gifs/ subtree of the user's data_dir. `path_is_within`
  canonicalizes both ends and refuses symlink escape or `..`
  traversal — a hostile frontend cannot ask Rust to encrypt
  `C:\Windows\System32\...`.
- `read_media_bytes` is scoped to the data_dir OR the Downloads/
  Aquamarine folder, with a 20 MB soft cap. The UI only ever asks
  for thumbnails of files it discovered through `list_local_media`
  or decrypted into those folders via the existing download pipeline.
- Stickers and GIFs ride the same ChaCha20-Poly1305 + CID blob store
  as every other attachment. The server sees ciphertext only.

### What changed, file by file

- `aqua-core/src/file_transfer.rs` — `FileEnvelope.category` added
  (`#[serde(default, rename = "cat")]`); `encrypt_file_with_category()`
  helper, `encrypt_file()` is now a thin wrapper. 8 file-transfer
  tests + 37 total aqua-core tests still green.
- `aqua-core/src/lib.rs` — re-export the new helper.
- `aqua-client-ui/src/media.rs` (new) — scans `stickers/` and
  `gifs/` for packs and items, creates folders with READMEs on first
  boot, `path_is_within` traversal check. 3 unit tests.
- `aqua-client-ui/src/state.rs` — boots `media::ensure_dirs` early,
  `MessageKind::File` carries `category`, `preview_for()` emits a
  contextual icon (🎨 sticker, 🎞️ gif, 🖼️ image, 📹 video,
  📎 file).
- `aqua-client-ui/src/commands.rs` — new commands `send_sticker`,
  `send_gif`, `list_local_media`, `read_media_bytes`;
  `attach_file_impl` is the shared backend; own-sent file messages
  now set `saved_at = source_path` so the sender's inline preview
  renders immediately without round-tripping through decrypt.
- `aqua-client-ui/src/lib.rs` — register the four new commands and
  declare the `media` module.
- `aqua-client-ui/Cargo.toml` — `tempfile` dev-dep for the media
  tests.
- `ui/src/lib/types.ts` — `MessageKind.file.content.category`;
  `ChatSummary.live`; `LocalMediaList`, `StickerPack`, `StickerItem`,
  `GifItem`.
- `ui/src/lib/tauri.ts` — four new `api.*` wrappers + `bytesToDataUrl`
  helper for inline previews.
- `ui/src/lib/components/MediaPicker.svelte` (new) — module-scoped
  `openMediaPicker(peer)` store, tabs for Stickers / GIFs, pack
  rail, grid of lazy-loaded thumbs, send on click.
- `ui/src/lib/components/ChatWindow.svelte` — sticker button in
  composer, `renderKind()` dispatch for inline media, auto-download
  effect for sticker/gif/small attachments, CSS for sticker + media
  bubbles.
- `ui/src/App.svelte` — mount `<MediaPicker />` alongside the other
  modals.
- `ui/src/lib/icons.ts` — new `sticker` icon.

### Build / verification

- `cargo check` + `cargo test -p aqua-client-ui`: all green, 3 new
  media tests pass.
- `npm run build` under `ui/`: 134 modules, 82 KB bundle, clean (one
  pre-existing a11y warning on `InputPrompt.autofocus` is not from
  this session).

### Where to resume

- The Rust side auto-creates `stickers/` and `gifs/` folders in the
  data dir on first boot and writes a README into each. The user
  drops files in and they appear next time the picker opens.
- A Telegram-import flow (fetch via a bot and drop into the local
  stickers/<pack>/ folder) is not implemented yet — the folders are
  ready to receive it. A separate `aqua-sticker-import` tool, or a
  Rust command that fetches a tg sticker pack and unpacks it, is the
  natural next step.
- Server-side storage of blobs is unchanged; stickers and GIFs are
  just regular encrypted file attachments to the relay.

---

## 2026-10-01 — Session 6: OS keyring for the vault master key

### Headline

The `master.key` plaintext file alongside `vault.sqlite` is gone. The
vault's 32-byte wrapping key now lives in the OS secret store
(Windows Credential Manager / macOS Keychain / Linux Secret Service),
so cloning the vault directory to another machine no longer gives up
the decryption key.

### Behaviour

Boot order (idempotent, works on fresh installs, upgrades, and
headless environments):

1. **Try keyring first.** Entry service `app.aquamarine.desktop`,
   username `vault-master-key`, value = hex of the 32 bytes. If
   present and well-formed, use it; if a stale `master.key` file
   happens to still be on disk, delete it.
2. **Migrate from the legacy file.** If the keyring is empty but a
   `master.key` file exists (installs from Session 2 onward), read
   the file, write the key into the keyring, delete the file. One
   log line: `migrated vault master key from file to OS keyring`.
3. **Fresh install.** Generate 32 random bytes, store in keyring;
   on keyring failure (headless Linux dev env, no DBus Secret
   Service, policy-locked Windows session) fall back to a
   `master.key` file next to the vault with a warning.

### Code change

* `aqua-client-ui/Cargo.toml` — added `keyring = "3"` with
  `apple-native`, `windows-native`, `sync-secret-service` features.
  Full native backends on every supported desktop OS; no Python
  bridge, no network calls.
* `aqua-client-ui/src/state.rs` — `load_or_create_master_key()`
  rewritten in-place, keeping the same signature so no caller needed
  touching. New helper `decode_key()` sanity-checks hex input.

### Verification

`cargo test --workspace` → 57/57 green (no regressions; keyring is
only used at startup inside Tauri, not in any library tests).

Can't exercise the keyring in this Linux sandbox (no DBus to Secret
Service), so the file-fallback path is what gets hit here. On
Windows the first boot of a vault that already has a `master.key`
will migrate it the next time the app starts; subsequent boots use
Credential Manager alone and the file will be gone.

### Status of the deferred list

Remaining open from the plan: **multi-device** (needs a device-id
per bundle, sender-side fanout, QR pairing — days of work), and
**WebRTC voice/video** (weeks). Everything else from the original
MVP scope is in.

---

## 2026-10-01 — Session 5: file transfer end-to-end

### Headline

Files now travel through the same E2EE pipeline as text messages, with
a round-trip proven at the crypto layer: 50-MiB-cap-per-file sanity cap,
per-file ChaCha20-Poly1305 key, SHA-256 content addressing on the
ciphertext, no plaintext anywhere on the server. The UI composer's
paperclip opens the OS file picker, encrypted blobs upload to
`POST /v1/blob/:cid`, the recipient sees a file card and clicks
"Download & decrypt" to save a verified copy to their Downloads folder.
57/57 workspace tests still green (29 → 37 in aqua-core, +8 new file
transfer tests).

### New: `aqua-core::file_transfer`

* `encrypt_file(plaintext, filename, mime) → (FileEnvelope, Vec<u8>)` —
  mint fresh 32-byte key + 12-byte nonce, encrypt with
  ChaCha20-Poly1305 (AAD = filename bytes, so a swapped name invalidates
  the tag), SHA-256 the ciphertext → that's the CID.
* `decrypt_file(envelope, ciphertext)` — verify SHA-256 matches the
  envelope's CID *before* AEAD, then decrypt.
* `FileEnvelope::to_message_text() / from_message_text()` — JSON wrapped
  by `AQUA-FILE:v1:` prefix; sent as the usual E2EE text payload
  through an existing `Session`, so there's no new wire type on the
  session itself.
* 8 unit tests covering roundtrip, fresh key per file, CID mismatch,
  tampered filename AAD, text roundtrip, prefix-not-present detection,
  malformed JSON rejection.

### Server

* New `POST /v1/blob/:cid` (bearer-auth required) and
  `GET /v1/blob/:cid` endpoints. Server verifies the body SHA-256
  matches the URL CID on upload, rejects mismatches, stores the
  ciphertext in a `DashMap<cid, Bytes>`. Content-addressed so dupes
  dedupe automatically. 64 MiB body cap via `DefaultBodyLimit`.
* Fetch is unauthenticated — the content is encrypted and useless
  without the per-file key, which never leaves the sender's device
  except through the already-E2EE session.

### Client core

* `Transport::upload_blob(cid, bytes)` and `download_blob(cid)` on the
  HTTP side.

### Client UI (Tauri + Svelte)

* **Backend commands:** `attach_file(peer, path)` reads the file,
  encrypts, uploads, sends the offer through the live Session, pushes
  a `MessageKind::File` into the local chat store.
  `download_file(peer, message_id)` extracts envelope, downloads,
  decrypts, writes to `%USERPROFILE%\Downloads\Aquamarine\` picking a
  unique filename on conflict.
* **`MessageKind::File` variant** on both sides; `classify_kind()`
  centralises offer parsing so `send_message` / incoming envelope
  decrypt both automatically turn `AQUA-FILE:v1:...` into file cards.
* **File card UI:** inline avatar-style file icon, name, size + mime,
  "Download & decrypt" button on the receiving side, "Saved locally"
  state once downloaded (keeps the local path in the message so a
  download isn't re-triggered).
* **File picker:** paperclip in the composer opens `tauri-plugin-dialog`
  → path handed to `attach_file`.
* **Chat preview** now shows `📎 filename.ext` instead of the raw
  offer JSON.
* Added `tauri-plugin-dialog`, `@tauri-apps/plugin-dialog`, and `dirs`
  deps; capability file updated with `dialog:default` /
  `dialog:allow-open`.

### Windows build (`.exe`) — second attempt

Cross-compile from the Linux sandbox still blocked by policy:
`static.rust-lang.org` returns `403 CONNECT` from the egress gateway
(verified in the proxy's `recentRelayFailures` log). The proxy README
explicitly says to not retry or route around 403s. So the two binaries
have to be produced on Windows.

Two paths shipped in the repo to make that painless:

* **`BUILD_WINDOWS.md`** — step-by-step for building both exes locally
  on Windows in 5–8 minutes.
* **`.github/workflows/windows-build.yml`** — ready-made CI pipeline:
  push to `main` builds both exes on a `windows-latest` runner and
  uploads them as workflow artifacts; tagging `v*` additionally
  attaches them to the GitHub Release. Wire this up once you add the
  git remote.

### Test coverage (57 tests, all green)

| Crate                | Unit | Integration | Total |
| :------------------- | ---: | ----------: | ----: |
| `aqua-core`          |   37 |           0 |    37 |
| `aqua-client-core`   |   12 |           0 |    12 |
| `aqua-server`        |    5 |           3 |     8 |
| **workspace**        | **54** | **3**    |  **57** |

---

## 2026-09-30 — Session 4: live transport wired to UI

### Headline

The mock chats are gone. The desktop app now talks to a real
`aqua-server` over the auth-challenge + WebSocket protocol from the
earlier session, decrypts incoming messages through the vault-persisted
`Session`, and emits Tauri events the Svelte frontend listens to. Two
independent app instances pointed at the same server can now exchange
end-to-end-encrypted messages, with keys never leaving either device.
49/49 workspace tests still green.

### Wire-type consolidation

Moved `BundleUpload`, `RelayMessage`, `PayloadKind` plus the auth
request/response types out of `aqua-server` into `aqua_core::wire`.
Both server and client now import from the same place; the client no
longer needs to duplicate structs.

### `aqua-client-core::transport`

New async transport module. Handles:

* Sign-in: `POST /v1/auth/challenge` → sign nonce → `POST /v1/auth/verify` → bearer token.
* WebSocket lifecycle: `GET /v1/ws?user=X&token=Y`, split into read + write tasks.
* Bundle publish (`upload_bundle`) and fetch (`fetch_bundle`).
* Envelope relay (`send_initial`, `send_envelope`), non-blocking, backed by an mpsc.
* Incoming stream: pre-decoded `Incoming { Initial | Envelope | Closed | Error }` events on an mpsc receiver.

The transport never touches crypto — decryption happens in the caller
against the vault-backed `Session` per peer.

### Tauri backend rewire (`aqua-client-ui`)

* `state::AppState` now carries `transport: Option<Arc<Transport>>` and
  `sessions: HashMap<String, Session>` (rehydrated from the vault on
  boot).
* New commands:
  * `connect_to_server(server_url, username)` — signs in, uploads the
    current SPK + all OPK publics, spawns the incoming reader task.
  * `start_chat_with(peer_username, first_message)` — fetches the
    peer's bundle, runs `Session::start_as_initiator`, sends the
    initial message, persists the session.
* `send_message` now encrypts through the live `Session` (when the
  chat is `live`) and pushes to the WebSocket. Mock chats still work
  locally.
* Incoming events emitted to the frontend: `aqua://incoming`,
  `aqua://chats_updated`, `aqua://disconnected`, `aqua://transport_error`.
* `IdentityKeyPair` now derives `Clone` so background tasks can hold a
  copy without keeping a lock on the app state.
* New `Vault::list_one_time_prekey_publics()` to peek OPK publics for
  bundle uploads without consuming them.

### Frontend

* New generic `InputPrompt.svelte` — one small modal used for both the
  "Connect" and "Start chat" flows.
* `NavRail` gains a plug icon; clicking it opens the connect dialog
  (server URL + username). It glows green with an online dot when
  connected.
* The "+" button in the chat list now opens the start-chat dialog.
* `stores.ts` subscribes to the four Tauri events and reactively
  updates the chat list / messages / info.
* On the very first launch, the chat list shows a single "Welcome"
  card explaining how to connect and start a chat.

### How to run the live version

Two terminals on the machine hosting the server:

```
cargo run -p aqua-server                  # terminal 1: server on :7777
cd crates/aqua-client-ui && cargo tauri dev   # terminal 2: first client
```

On a second machine (or a second desktop session), do the same second
step; in both apps click the plug icon at the bottom-left of the nav
rail, enter `http://<server-host>:7777` and a distinct username. Once
both apps show a green online dot, hit "+" in the chat list of one and
type the other's username plus a first message. The message appears
on the other side, decrypted, immediately.

### What's still deferred

* **Windows .exe cross-compile from this Linux sandbox** — attempted,
  blocked: the proxy refuses `static.rust-lang.org`, so the
  `x86_64-pc-windows-gnu` Rust target won't install here. Building
  under Windows locally with `cargo tauri build` still works — that's
  how the release binary should be produced anyway.
* **File transfer.** The transport handles arbitrary opaque payloads
  already, so this is mostly protocol design (chunking, per-file KEK,
  manifest signing) plus a `POST/GET /v1/blob/:cid` on the server. Not
  in this session.
* **Multi-device.** Requires promoting the server's `bindings` map
  from `user_id → identity_dh_pub` to `user_id → Vec<(device_id,
  identity_dh_pub)>`, plus sender-side fanout across peer devices,
  plus a QR pairing flow. Deferred.
* **OS keyring for the vault master key.** Still lives plain in
  `master.key` alongside the vault. `keyring` crate integration is
  routine.

---

## 2026-09-30 — Session 3: Tauri 2 desktop shell

### Headline

The desktop app compiles end-to-end. `cargo tauri dev` from
`crates/aqua-client-ui/` opens a real window with the ported UI, backed
by real `aqua-client-core` vault persistence (identity + prekeys on
disk, ChaCha20-encrypted). Networking is still stubbed with a mock chat
store — the wire will land next.

### Layout

`crates/aqua-client-ui/` is a standalone Tauri 2 project (deliberately
`exclude`'d from the Cargo workspace to keep `cargo build --workspace`
independent from the Tauri build.rs and its frontend prerequisites).

```
crates/aqua-client-ui/
├── Cargo.toml            # aqua-client-ui = Rust backend crate
├── build.rs
├── tauri.conf.json       # dev URL: http://localhost:5173, dist: ui/dist
├── capabilities/default.json
├── icons/                # placeholder transparent PNGs
├── src/
│   ├── main.rs
│   ├── lib.rs            # tauri::Builder, invoke_handler wiring
│   ├── state.rs          # AppState + mock chat seed
│   └── commands.rs       # #[tauri::command] handlers
└── ui/                   # Svelte + Vite + TypeScript
    ├── package.json
    ├── vite.config.ts
    ├── svelte.config.js
    ├── tsconfig{,.node}.json
    ├── index.html
    └── src/
        ├── main.ts        # svelte 5 mount()
        ├── app.css        # design tokens (dark commitment)
        ├── App.svelte
        └── lib/
            ├── types.ts       # mirrors Rust wire types
            ├── tauri.ts       # `invoke` wrappers
            ├── stores.ts      # svelte stores + actions
            ├── icons.ts       # inline SVGs
            └── components/
                ├── NavRail.svelte
                ├── ChatList.svelte
                ├── ChatWindow.svelte
                ├── FingerprintModal.svelte
                └── Toast.svelte
```

### What actually works right now

* App launches with **real identity+vault on disk** — first run
  generates identity, saves it, generates SPK + 50 OPKs. Second run
  loads them back (via `aqua-client-core::Vault` + `PrekeyManager`).
* UI is a straight port of the HTML prototype: rail + chat list +
  chat pane, ambient orbs, aquamarine neon accents, verified badges,
  code-block bubbles, message grouping.
* Working interactions: chat switch, search filter, per-chat draft
  persisted in `localStorage`, send-message (locally appends to mock
  chat store; backend bumps the chat to top of the list), verify
  fingerprint (opens modal with word + hex fingerprint derived from
  the local identity's DH public), copy session fingerprint to
  clipboard, `⌘K`/`Ctrl+K` focuses search.
* Session-info footer shows real values from `PrekeyManager`:
  `OPK N available` and `SPK rotates in ?d`.
* Tauri commands: `init_app`, `list_chats`, `get_messages`,
  `send_message`, `verify_fingerprint`, `session_fingerprint`.
* Frontend build: `npm run build` produces 20 kB gzipped CSS + 22 kB
  gzipped JS. Svelte 5 with runes.

### What's stubbed (next session)

* Sending doesn't go over the network yet — the "Roma" chat is a
  scripted mock. Wiring: replace the mock chat store with a
  `Session`-per-peer map, a WS client that connects to `aqua-server`,
  and Tauri events pushed to the frontend on incoming envelopes.
* No onboarding UX — identity is auto-created on first launch. Real
  flow: pick a username, POST auth challenge/verify, upload bundle.
* No key backup / recovery. Master key is stored plain next to the
  vault (`master.key`). Next: OS keyring (`keyring` crate) so the DB
  stays encrypted at rest even if the file is copied elsewhere.

### How to run

```
cd crates/aqua-client-ui/ui && npm install && cd ..
cargo tauri dev
```

Requires: Rust stable, Node.js 22+, `cargo install tauri-cli --version '^2'`,
plus Windows WebView2 (pre-installed on Win10/11) or Linux GTK/WebKit dev
packages (on Ubuntu 24.04: `apt install libgtk-3-dev
libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev`).

---

## 2026-09-30 — Session 2: persistence, prekey rotation, server auth

### Headline

Sessions now survive restart, prekey material rotates on schedule, and
the server rejects any request that isn't a signed challenge from the
identity it claims to belong to. **49 tests, 0 failures** across the
whole workspace.

### New in `aqua-core`

* All secret types (`DhSecret`, `SignSecret`, `IdentityKeyPair`,
  `SignedPreKey`, `OneTimePreKey`) plus `DoubleRatchet` and `Session`
  now derive/impl `serde::{Serialize, Deserialize}`. These are
  intentionally only used inside `aqua-client-core::vault` for
  encrypted-at-rest storage; the impls are documented as
  never-send-over-the-wire.

### New crate: `aqua-client-core::vault`

Encrypted-at-rest SQLite store. Every row is
`(nonce, ChaCha20-Poly1305(master_key, nonce, bincode(record)))` with
a per-record AAD (record type + id). AAD prefix `Aquamarine/Vault/v1:`
means bumping the version invalidates all older rows.

* `Vault::open(path, MasterKey)` / `open_in_memory` (for tests)
* Handles identity, sessions (per peer), signed prekey history,
  one-time prekey pool.
* `take_one_time_prekey(id)` is atomic — an OPK cannot be handed out
  twice even under concurrent requests.
* 6 tests including the critical one: **Bob's session is saved, the
  vault is closed and reopened, and Bob keeps decrypting fresh
  messages from Alice with the DH ratchet still working correctly.**
  Also: wrong master key fails, tampered ciphertext fails.

### New: `aqua-client-core::prekeys`

Rotation policy layer over the vault. Pure logic; no network I/O.

* `SPK_ROTATION_PERIOD = 7 days`, `OPK_MIN_POOL = 10`, `OPK_TARGET = 50`.
* `PrekeyManager::check_and_refresh(now)` returns a `RefreshOutcome`
  describing what changed; the transport layer publishes it.
* SPK rotates at the 7-day boundary. Explicitly does NOT rotate when
  the clock goes backwards (guards against a wrong-clock machine
  burning prekeys).
* OPK pool refills to `OPK_TARGET` when it drops below `OPK_MIN_POOL`,
  and new ids never collide with what's still in the pool.
* 6 tests covering all of the above.

### New: `aqua-server::auth`

Signed-challenge authentication using each identity's Ed25519 key.

* `POST /v1/auth/challenge`  — issues a 24-byte random nonce bound to
  the caller's DH public.
* `POST /v1/auth/verify`     — verifies the signature, consumes the
  nonce, mints a bearer token.
* Bearer token = `[dh_pub][expires_at][HMAC-SHA256(server_key,
  dh_pub || expires_at)]`, base64url-encoded. Stateless verification;
  server key rotation invalidates every outstanding token in one step.
* `POST /v1/bundle/:user` now requires `Authorization: Bearer <token>`.
  First upload claims the username for the identity in the token;
  subsequent uploads under that name must come from the same identity.
* WebSocket upgrade at `GET /v1/ws?user=X&token=Y` requires a valid
  token AND that the token identity matches the one bound to `X`.
* `GET /v1/bundle/:user` remains public (contacts need to fetch it to
  message you).
* 5 unit tests for token minting / tampering / cross-server rejection /
  nonce consumption, plus 3 integration tests (full authenticated E2E,
  unauthenticated upload refused, bad signature refused).

### Test coverage summary (49 tests, all green)

| Crate                | Unit | Integration | Total |
| :------------------- | ---: | ----------: | ----: |
| `aqua-core`          |   29 |           0 |    29 |
| `aqua-client-core`   |   12 |           0 |    12 |
| `aqua-server`        |    5 |           3 |     8 |
| **workspace**        | **46** | **3**    |  **49** |

---

## 2026-09-30 — Session 1b: UI prototype

### Headline

Interactive UI prototype landed at `ui/preview.html` — self-contained
single-file HTML, opens directly in any browser, no build step. Purpose:
lock the visual direction and interaction model before spending a day
porting to Svelte and wiring Tauri.

### Design direction

* **Aesthetic:** Discord's neon + Telegram's information architecture.
  Dark commitment (no light theme yet — the neon palette needs the void
  behind it).
* **Palette:** aquamarine `#00E5C7` as the single loud accent, violet
  `#7B5CFF` as the crypto secondary tone, layered `#06070C → #171C28`
  greys with a slight cool bias. Two ambient orbs breathing in the void.
* **Type:** Onest (300–800) for UI, JetBrains Mono for keys and code.
  Deliberately not Inter / Space Grotesk.
* **Layout:** three-column shell — 68px nav rail, 340px chat list,
  active chat. Collapses to single column below 720px with a back
  button.

### Working in the prototype

* Click chats to switch (mock data — Nika typing, Roma active, Devlog
  channel-style, Zoya/Mira/Alex).
* Search filters the chat list in real time.
* Composer sends messages: `Enter` sends, `Shift+Enter` newline. Sent
  messages appear in the active chat, bump it to the top, save/restore
  a per-chat draft in `localStorage`.
* Click the verified shield on Roma → fingerprint-verification modal
  with a 5×2 word list + hex SHA-256.
* Session fingerprint at the bottom-left has a copy-to-clipboard.
* Ambient E2EE indicators everywhere: header lock, per-message lock in
  previews, "X3DH · Double Ratchet · your keys never leave this device"
  under the composer.
* `⌘K` / `Ctrl+K` focuses search.

### What's next for the UI

Once the direction is locked (or after any redesign), port the prototype
into `crates/aqua-client-ui/` as a Vite + Svelte + TypeScript project,
then wrap it in Tauri 2 with `aqua-client-core` behind Tauri commands.
That gives us the real desktop app.

---

## 2026-09-30 — Session 1: crypto core, signaling server, end-to-end test

### Headline

The full text-messaging path works end-to-end. Two clients register
prekey bundles over HTTP, connect via WebSocket, and exchange
Double-Ratchet-encrypted messages through the relay server. 30/30 tests
green (29 unit + 1 integration).

### What exists now

* Cargo workspace at repo root with three crates:
  * **`crates/aqua-core`** — protocol / crypto library. 29 unit tests.
  * **`crates/aqua-server`** — `axum` HTTP + WebSocket signaling
    server, in-memory state. Now both a library and a binary so tests
    can boot it in-process. One integration test covering the full
    flow.
  * **`crates/aqua-client-core`** — stub that re-exports `aqua-core`.
    Will grow into the transport / session-store layer.

### `aqua-core` module map

| Module     | What it holds                                                              |
| ---------- | -------------------------------------------------------------------------- |
| `crypto`   | Wrappers around X25519, Ed25519, ChaCha20-Poly1305, HKDF, HMAC-SHA256.     |
| `keys`     | `IdentityKeyPair`, `SignedPreKey`, `OneTimePreKey`, `PreKeyBundle`.        |
| `x3dh`     | Initiator & responder halves of the X3DH agreement, plus AD helper.       |
| `ratchet`  | Double Ratchet: root ratchet, chain ratchet, DH ratchet, skipped-key map. |
| `wire`     | `InitialMessage` and `Envelope` (over-the-wire types).                    |
| `session`  | High-level `Session::start_as_initiator` / `accept_initial` / encrypt / decrypt. |

### Design decisions worth remembering

* **Own protocol, audited primitives.** We wrote our own X3DH + Double
  Ratchet gluing together `x25519-dalek`, `ed25519-dalek`,
  `chacha20poly1305`, `hkdf`. Not wire-compatible with Signal or Matrix,
  but small enough to audit in an afternoon. Swap to `vodozemac` later
  if we need Matrix interop.
* **Two-key identity.** `IdentityKeyPair` = separate Ed25519 signing key
  + separate X25519 DH key. Simpler than Signal's XEdDSA, costs 32 extra
  bytes on the wire per identity.
* **KDF constants.**
  * X3DH info: `"Aquamarine/X3DH v1"`, `F = 32×0xFF`.
  * Root KDF info: `"Aquamarine/RootKDF v1"`.
  * Message key KDF info: `"Aquamarine/MsgKey v1"`.
  * Chain KDF bytes: `0x01` → message key, `0x02` → next chain key.
  * `MAX_SKIP = 1000`.
* **`Session` is the public API.** Application code should not need to
  touch `x3dh` or `ratchet` directly.

### Server API (current)

* `POST /v1/bundle/:user` — upload identity + SPK + one-time prekeys.
  SPK signature is verified before storing.
* `GET  /v1/bundle/:user` — return a bundle; pops one OPK off the queue.
* `WS   /v1/ws?user=:id`  — bidirectional envelope relay. Queues messages
  for offline recipients. Server treats payloads as opaque bytes.
* No authentication yet. A real deployment must gate uploads and WS
  connects behind a signed challenge from the identity key.

### Test coverage (all passing)

**aqua-core (29 unit tests):**

* `crypto`: X25519 agree/disagree, Ed25519 sign+verify+reject, HKDF
  determinism, AEAD roundtrip + tamper + wrong-AAD, HMAC determinism.
* `keys`: SPK verify + wrong identity, bundle verify + tamper, bundle
  bincode roundtrip.
* `x3dh`: initiator/responder agree with and without OPK, different
  sessions yield different keys, tampered bundle rejected, AD layout.
* `ratchet`: simple send/recv, 10-turn ping-pong, out-of-order within a
  chain, out-of-order across DH ratchet steps, tampered ciphertext,
  replay, wrong AD, MAX_SKIP enforced.
* `session`: 10-turn end-to-end conversation, wrong-OPK detection,
  envelope serde roundtrip.

**aqua-server (1 integration test, `tests/e2e.rs`):**

* Boots the real server on `127.0.0.1:0`.
* Alice + Bob register bundles (3 OPKs each) over HTTP.
* Both connect via `tokio-tungstenite` as real WebSocket clients.
* Alice fetches Bob's bundle, starts a session, sends the initial
  X3DH-carrying message through the relay.
* Bob decrypts and confirms plaintext.
* 5 rounds of ping-pong (10 Double-Ratchet-encrypted messages) succeed.

---

## What's next (in priority order)

1. **On-disk session persistence** (`aqua-client-core`). Ratchet state
   must survive app restart. Design: SQLite via `rusqlite`, session
   blob wrapped by a KEK from the OS keystore (`keyring` crate), and
   the whole database ChaCha20-encrypted at rest.
2. **CLI client** (`aqua-cli`, new crate). Two terminals, two users, a
   real chat over the local server. Proof-of-life before touching UI.
3. **Prekey rotation policy** in `aqua-client-core`: rotate SPK weekly,
   refill OPKs when server-side stock drops below N.
4. **Auth on the server.** Signed challenge from identity key over a
   random nonce; short-lived bearer token for subsequent HTTP/WS
   calls. Also make WS remember which `mpsc::Sender` belongs to which
   connection so we don't blast messages to stale devices.
5. **Tauri 2 desktop shell.** Wire `aqua-client-core` behind Tauri
   commands. Minimal UI: contact list + one chat window.
6. **Multi-device.** Each device registers its own bundle under the same
   account. Sender fan-outs one envelope per recipient device.
7. **File transfer.** Content-addressed chunks, each chunk AEAD-encrypted
   with a per-file key derived from the recipient's session.
8. **WebRTC DataChannel P2P fallback / voice-video.** After the WS path
   is solid. This is many weeks of work; a partial dependency graph:
   `webrtc-rs` for the stack, ICE/STUN/TURN infra to run, signaling
   messages piggybacking on the same WebSocket relay.

## Deliberate non-goals for now

* No real authentication: any HTTP client can upload a bundle under any
  username, and any WS client can claim any username. Do NOT expose the
  server on a public interface until (4) above lands.
* No persistence: server state is `HashMap`s in RAM, and clients
  regenerate identities on restart. Restarting anything wipes the world.
* No wire compatibility with Signal / Matrix / anything else.
* No transport hardening (DPI resistance, pluggable transports). Out of
  scope for the MVP.

## Resume checklist

When picking this back up:

1. `cargo test -p aqua-core` — must be green.
2. `cargo build --workspace` — must be green.
3. Read this file top-down to skip re-reading module bodies.
4. Prefer growing `aqua-client-core` and adding integration tests over
   touching `aqua-core` internals — the crypto layer is intentionally
   stable.
