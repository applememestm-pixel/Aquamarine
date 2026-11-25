import { get, writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { info } from "./stores";
import { lang, tr } from "./i18n";
import { askConfirm } from "./components/Confirm.svelte";

type Processing = { speech?: boolean; translation?: boolean; pro_bots?: boolean };
export const processing = writable<Processing>({});
let connection = "";
let checkedAt = 0;

export async function refreshProcessing(force = false) {
  if (!connection || (!force && Date.now() - checkedAt < 30_000)) return;
  const checking = connection;
  checkedAt = Date.now();
  const value = await invoke<Processing>("server_processing", { operation: "capabilities" }).catch(() => ({}));
  if (checking === connection) processing.set(value && typeof value === "object" ? value : {});
}

info.subscribe(i => {
  const next = i?.connected ? JSON.stringify([i.username, i.server_url]) : "";
  if (next === connection) return;
  connection = next;
  checkedAt = 0;
  processing.set({});
  void refreshProcessing();
});

export function consentToProcessing() {
  return askConfirm({
    title: tr("Send this message for processing?"),
    text: tr("The selected content will be sent to your Aquamarine server for processing."),
    ok: tr("Continue"),
  });
}

export async function translateSelected(text: string) {
  if (!(await consentToProcessing())) return null;
  const result = await invoke<{ text: string }>("server_processing", {
    operation: "translate", text, target: get(lang),
  });
  return result.text;
}

export async function processSpeech(pcm: Uint8Array) {
  const blocks: string[] = [];
  const size = 24 * 1024;
  for (let offset = 0; offset < pcm.length; offset += size) {
    blocks.push(btoa(String.fromCharCode(...pcm.subarray(offset, offset + size))));
  }
  const result = await invoke<{ text: string }>("server_processing", {
    operation: "speech", pcm: blocks.join(""),
  });
  return result.text;
}
