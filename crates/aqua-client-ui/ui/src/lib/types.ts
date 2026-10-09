/** Mirrors `ChatSummary` in the Rust backend. */
export interface ChatSummary {
  id: string;
  name: string;
  letter: string;
  gradient: string;
  preview: string;
  preview_is_typing: boolean;
  preview_by_me: boolean;
  has_lock: boolean;
  time: string;
  unread: number;
  online: boolean;
  verified: boolean;
  channel: boolean;
  /** True once we have a live Signal session with this peer. */
  live: boolean;
}

/** Mirrors `ChatMessage` in the Rust backend. */
export interface ChatMessage {
  id: string;
  from: string;
  time: string;
  kind: MessageKind;
  /** Local history: deleted by the sender (kept), previous versions. */
  meta?: { deleted?: boolean; edits?: string[] } | null;
}

export type MessageKind =
  | { type: "text";            content: { text: string } }
  | { type: "code";            content: { code: string; language: string } }
  | { type: "file";            content: {
      cid: string;
      key_b64: string;
      nonce_b64: string;
      name: string;
      size: number;
      mime: string | null;
      saved_at: string | null;
      /** Render hint: "sticker" | "gif" | "image" | "video" | "voice" | undefined */
      category: string | null;
    } }
  | { type: "article";         content: {
      title: string;
      body: string;             // markdown
      cover_data_url: string | null;
    } }
  | { type: "poll";            content: {
      id: string;
      question: string;
      options: string[];
      multi: boolean;
      quiz?: boolean;
      correct?: number | null;
      explanation?: string;
      public?: boolean;
      close_at?: number | null;
    } }
  | { type: "reply";           content: {
      quoted_author: string;
      quoted_text: string;
      text: string;
    } }
  | { type: "system";          content: { text: string } }
  | { type: "date_separator";  content: { label: string } };

/** Mirrors `InitInfo` in the Rust backend. */
export interface InitInfo {
  has_identity: boolean;
  /** True only after the email-registration wizard completes on this device. */
  registered: boolean;
  username: string | null;
  email?: string | null;
  session_fingerprint: string;
  opk_available: number;
  spk_rotates_in_days: number;
  connected?: boolean;
  server_url?: string | null;
  proxy?: string | null;
  data_dir?: string;
}

/** Mirrors `FingerprintView` in the Rust backend. */
export interface FingerprintView {
  words: string[];
  hex: string;
  peer_name: string;
}

export interface SendResult {
  id: string;
  time: string;
}

export interface AttachResult {
  message_id: string;
  cid: string;
  size: number;
}

export interface DownloadResult {
  saved_path: string;
}

// ---- Local media library (stickers / GIFs) ----

export interface StickerItem {
  id: string;
  path: string;
  emoji: string | null;
  mime: string;
  size: number;
}

export interface StickerPack {
  id: string;
  title: string;
  /** "sticker" | "emoji" (Telegram custom-emoji pack). */
  kind?: string;
  /** Telegram short name, when imported from t.me/add… */
  source?: string | null;
  items: StickerItem[];
}

export interface GifItem {
  id: string;
  path: string;
  name: string;
  mime?: string;
  size: number;
}

export interface LocalMediaList {
  packs: StickerPack[];
  gifs: GifItem[];
  stickers_dir: string;
  gifs_dir: string;
}

/** Result of importing a Telegram sticker pack (mirrors `ImportReport`). */
export interface StickerImportReport {
  pack_name: string;
  pack_title: string;
  dir: string;
  imported: number;
  renderable: number;
  animated: number;
  video: number;
  total: number;
}

/** A public profile card from the server directory. */
export interface DirProfile {
  username: string;
  display_name: string;
  about: string;
  phone: string;
  is_bot: boolean;
  /** Public profile extras (banner colours, status, pronouns, …). */
  extra?: Record<string, unknown>;
  /** Server version of the avatar / banner image; 0 = none. */
  avatar_ver?: number;
  banner_ver?: number;
}

/** Discord-style activity detected on this PC (or shared by a peer). */
export interface Activity {
  kind: "playing" | "listening" | "watching";
  name: string;
  details: string;
  state: string;
}

/** A bot the current user owns (with its API token). */
export interface BotInfo {
  username: string;
  token: string;
}

// ---- Groups / supergroups / channels ----

export type GroupKind = "group" | "supergroup" | "channel";
export type Perm = "all" | "admins";
export type GroupRole = "owner" | "admin" | "member";

/** Telegram admin rights (all optional; owner has everything). */
export interface AdminRights {
  change_info?: boolean;
  post_messages?: boolean;    // channels
  edit_messages?: boolean;    // channels: edit others' posts
  delete_messages?: boolean;
  ban_users?: boolean;
  invite_users?: boolean;
  pin_messages?: boolean;
  manage_tags?: boolean;
  add_admins?: boolean;
  anonymous?: boolean;
}

export interface GroupMember {
  username: string;
  role: GroupRole;
  /** Member tag shown next to the name (Telegram's member tags / "prefix"). */
  tag?: string;
  /** Custom admin title ("Moderator", "Founder"…). */
  title?: string;
  rights?: AdminRights;
  /** Restricted: can read but not send. */
  muted?: boolean;
}

/** Discord-style server tag: up to 4 characters + badge + colour. */
export interface ClanTag {
  text: string;
  badge: string;
  color: string;
}

export interface GroupSettings {
  send_messages: Perm;
  send_media: Perm;
  send_stickers: Perm;
  send_polls: Perm;
  add_members: Perm;
  pin_messages: Perm;
  change_info: Perm;
  slow_mode: number;
  sign_messages: boolean;
  is_public: boolean;
  public_link: string;
  history_visible: boolean;
  // ---- Telegram parity (stored in the backend's flattened extras) ----
  send_photos?: Perm;
  send_videos?: Perm;
  send_files?: Perm;
  send_music?: Perm;
  send_voice?: Perm;
  send_video_msgs?: Perm;
  embed_links?: Perm;
  edit_own_tags?: Perm;
  reactions?: "all" | "some" | "none";
  allowed_reactions?: string[];
  protected_content?: boolean;
  join_requests?: boolean;
  show_authors?: boolean;
  hide_members?: boolean;
  discussion?: string;
  accent?: string;
}

export interface JoinRequest { username: string; note: string; at: string }
export interface GroupLogEntry { at: string; by: string; text: string }

/** Mirrors `Group` in the Rust backend. */
export interface Group {
  id: string;
  kind: GroupKind;
  title: string;
  description: string;
  gradient: string;
  owner: string;
  members: GroupMember[];
  settings: GroupSettings;
  created_at: string;
  version: number;
  banned?: string[];
  requests?: JoinRequest[];
  log?: GroupLogEntry[];
  clan?: ClanTag | null;
}
