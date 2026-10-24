import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * Turn a local file path into a URL the webview can load directly through
 * Tauri's asset protocol — streamed from disk, no IPC and no base64. This
 * is the fast path for inline media (images, video, audio, stickers);
 * `read_media_bytes` stays as a fallback for odd cases.
 */
/** One relay in the sign-in server picker (servers.rs). */
export interface ServerProbe {
  url: string; host: string; port: number; ip: string | null;
  scope: "loopback" | "lan" | "emulator" | "public" | null;
  ok: boolean; ms: number | null; users: number | null; online: number | null;
  country: string | null; country_code: string | null; city: string | null;
}

/** One account on this device (accounts.rs); slot "" is the first one. */
export interface AccountSlot {
  slot: string; registered: boolean; username: string | null; server_url: string | null; active: boolean;
}

export function assetUrl(path: string | null | undefined): string {
  if (!path) return "";
  try { return convertFileSrc(path); } catch { return ""; }
}
import type {
  ArticleMedia,
  AttachResult,
  BotInfo,
  ChatMessage,
  ChatSummary,
  DirProfile,
  DownloadResult,
  FingerprintView,
  Group,
  InitInfo,
  LocalMediaList,
  Activity,
  SendResult,
  StickerImportReport,
  StickerPack,
} from "./types";

export interface LinkPreview {
  url: string;
  kind: "page" | "image" | "video" | "youtube" | "telegraph";
  site: string | null;
  title: string | null;
  description: string | null;
  image_path: string | null;
  video_url: string | null;
  video_mime: string | null;
  video_size: number | null;
  youtube_id: string | null;
  telegraph_path: string | null;
}
export interface CachedMedia { path: string; mime: string; size: number }
export interface TelegraphPage {
  path: string;
  url: string;
  title: string;
  description?: string;
  author_name?: string;
  author_url?: string;
  image_url?: string;
  views?: number;
  content?: import("./markup").TgNode[];
}

export interface IncomingPayload {
  peer_id: string;
  text: string;
  time: string;
  is_new_chat: boolean;
}

export interface GpuStatus { mode: "auto" | "on" | "off"; software: boolean; reason: string; dmabuf?: boolean; dmabuf_pending?: boolean }

/** One device of my account (Settings → Devices). */
export interface DeviceInfo {
  addr: string; id: string; primary: boolean; name: string; added: number; online: boolean; current: boolean;
}

