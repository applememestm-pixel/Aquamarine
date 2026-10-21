/**
 * Bot keyboards (Telegram API layer on the relay): a bot message starts with
 * `[kb:v1]{…}` and the numbered fallback list follows an invisible separator.
 */
export interface KbButton {
  t: string;
  /** callback_data */
  cb?: string;
  url?: string;
  /** web_app url */
  wa?: string;
  /** switch_inline_query text */
  q?: string;
  contact?: boolean;
  location?: boolean;
}
export interface BotKb {
  kind: "inline" | "reply" | "remove";
  /** The bot's message id (callback_query.message.message_id). */
  m: number;
  rows?: KbButton[][];
  once?: boolean;
  /** input_field_placeholder */
  ph?: string;
}

const KB = "[kb:v1]";

/** `{ body, kb }` for a bot message; plain text otherwise. */
export function parseBotText(text: string): { body: string; kb: BotKb | null } {
  if (!text.startsWith(KB)) return { body: text, kb: null };
  const nl = text.indexOf("\n");
  const head = nl < 0 ? text.slice(KB.length) : text.slice(KB.length, nl);
  const rest = nl < 0 ? "" : text.slice(nl + 1);
  const body = rest.split("⁣")[0];
  try {
    const kb = JSON.parse(head) as BotKb;
    if (!kb || typeof kb !== "object" || !["inline", "reply", "remove"].includes(kb.kind)) return { body, kb: null };
    kb.rows = (kb.rows ?? []).filter((r) => Array.isArray(r)).map((r) => r.filter((b) => b && typeof b.t === "string"));
    return { body, kb };
  } catch {
    return { body, kb: null };
  }
}