export const api = {
  initApp:            (): Promise<InitInfo>                                        => invoke("init_app"),
  completeRegistration: (username: string, email: string | null): Promise<InitInfo> => invoke("complete_registration", { username, email }),
  logOut:             (): Promise<InitInfo>                                        => invoke("log_out"),
  listChats:          (): Promise<ChatSummary[]>                                   => invoke("list_chats"),
  getMessages:        (peer_id: string): Promise<ChatMessage[]>                    => invoke("get_messages", { peerId: peer_id }),
  sendMessage:        (peer_id: string, text: string): Promise<SendResult>        => invoke("send_message", { peerId: peer_id, text }),
  verifyFingerprint:  (peer_id: string): Promise<FingerprintView>                  => invoke("verify_fingerprint", { peerId: peer_id }),
  sessionFingerprint: (): Promise<string>                                          => invoke("session_fingerprint"),
  connectToServer:    (server_url: string, username: string): Promise<InitInfo>    => invoke("connect_to_server", { serverUrl: server_url, username }),
  setNetworkProxy:    (url: string): Promise<InitInfo>                             => invoke("set_network_proxy", { url }),

  // Email registration + user directory
  /** purpose: the relay answers at once if the e-mail already has an account (signup) or has none (signin/reset). */
  requestEmailCode:   (server_url: string, email: string, purpose?: "signup" | "signin" | "reset", nonce?: string): Promise<{ dev_code: string | null; delivery_failed: boolean; error: string | null }> => invoke("request_email_code", { serverUrl: server_url, email, purpose: purpose ?? null, nonce: nonce ?? null }),
  pollEmailCode:      (server_url: string, email: string, nonce: string): Promise<string | null> => invoke("poll_email_code", { serverUrl: server_url, email, nonce }),
  verifyEmailCode:    (server_url: string, email: string, code: string): Promise<string> => invoke("verify_email_code", { serverUrl: server_url, email, code }),
  registerAccount:    (server_url: string, username: string, ticket: string, display_name: string, about: string, phone: string, password?: string, email?: string): Promise<void> => invoke("register_account", { serverUrl: server_url, username, ticket, displayName: display_name, about, phone, password: password || null, email: email || null }),
  listDevices:        (): Promise<DeviceInfo[]> => invoke("list_devices"),
  removeDevice:       (id: string): Promise<void> => invoke("remove_device", { id }),
  renameDevice:       (id: string, name: string): Promise<void> => invoke("rename_device", { id, name }),
  loginAccount:       (server_url: string, ticket: string, password: string, email?: string): Promise<string> => invoke("login_account", { serverUrl: server_url, ticket, password, email: email || null }),
  fedPeers:           (server_url: string): Promise<{ federated: boolean; self?: string; servers: string[] }> => invoke("fed_peers", { serverUrl: server_url }),
  probeServers:       (urls: string[]): Promise<ServerProbe[]>                  => invoke("probe_servers", { urls }),
  publicServers:      (): Promise<{ url: string; name: string | null; note: string | null }[]> => invoke("public_servers"),
  scanLanRelays:      (port?: number): Promise<string[]>                        => invoke("scan_lan_relays", { port: port ?? null }),
  backupStatus:       (): Promise<BackupStatus> => invoke("backup_status"),
  backupEnable:       (password: string): Promise<string> => invoke("backup_enable", { password }),
  backupPhrase:       (): Promise<string | null> => invoke("backup_phrase"),
  /** Auto-delete timer of a chat in seconds (0 = off). */
  /** Send to a bot without showing it in the chat (button press / mini app data). */
  botSendRaw:         (peer_id: string, text: string): Promise<void> => invoke("bot_send_raw", { peerId: peer_id, text }),
  /** Signed initData for a bot's mini app. */
  webappInit:         (bot: string, start_param?: string): Promise<{ init_data: string; signed: boolean }> => invoke("webapp_init", { bot, startParam: start_param ?? null }),
  /** The bot's menu button. */
  webappMenu:         (bot: string): Promise<{ type: string; text?: string; web_app?: { url: string } }> => invoke("webapp_menu", { bot }),
  /** Save a text export (.ics/.md/.txt/.html/.csv) to a path the user picked. */
  writeTextFile:      (path: string, text: string): Promise<void> => invoke("write_text_file", { path, text }),
  /** Desktop: open a chat in its own window (or focus it). */
  openChatWindow:     (peer_id: string, title: string): Promise<void> => invoke("open_chat_window", { peerId: peer_id, title }),
  /** A [ctl:v1] control to a group's owner and admins only. */
  groupControlAdmins: (gid: string, body: string): Promise<void> => invoke("group_control_admins", { gid, body }),
  getChatTtl:         (peer_id: string): Promise<number> => invoke("get_chat_ttl", { peerId: peer_id }),
  /** Set a 1:1 chat's auto-delete timer (shared with the other person). */
  setChatTtl:         (peer_id: string, secs: number): Promise<void> => invoke("set_chat_ttl", { peerId: peer_id, secs }),
  /** Does this device know the 24-word key / does the relay have it? */
  recoveryStatus:     (): Promise<{ local: boolean; server: boolean | null }> => invoke("recovery_status"),
  /** Ensure a recovery key exists, register it on the relay, return the words. */
  recoveryCreate:     (): Promise<string> => invoke("recovery_create"),
  /** Forgotten password: e-mail ticket + 24 words (or "") → new password. */
  passwordReset:      (server_url: string, ticket: string, email: string, phrase: string, password: string): Promise<string> =>
    invoke("password_reset", { serverUrl: server_url, ticket, email, phrase, password }),
  backupNow:          (ui: string, force: boolean): Promise<BackupStatus> => invoke("backup_now", { ui, force }),
  backupDisable:      (): Promise<void> => invoke("backup_disable"),
  backupRestore:      (server_url: string, ticket: string, password: string, email: string, phrase?: string): Promise<{ found: boolean; need_phrase: boolean; username: string | null; email: string | null; ui: string }> =>
    invoke("backup_restore", { serverUrl: server_url, ticket, password, email, phrase: phrase || null }),
  registerUsername:   (ticket: string, username: string, display_name: string, about: string, phone: string): Promise<void> => invoke("register_username", { ticket, username, displayName: display_name, about, phone }),
  searchDirectory:    (query: string): Promise<DirProfile[]>                        => invoke("search_directory", { query }),
  fetchDirectoryProfile: (username: string): Promise<DirProfile | null>            => invoke("fetch_directory_profile", { username }),
  publishProfile:     (display_name: string, about: string, phone: string): Promise<void> => invoke("publish_profile", { displayName: display_name, about, phone }),
  publishProfileFull: (display_name: string, about: string, phone: string, extra: Record<string, unknown>): Promise<void> =>
    invoke("publish_profile_full", { displayName: display_name, about, phone, extra }),
  // Public avatar / banner (stored on the server, visible to everyone).
  setProfileImage:      (kind: "avatar" | "banner", path: string): Promise<{ ver: number; path: string }> => invoke("set_profile_image", { kind, path }),
  setProfileImageBytes: (kind: "avatar" | "banner", mime: string, bytes: number[]): Promise<{ ver: number; path: string }> => invoke("set_profile_image_bytes", { kind, mime, bytes }),
  clearProfileImage:    (kind: "avatar" | "banner"): Promise<void> => invoke("clear_profile_image", { kind }),
  clearProfileMedia:    (kind: "avatar" | "banner" | "music"): Promise<void> => invoke("clear_profile_image", { kind }),
  setProfileMediaFile:  (kind: "banner" | "music", path: string): Promise<{ ver: number; path: string }> => invoke("set_profile_media_file", { kind, path }),
  profileMediaPath:     (username: string, kind: "avatar" | "banner" | "music", ver: number): Promise<string | null> => invoke("profile_media_path", { username, kind, ver }),

  // Media library management
  addMediaFiles:      (target: "gifs" | "stickers" | "emoji", pack: string | null, paths: string[]): Promise<number> => invoke("add_media_files", { target, pack, paths }),
  deleteMediaItem:    (path: string): Promise<void> => invoke("delete_media_item", { path }),
  deleteStickerPack:  (title: string): Promise<void> => invoke("delete_sticker_pack", { title }),
  createStickerPack:  (title: string, kind: "sticker" | "emoji"): Promise<string> => invoke("create_sticker_pack", { title, kind }),
  packAddFiles:       (title: string, paths: string[]): Promise<string[]> => invoke("pack_add_files", { title, paths }),
  packSetOrder:       (title: string, files: string[]): Promise<void> => invoke("pack_set_order", { title, files }),
  packSetIcon:        (title: string, file: string | null): Promise<void> => invoke("pack_set_icon", { title, file }),
  stickerSetEmoji:    (path: string, emoji: string): Promise<string> => invoke("sticker_set_emoji", { path, emoji }),
  stickerDelete:      (path: string): Promise<void> => invoke("sticker_delete", { path }),
  syncTelegramPack:   (title: string): Promise<{ title: string; changed: boolean; added: number; removed: number }> => invoke("sync_telegram_pack", { title }),
  checkPackUpdates:   (): Promise<{ title: string; changed: boolean; added: number; removed: number }[]> => invoke("check_pack_updates"),
  renameStickerPack:  (title: string, new_title: string): Promise<void> => invoke("rename_sticker_pack", { title, newTitle: new_title }),

  // Rich presence (what you're playing / listening to / watching)
  setActivityDetection: (enabled: boolean): Promise<void> => invoke("set_activity_detection", { enabled }),
  currentActivity:      (): Promise<Activity[]> => invoke("current_activity"),

  // Bots
  startBotChat:       (bot_username: string, first_message: string): Promise<ChatSummary> => invoke("start_bot_chat", { botUsername: bot_username, firstMessage: first_message }),
  createBot:          (username: string, display_name: string): Promise<BotInfo>       => invoke("create_bot", { username, displayName: display_name }),
  listBots:           (): Promise<BotInfo[]>                                           => invoke("list_bots"),
  gamesSearch:        (q: string): Promise<{ id: string; name: string; has_icon: boolean }[]> => invoke("games_search", { q }),
  gameIcon:           (id: string): Promise<string | null>                              => invoke("game_icon", { id }),
  gameCover:          (id: string): Promise<string | null> => invoke("game_cover", { id }),
  saveWallpaper:      (name: string, bytes: number[]): Promise<string>                  => invoke("save_wallpaper", { name, bytes }),
  listWallpapers:     (): Promise<string[]>                                            => invoke("list_wallpapers"),
  deleteWallpaper:    (path: string): Promise<void>                                    => invoke("delete_wallpaper", { path }),
  saveTempFile:       (name: string, bytes: number[]): Promise<string>                  => invoke("save_temp_file", { name, bytes }),
  saveConsoleBot:     (path: string): Promise<void>                                    => invoke("save_console_bot", { path }),
  deleteBot:          (username: string): Promise<void>                                => invoke("delete_bot", { username }),
  startChatWith:      (peer_username: string, first_message: string): Promise<ChatSummary> => invoke("start_chat_with", { peerUsername: peer_username, firstMessage: first_message }),

  // Groups / supergroups / channels
  listGroups:         (): Promise<Group[]>                                             => invoke("list_groups"),
  createGroup:        (kind: string, title: string, description: string, members: string[]): Promise<Group> => invoke("create_group", { kind, title, description, members }),
  updateGroup:        (group: Group): Promise<Group>                                   => invoke("update_group", { group }),
  leaveGroup:         (gid: string): Promise<void>                                     => invoke("leave_group", { gid }),
  setPhotoPrivacy:    (everybody: boolean, allow: string[], deny: string[]): Promise<void> => invoke("set_photo_privacy", { everybody, allow, deny }),
  setLocalPrefs:      (keepDeleted: boolean, keepEdits: boolean, blocked: string[]): Promise<void> => invoke("set_local_prefs", { keepDeleted, keepEdits, blocked }),
  deleteGroupMessageOf: (gid: string, author: string, text: string): Promise<void>    => invoke("delete_group_message_of", { gid, author, text }),
  groupControlTo:     (gid: string, to: string, body: string): Promise<void>         => invoke("group_control_to", { gid, to, body }),
  groupControl:       (gid: string, body: string): Promise<void>                       => invoke("group_control", { gid, body }),
  groupSetMyTag:      (gid: string, tag: string): Promise<void>                        => invoke("group_set_my_tag", { gid, tag }),
  groupRequestJoin:   (gid: string, admin: string, title: string, note: string): Promise<void> => invoke("group_request_join", { gid, admin, title, note }),
  groupSend:          (gid: string, body: string): Promise<SendResult>                 => invoke("group_send", { gid, body }),
  setGroupRoom:       (gid: string, room: string): Promise<void>                      => invoke("set_group_room", { gid, room }),

  // Edit / delete (local store mutation; relay the control separately)
  editMessage:        (peer_id: string, old_text: string, new_text: string): Promise<void> => invoke("edit_message", { peerId: peer_id, oldText: old_text, newText: new_text }),
  deleteMessage:      (peer_id: string, text: string): Promise<void>                    => invoke("delete_message", { peerId: peer_id, text }),
  deleteMessagesLocal: (peerId: string, ids: string[]): Promise<number> => invoke("delete_messages_local", { peerId, ids }),
  deleteChat:         (peerId: string): Promise<void> => invoke("delete_chat", { peerId }),
  markChatRead:       (peerId: string): Promise<void> => invoke("mark_chat_read", { peerId }),
  attachFile:         (peer_id: string, file_path: string): Promise<AttachResult>  => invoke("attach_file", { peerId: peer_id, filePath: file_path }),
  /** Re-send a file with a "Forwarded from" header (JSON). */
  forwardFile:        (peer_id: string, file_path: string, header: string): Promise<AttachResult> => invoke("forward_file", { peerId: peer_id, filePath: file_path, header }),
  attachFileAs:       (peerId: string, filePath: string, mode: "file" | "media" | "compressed"): Promise<AttachResult> => invoke("attach_file_as", { peerId, filePath, mode }),
  attachBytes:        (peerId: string, name: string, mime: string, bytes: number[]): Promise<AttachResult> => invoke("attach_bytes", { peerId, name, mime, bytes }),
  downloadFile:       (peer_id: string, message_id: string): Promise<DownloadResult> => invoke("download_file", { peerId: peer_id, messageId: message_id }),
  sendCallSignal:     (peer_id: string, signal: object): Promise<void> =>
    invoke("send_call_signal", { peerId: peer_id, signalJson: JSON.stringify(signal) }),
  // Raw control payload over the E2EE session, never shown as a bubble
  // (poll votes, read receipts).
  sendControl:        (peer_id: string, payload: string): Promise<void> =>
    invoke("send_control", { peerId: peer_id, payload }),

  // Local media (stickers + GIFs)
  sendSticker:        (peer_id: string, path: string): Promise<AttachResult>       => invoke("send_sticker", { peerId: peer_id, filePath: path }),
  sendGif:            (peer_id: string, path: string): Promise<AttachResult>       => invoke("send_gif", { peerId: peer_id, filePath: path }),
  listLocalMedia:     (): Promise<LocalMediaList>                                  => invoke("list_local_media"),
  readMediaBytes:     (path: string): Promise<number[]>                            => invoke("read_media_bytes", { path }),
  readUserFont:       (path: string): Promise<number[]>                            => invoke("read_user_font", { path }),
  readUserImage:      (path: string): Promise<number[]>                            => invoke("read_user_image", { path }),
  // Open web: link previews, cached remote media, Telegraph pages, external links.
  linkPreview:        (url: string): Promise<LinkPreview>                           => invoke("link_preview", { url }),
  cacheRemoteMedia:   (url: string, max_mb?: number): Promise<CachedMedia>         => invoke("cache_remote_media", { url, maxMb: max_mb ?? null }),
  telegraphPage:      (path: string): Promise<TelegraphPage>                        => invoke("telegraph_page", { path }),
  openExternal:       (url: string): Promise<void>                                  => invoke("open_external", { url }),
  uploadAttachment:   (peer_id: string, file_path: string, keep?: boolean): Promise<ArticleMedia> => invoke("upload_attachment", { peerId: peer_id, filePath: file_path, keep: keep ?? null }),
  prepareBannerVideo: (path: string): Promise<string>                                  => invoke("prepare_banner_video", { path }),
  gpuStatus:          (): Promise<GpuStatus> => invoke("gpu_status"),
  setDmabuf:          (on: boolean): Promise<void> => invoke("set_dmabuf", { on }),
  confirmDmabuf:      (): Promise<void> => invoke("confirm_dmabuf"),
  restartApp:         (): Promise<void> => invoke("restart_app"),
  serverPing:         (url: string, via?: string): Promise<number> => invoke("server_ping", { url, via: via ?? null }),
  accountsList:       (): Promise<AccountSlot[]> => invoke("accounts_list"),
  accountSwitch:      (slot: string, store: string): Promise<{ slot: string; store: string | null }> => invoke("account_switch", { slot, store }),
  accountForget:      (slot: string): Promise<void> => invoke("account_forget", { slot }),
  appRelaunch:        (): Promise<void> => invoke("app_relaunch"),
  setGpuMode:         (mode: "auto" | "on" | "off"): Promise<void>                     => invoke("set_gpu_mode", { mode }),
  fetchAttachment:    (media: ArticleMedia): Promise<string>                         => invoke("fetch_attachment", { media }),
  allowMediaPreview:  (path: string): Promise<void>                                  => invoke("allow_media_preview", { path }),
  mediaStreamUrl:     (path: string): Promise<string>                                => invoke("media_stream_url", { path }),
  transcodeMedia:     (path: string): Promise<string>                                => invoke("transcode_media", { path }),
  mediaSupport:       (): Promise<{ blocking: string[]; recommended: string[] }>    => invoke("media_support"),
  listSounds:         (): Promise<{ id: string; name: string; path: string }[]>    => invoke("list_sounds"),
  saveSound:          (name: string, bytes: number[]): Promise<{ id: string; name: string; path: string }> => invoke("save_sound", { name, bytes }),
  deleteSound:        (id: string): Promise<void>                                  => invoke("delete_sound", { id }),
  writeThemeFile:     (path: string, text: string): Promise<void>                   => invoke("write_theme_file", { path, text }),
  readThemeFile:      (path: string): Promise<string>                               => invoke("read_theme_file", { path }),
  probeMediaFile:     (path: string, kind: "video" | "audio" | "size"): Promise<{ poster?: string | null; duration?: number | null; size: number; too_big: boolean }> => invoke("probe_media", { path, kind }),
  copyFileTo:         (source: string, dest: string): Promise<void>                => invoke("copy_file_to", { source, dest }),

  // Voice & video messages (recorded in-browser → bytes sent to Rust → encrypted via attach pipeline)
  sendVoiceMessage:   (peer_id: string, name: string, mime: string, bytes: number[]): Promise<AttachResult> => invoke("send_voice_message", { peerId: peer_id, name, mime, bytes }),
  sendCircleVideo:    (peer_id: string, name: string, mime: string, bytes: number[]): Promise<AttachResult> => invoke("send_circle_video",  { peerId: peer_id, name, mime, bytes }),
  sendEditedImage:    (peer_id: string, name: string, mime: string, bytes: number[]): Promise<AttachResult> => invoke("send_edited_image",  { peerId: peer_id, name, mime, bytes }),

  // Sticker editor: save a user-created sticker PNG into the local
  // stickers library at <data_dir>/stickers/<pack>/<name>.png.
  saveSticker:        (pack: string, name: string, bytes: number[]): Promise<string> => invoke("save_sticker",       { pack, name, bytes }),
  // Import a Telegram sticker pack (link / @name / short name) using a
  // Telegram bot token. Downloads straight from the Telegram Bot API into
  // the local stickers library; never touches our own relay.
  packPreview:        (pack_ref: string): Promise<{ ref: string; title: string; kind: string; installed: boolean; local: StickerPack | null; server: Record<string, unknown> | null }> => invoke("pack_preview", { packRef: pack_ref }),
  serverPacks:        (): Promise<{ telegram: boolean; packs: { name: string; title: string; type: string; count: number }[] }> => invoke("server_packs"),
  importPackViaServer: (link: string): Promise<StickerImportReport>                  => invoke("import_pack_via_server", { link }),
  packFilePath:       (name: string, file: string): Promise<string>                  => invoke("pack_file_path", { name, file }),
  importTelegramStickers: (bot_token: string, pack_ref: string): Promise<StickerImportReport> =>
    invoke("import_telegram_stickers", { botToken: bot_token, packRef: pack_ref }),
};

export interface CallSignal {
  type: "offer" | "answer" | "ice" | "reject" | "hangup" | "media" | "restart";
  sdp?: string;
  candidate?: RTCIceCandidateInit;
  /** reject: "busy" when the callee is already in a call. */
  reason?: string;
  /** media: the sender's live media state. */
  muted?: boolean;
  deaf?: boolean;
  cam?: boolean;
  screen?: boolean;
  screenAudio?: boolean;
  label?: string;
}

export interface CallSignalEvent {
  from: string;
  signal: CallSignal;
}

export interface PollVoteEvent { from: string; vote: { poll: string; options?: number[]; option?: number; at?: number }; gid?: string }

/** Send a control payload to a chat: 1:1 over the session, groups via fan-out. */
export function sendChatControl(chatId: string, payload: string): Promise<void> {
  return chatId.startsWith("grp_") ? api.groupControl(chatId, payload) : api.sendControl(chatId, payload);
}

export interface BackupStatus { enabled: boolean; last: { at: number; size: number } | null; has_phrase: boolean }

export const events = {
  onIncoming:        (fn: (p: IncomingPayload) => void)   => listen<IncomingPayload>("aqua://incoming", (e) => fn(e.payload)),
  onChatsUpdated:    (fn: () => void)                      => listen("aqua://chats_updated", () => fn()),
  onDisconnected:    (fn: () => void)                      => listen("aqua://disconnected", () => fn()),
  onTransportError:  (fn: (msg: string) => void)           => listen<string>("aqua://transport_error", (e) => fn(e.payload)),
  onCallSignal:      (fn: (p: CallSignalEvent) => void)    => listen<CallSignalEvent>("aqua://call_signal", (e) => fn(e.payload)),
  onPollVote:        (fn: (p: PollVoteEvent) => void) => listen<PollVoteEvent>("aqua://poll_vote", (e) => fn(e.payload)),
  onPollClose:       (fn: (p: { from: string; close: { poll: string }; gid?: string }) => void) =>
    listen<{ from: string; close: { poll: string }; gid?: string }>("aqua://poll_close", (e) => fn(e.payload)),
  onSeen:            (fn: (p: { from: string }) => void)   => listen<{ from: string }>("aqua://seen", (e) => fn(e.payload)),
  onPresence:        (fn: (p: { from: string; presence: import("./presence").Presence }) => void) =>
    listen<{ from: string; presence: import("./presence").Presence }>("aqua://presence", (e) => fn(e.payload)),
  onActivity:        (fn: (a: Activity[]) => void) => listen<Activity[]>("aqua://activity", (e) => fn(e.payload)),
  onReaction:        (fn: (p: { from: string; react: { key: string; emoji: string; add: boolean }; gid?: string }) => void) =>
    listen<{ from: string; react: { key: string; emoji: string; add: boolean }; gid?: string }>("aqua://reaction", (e) => fn(e.payload)),
  onGroupsUpdated:   (fn: () => void)                      => listen("aqua://groups_updated", () => fn()),
  onGroupJoinRequest: (fn: (p: { gid: string; from: string }) => void) => listen<{ gid: string; from: string }>("aqua://group_join_request", (e) => fn(e.payload)),
  onGroupCall:       (fn: (p: { from: string; gid: string; signal: Record<string, unknown> }) => void) =>
    listen<{ from: string; gid: string; signal: Record<string, unknown> }>("aqua://gcall", (e) => fn(e.payload)),
  onTyping:          (fn: (p: { from: string; gid?: string }) => void)   => listen<{ from: string; gid?: string }>("aqua://typing", (e) => fn(e.payload)),
};

/**
 * Convert a raw byte array (as returned by `readMediaBytes`) to a
 * `data:` URL for inline rendering. The backend already caps file size
 * (~20 MB) so we can safely base64-encode in one go.
 */
export function bytesToDataUrl(bytes: number[], mime: string): string {
  // Fast path for small blobs — build a binary string in chunks of
  // 32KB to avoid blowing out the call-stack on `apply(null, bytes)`.
  const CHUNK = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += CHUNK) {
    parts.push(String.fromCharCode.apply(null, bytes.slice(i, i + CHUNK) as unknown as number[]));
  }
  const b64 = btoa(parts.join(""));
  return `data:${mime};base64,${b64}`;
}
