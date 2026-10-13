<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  // Null closed, string = username whose profile we're showing,
  // or "__me__" for the current user.
  export const profileState = writable<string | null>(null);

  export function openProfile(username = "__me__") { profileState.set(username); }
  export function closeProfile() { profileState.set(null); }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { profile, updateProfile, type Profile } from "../profile";
  import { contacts, saveContact, getContact } from "../contacts";
  import { info } from "../stores";
  import { api, bytesToDataUrl } from "../tauri";
  import { icons } from "../icons";
  import { openFileDialog } from "../dialog";
  import { showToast } from "./Toast.svelte";
  import { broadcastPresence, peerPresence, myActivity, shareActivity, myOnline, activityVerb, activityLine } from "../presence";
  import { peerCards, refreshPeer, publishMyProfile, setMyImage, setMyImageBytes, clearMyImage, str, clans } from "../people";
  import ClanTag from "./ClanTag.svelte";
  import { privacy, PRIV_ITEMS, allowed, exceptionOf, setException, exceptionCount, contactSet, type PrivKey, type Exception } from "../privacy";
  import { t, tr } from "../i18n";
  import { blocked, setBlocked } from "../ghost";
  import { cropImage } from "./ImageCropper.svelte";
  import type { Activity } from "../types";
  import { openRegistration, registrationDismissed } from "./RegistrationWizard.svelte";

  async function logOut() {
    try {
      info.set(await api.logOut());
      registrationDismissed.set(false);
      closeProfile();
      openRegistration();
      showToast(tr("Logged out"));
    } catch (e) {
      showToast(`Logout failed: ${e}`);
    }
  }

  let uploading = $state<"" | "avatar" | "banner">("");

  /** Avatar / banner go straight to the server so everyone sees them. */
  async function pickImage(kind: "avatar" | "banner") {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: kind === "avatar" ? "Pick an avatar" : "Pick a banner",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      if (path.toLowerCase().endsWith(".gif")) {
        // Animated: upload as-is (cropping would flatten it to one frame).
        uploading = kind;
        await setMyImage(kind, path);
      } else {
        const raw = await api.readUserImage(path);
        const src = URL.createObjectURL(new Blob([new Uint8Array(raw)]));
        const blob = await cropImage(src, kind === "avatar"
          ? { aspect: 1, round: true, outWidth: 512, title: "Position your avatar" }
          : { aspect: 3, outWidth: 1500, title: "Position your banner" });
        URL.revokeObjectURL(src);
        if (!blob) return;
        uploading = kind;
        await setMyImageBytes(kind, "image/webp", Array.from(new Uint8Array(await blob.arrayBuffer())));
      }
      if (draft) draft = { ...draft, avatar_url: $profile.avatar_url, banner_url: $profile.banner_url };
      showToast(kind === "avatar" ? "Avatar updated — visible to everyone" : "Banner updated — visible to everyone");
    } catch (e) {
      showToast(`Could not upload ${kind}: ${e}`, 5000);
    } finally {
      uploading = "";
    }
  }
  async function removeImage(kind: "avatar" | "banner") {
    try {
      uploading = kind;
      await clearMyImage(kind);
      if (draft) draft = { ...draft, avatar_url: $profile.avatar_url, banner_url: $profile.banner_url };
    } catch (e) {
      showToast(`Could not remove ${kind}: ${e}`);
    } finally {
      uploading = "";
    }
  }
  // Draft state for the editor.
  let draft = $state<Profile | null>(null);
  let editing = $state(false);
  let activeUsername = $derived($profileState ?? "");
  let isMe = $derived(activeUsername === "__me__");

  // Build the display view for a contact OR the current user.
  interface View {
    display_name: string;
    username: string;
    about: string;
    phone: string;
    birthday: string;
    favourite_music: string;
    banner_a: string;
    banner_b: string;
    verified: boolean;
    fingerprint: string | null;
    avatar_url: string;
    status: string;
    pronouns: string;
    is_bot: boolean;
    banner_url: string;
    online: string;
    activity: Activity[];
  }
  let view = $derived<View | null>(buildView(activeUsername, $profile, $contacts, $info, $peerCards, $peerPresence, $myActivity));
  // Which avatar image to show: the live draft when editing myself, else
  // the stored/served one.
  let shownAvatar = $derived(
    (editing && isMe && draft) ? (draft.avatar_url ?? "") : (view?.avatar_url ?? ""),
  );
  let shownBanner = $derived(
    (editing && isMe && draft) ? (draft.banner_url ?? "") : (view?.banner_url ?? ""),
  );
  let bannerA = $derived((editing && isMe && draft) ? draft.banner_a : (view?.banner_a ?? "#4F7AFE"));
  let bannerB = $derived((editing && isMe && draft) ? draft.banner_b : (view?.banner_b ?? "#9B6CFF"));
  // Now-playing to show on the card: our own when viewing self, else the
  // peer's last-broadcast presence.
  let np = $derived.by(() => {
    if (isMe) return $profile.listening && $profile.listening.title ? $profile.listening : null;
    const p = $peerPresence[activeUsername];
    return p?.listening && p.listening.title ? p.listening : null;
  });

  function buildView(
    uname: string,
    me: Profile,
    _contactsMap: Record<string, unknown>,
    i: typeof $info,
    cards: typeof $peerCards,
    pres: typeof $peerPresence,
    myAct: Activity[],
  ): View | null {
    if (!uname) return null;
    if (uname === "__me__") {
      return {
        display_name: me.display_name,
        username: i?.username ?? "—",
        about: me.about,
        phone: me.phone,
        birthday: me.birthday,
        favourite_music: me.favourite_music,
        banner_a: me.banner_a,
        banner_b: me.banner_b,
        verified: true,
        fingerprint: i?.session_fingerprint ?? null,
        avatar_url: me.avatar_url ?? "",
        status: me.status ?? "",
        pronouns: me.pronouns ?? "",
        is_bot: false,
        banner_url: me.banner_url ?? "",
        online: getStore(myOnline),
        activity: getStore(shareActivity) ? myAct : [],
      };
    }
    const c = getContact(uname);
    const srv = cards[uname];
    const x = srv?.extra ?? {};
    const pr = pres[uname];
    return {
      // Prefer the server-published card, then the local contact book.
      display_name: c?.full_name || srv?.display_name || uname,
      username: uname,
      // Fields they keep private arrive over E2EE presence (`pp`).
      about: srv?.about || pr?.pp?.about || "",
      phone: srv?.phone || pr?.pp?.phone || c?.phone || "",
      birthday: str(x.birthday) || pr?.pp?.birthday || "",
      favourite_music: str(x.favourite_music),
      banner_a: str(x.banner_a) || (srv?.is_bot ? "#FFB067" : "#4F7AFE"),
      banner_b: str(x.banner_b) || (srv?.is_bot ? "#FF7CAC" : "#9B6CFF"),
      verified: !!srv?.fetched && !!srv?.display_name,
      fingerprint: null,
      avatar_url: srv?.avatar ?? "",
      status: pr?.status || str(x.status),
      pronouns: str(x.pronouns),
      is_bot: !!srv?.is_bot,
      banner_url: srv?.banner ?? "",
      online: pr?.online ?? "",
      activity: pr?.activity ?? [],
    };
  }

  // Refresh the peer's public card whenever their profile opens.
  $effect(() => {
    const u = $profileState;
    privPage = false;
    if (u && u !== "__me__") void refreshPeer(u, true);
  });

  // ---- Privacy for this one person (Telegram X) ----
  let privPage = $state(false);
  const privCount = $derived(activeUsername && !isMe ? exceptionCount(activeUsername, $privacy) : 0);
  function setPriv(k: PrivKey, v: Exception) {
    setException(k, activeUsername, v);
  }
  function setAllPriv(v: Exception) {
    for (const it of PRIV_ITEMS) setException(it.key, activeUsername, v);
  }

  function startEdit() {
    if (!isMe) return;
    draft = { ...$profile };
    editing = true;
  }
  function cancelEdit() { editing = false; draft = null; }
  async function saveEdit() {
    if (!draft) return;
    updateProfile(draft);
    editing = false;
    draft = null;
    // Publish name / bio / banner colours to the server, then tell peers.
    try { await publishMyProfile(); showToast(tr("Profile saved")); }
    catch (e) { broadcastPresence(); showToast(`Saved on this device — will publish when connected (${e})`, 5000); }
  }
  function setListening(field: "title" | "artist" | "url", value: string) {
    if (!draft) return;
    const cur = draft.listening ?? { title: "", artist: "" };
    draft = { ...draft, listening: { ...cur, [field]: value } };
  }

  // Edit contact (saves to contact book).
  let contactDraft = $state<{ full_name: string; phone: string } | null>(null);
  function startContactEdit() {
    if (isMe) return;
    const c = getContact(activeUsername);
    contactDraft = { full_name: c?.full_name ?? "", phone: c?.phone ?? "" };
    editing = true;
  }
  function saveContactEdit() {
    if (!contactDraft) return;
    saveContact(activeUsername, contactDraft);
    editing = false;
    contactDraft = null;
  }

  function onBackdrop(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("backdrop")) closeProfile();
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") closeProfile(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });
</script>

{#if $profileState && view}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close profile")}
    onclick={onBackdrop}
    onkeydown={(e) => { if (e.key === "Escape" || e.key === "Enter") closeProfile(); }}
  >
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <!-- Close stays put; banner + body scroll together, so the avatar is
           never clipped under the banner when the form is long. -->
      <button class="close" onclick={closeProfile} title={$t("Close (Esc)")}>×</button>
      <div class="scroll">
      <div class="banner" style:background={shownBanner ? `center / cover no-repeat url("${shownBanner}")` : `linear-gradient(135deg, ${bannerA}, ${bannerB})`}>
        {#if editing && isMe && draft}
          <div class="banner-tools">
            <button class="bn-btn" onclick={() => pickImage("banner")} disabled={!!uploading}>{uploading === "banner" ? "Uploading…" : shownBanner ? "Change banner" : "Upload banner"}</button>
            {#if shownBanner}<button class="bn-btn" onclick={() => removeImage("banner")} disabled={!!uploading}>{$t("Remove")}</button>{/if}
          </div>
        {/if}
      </div>

      <div class="body">
        <div class="avatar-wrap">
          <div class="avatar" style={`background: linear-gradient(135deg, ${bannerA}, ${bannerB});`}>
            {#if shownAvatar}
              <img src={shownAvatar} alt={view.display_name} />
            {:else}
              {view.display_name.charAt(0).toUpperCase()}
            {/if}
          </div>
          {#if view.online && view.online !== "offline"}<span class="presence {view.online}" title={view.online}></span>{/if}
          {#if editing && isMe && draft}
            <button class="av-edit" onclick={() => pickImage("avatar")} title={$t("Change avatar")} disabled={!!uploading}>{$t(uploading === "avatar" ? "…" : "Change")}</button>
            {#if draft.avatar_url}
              <button class="av-remove" onclick={() => removeImage("avatar")} title={$t("Remove avatar")} disabled={!!uploading}>×</button>
            {/if}
          {/if}
        </div>

        {#if editing && isMe && draft}
          <input
            class="edit-name"
            bind:value={draft.display_name}
            placeholder={$t("Display name")}
          />
          <div class="username">@{view.username}</div>
          <div class="pub-note">{$t("Avatar and banner are published to the server — everyone you chat with sees them.")}</div>
          <label class="field">
            <span>{$t("Custom status")}</span>
            <input bind:value={draft.status} placeholder={$t("What's on your mind?")} />
          </label>
          <label class="field">
            <span>{$t("Pronouns")}</span>
            <input bind:value={draft.pronouns} placeholder="she/her · he/him · они" />
          </label>
          <div class="field">
            <span>{$t("Now playing (shown to people you chat with)")}</span>
            <div class="np-edit">
              <input placeholder={$t("Track title")} value={draft.listening?.title ?? ""} oninput={(e) => setListening("title", (e.target as HTMLInputElement).value)} />
              <input placeholder={$t("Artist")} value={draft.listening?.artist ?? ""} oninput={(e) => setListening("artist", (e.target as HTMLInputElement).value)} />
              <input placeholder={$t("Link (Spotify / YouTube, optional)")} value={draft.listening?.url ?? ""} oninput={(e) => setListening("url", (e.target as HTMLInputElement).value)} />
            </div>
          </div>
          <label class="field">
            <span>{$t("About")}</span>
            <textarea bind:value={draft.about} rows="3" placeholder={$t("Tell people a little about yourself...")}></textarea>
          </label>
          <label class="field">
            <span>{$t("Phone")}</span>
            <input bind:value={draft.phone} placeholder="+7 999 123-45-67" />
          </label>
          <label class="field">
            <span>{$t("Birthday")}</span>
            <input bind:value={draft.birthday} placeholder={$t("15 March 1998")} />
          </label>
          <label class="field">
            <span>{$t("Favourite music")}</span>
            <input bind:value={draft.favourite_music} placeholder={$t("Pink Floyd, 2 Pac, Radiohead...")} />
          </label>
          <div class="row">
            <label class="field half">
              <span>{$t("Banner A")}</span>
              <input type="color" bind:value={draft.banner_a} />
            </label>
            <label class="field half">
              <span>{$t("Banner B")}</span>
              <input type="color" bind:value={draft.banner_b} />
            </label>
          </div>
          <div class="buttons">
            <button class="btn-ghost" onclick={cancelEdit}>{$t("Cancel")}</button>
            <button class="btn-primary" onclick={saveEdit}>{$t("Save")}</button>
          </div>
        {:else if editing && !isMe && contactDraft}
          <div class="username">@{view.username}</div>
          <label class="field">
            <span>{$t("Full name")}</span>
            <input bind:value={contactDraft.full_name} placeholder="Иван Иванов" />
          </label>
          <label class="field">
            <span>{$t("Phone")}</span>
            <input bind:value={contactDraft.phone} placeholder="+7 999 123-45-67" />
          </label>
          <div class="buttons">
            <button class="btn-ghost" onclick={cancelEdit}>{$t("Cancel")}</button>
            <button class="btn-primary" onclick={saveContactEdit}>{$t("Save")}</button>
          </div>
        {:else if privPage && !isMe}
          <button class="pv-back" onclick={() => (privPage = false)}>‹ {view.display_name}</button>
          <h2 class="name">{$t("Privacy for @{u}", { u: activeUsername })}</h2>
          <div class="pv-sub">
            {$t("Rules here override your general privacy settings for this person and show up there as exceptions.")}
            {#if $contactSet.has(activeUsername)}<b>{$t("In your contacts.")}</b>{/if}
          </div>
          <div class="pv-quick">
            <button onclick={() => setAllPriv("allow")}>{$t("Allow everything")}</button>
            <button onclick={() => setAllPriv("deny")}>{$t("Restrict everything")}</button>
            <button onclick={() => setAllPriv(null)}>{$t("Reset")}</button>
          </div>
          {#each PRIV_ITEMS as it (it.key)}
            {@const ex = exceptionOf(it.key, activeUsername, $privacy)}
            {@const eff = allowed(it.key, activeUsername, $privacy)}
            <div class="pv-row">
              <div class="pv-text">
                <div class="pv-name">{$t(it.label)}</div>
                <div class="pv-eff" class:no={!eff}>{eff ? $t("Can see / can do") : $t("Restricted")}{#if !ex}{" · "}{$t("by default")}{/if}</div>
              </div>
              <div class="pv-seg">
                <button class:on={ex === null} onclick={() => setPriv(it.key, null)} title={$t("Follow general settings")}>{$t("Default")}</button>
                <button class:on={ex === "allow"} class="ok" onclick={() => setPriv(it.key, "allow")}>{$t("Allow")}</button>
                <button class:on={ex === "deny"} class="bad" onclick={() => setPriv(it.key, "deny")}>{$t("Deny")}</button>
              </div>
            </div>
          {/each}
        {:else}
          <h2 class="name">
            {view.display_name}
            {#if view.verified}<span class="badge" title={$t("Verified identity")}>{@html icons.verified}</span>{/if}
            {#if view.is_bot}<span class="bot-tag">BOT</span>{/if}
            {#if view.pronouns}<span class="pronouns">{view.pronouns}</span>{/if}
            {#if $clans[isMe ? ($info?.username ?? "") : activeUsername]}<ClanTag tag={$clans[isMe ? ($info?.username ?? "") : activeUsername]} size="md" />{/if}
          </h2>
          <div class="username">@{view.username}</div>
          {#if view.status}
            <div class="status-line">{view.status}</div>
          {/if}

          {#each view.activity as a (a.kind + a.name)}
            <div class="act-card">
              <div class="act-ic" data-k={a.kind}>{a.kind === "playing" ? "🎮" : a.kind === "listening" ? "🎧" : "📺"}</div>
              <div class="np-info">
                <div class="np-label">{activityVerb(a)}</div>
                <div class="np-track"><b>{a.name}</b></div>
                {#if activityLine(a)}<div class="act-line">{activityLine(a)}</div>{/if}
              </div>
            </div>
          {/each}

          {#if np}
            <a class="np-card" href={np.url || undefined} target="_blank" rel="noopener" title={np.url ? "Open" : ""}>
              <span class="np-ic">♪</span>
              <div class="np-info">
                <div class="np-label">{$t("Listening to")}</div>
                <div class="np-track">{np.title}{#if np.artist} · {np.artist}{/if}</div>
              </div>
            </a>
          {/if}

          {#if view.about}
            <section class="section">
              <div class="section-label">{$t("About me")}</div>
              <p class="section-body">{view.about}</p>
            </section>
          {/if}

          {#if view.phone}
            <section class="section">
              <div class="section-label">{$t("Phone")}</div>
              <p class="section-body mono">{view.phone}</p>
            </section>
          {/if}

          {#if view.birthday}
            <section class="section">
              <div class="section-label">{$t("Birthday")}</div>
              <p class="section-body">{view.birthday}</p>
            </section>
          {/if}

          {#if view.favourite_music}
            <section class="section">
              <div class="section-label">{$t("Favourite music")}</div>
              <p class="section-body">{view.favourite_music}</p>
            </section>
          {/if}

          {#if view.fingerprint}
            <section class="section">
              <div class="section-label">{$t("Session fingerprint")}</div>
              <p class="section-body mono small">{view.fingerprint}</p>
            </section>
          {/if}

          <div class="buttons">
            {#if isMe}
              <button class="btn-primary" onclick={startEdit}>{$t("Edit profile")}</button>
              <button class="btn-ghost danger" onclick={logOut}>{$t("Log out")}</button>
            {:else}
              <button class="btn-ghost danger" onclick={() => setBlocked(activeUsername, !$blocked.has(activeUsername))}>
                {$blocked.has(activeUsername) ? $t("Unblock") : $t("Block")}
              </button>
              <button class="btn-ghost" onclick={() => (privPage = true)}>
                {$t("Privacy for this person")}{#if privCount > 0} <span class="pv-badge">{privCount}</span>{/if}
              </button>
              <button class="btn-primary" onclick={startContactEdit}>{$t("Edit contact")}</button>
            {/if}
          </div>
        {/if}
      </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.6);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 120;
    animation: fade-in 0.15s ease-out;
    padding: 20px;
  }
  @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }

  .sheet {
    width: min(440px, 94vw);
    max-height: 86vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    overflow: hidden;
    position: relative;
    animation: slide-up 0.22s ease-out;
  }
  .scroll { flex: 1 1 auto; min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .banner {
    height: 120px;
    position: relative;
    flex-shrink: 0;
  }
  .banner-tools { position: absolute; right: 12px; bottom: 12px; display: flex; gap: 6px; align-items: center; }
  .bn-btn {
    background: rgba(0, 0, 0, 0.55); color: #fff; border: none; border-radius: 8px;
    padding: 5px 10px; font-size: 12px; font-weight: 700; cursor: pointer; backdrop-filter: blur(6px);
  }
  .bn-btn:hover:not(:disabled) { background: rgba(0, 0, 0, 0.75); }
  .bn-btn:disabled { opacity: 0.6; cursor: default; }
  .pv-back { background: none; border: none; color: var(--accent); font: inherit; font-weight: 700; font-size: 13px; padding: 0 0 6px; cursor: pointer; }
  .pv-sub { font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; margin: 6px 0 12px; }
  .pv-sub b { color: var(--fg-muted); font-weight: 600; margin-left: 4px; }
  .pv-quick { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 10px; }
  .pv-quick button {
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 100px;
    padding: 5px 11px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .pv-quick button:hover { color: var(--accent); border-color: var(--accent-soft); }
  .pv-row { display: flex; align-items: center; gap: 10px; padding: 9px 12px; background: var(--bg-input); border-radius: 10px; margin-bottom: 6px; }
  .pv-text { flex: 1; min-width: 0; }
  .pv-name { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .pv-eff { font-size: 11.5px; color: var(--accent); }
  .pv-eff.no { color: #ff8f8f; }
  .pv-seg { display: flex; background: var(--bg-shell); border: 1px solid var(--border); border-radius: 9px; padding: 2px; flex-shrink: 0; }
  .pv-seg button { border: none; background: none; color: var(--fg-dim); font: inherit; font-size: 11.5px; font-weight: 700; padding: 5px 8px; border-radius: 7px; cursor: pointer; }
  .pv-seg button.on { background: var(--bg-hover); color: var(--fg); }
  .pv-seg button.ok.on { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent); }
  .pv-seg button.bad.on { background: rgba(255, 77, 109, 0.16); color: #ff8f8f; }
  .pv-badge { background: var(--accent); color: #06070C; border-radius: 100px; padding: 0 6px; font-size: 11px; margin-left: 4px; }
  .pub-note { font-size: 11.5px; color: var(--fg-dim); margin: 4px 0 10px; }
  .act-card {
    display: flex; align-items: center; gap: 12px;
    margin-top: 12px; padding: 10px 12px;
    background: var(--bg-input); border: 1px solid var(--border); border-radius: 12px;
  }
  .act-ic { width: 40px; height: 40px; border-radius: 10px; display: grid; place-items: center; font-size: 20px; background: var(--accent-soft); flex-shrink: 0; }
  .act-line { font-size: 12px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .close {
    position: absolute;
    z-index: 3;
    top: 10px; right: 10px;
    width: 28px; height: 28px;
    background: rgba(0, 0, 0, 0.35);
    border: none;
    border-radius: 8px;
    color: #fff;
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    backdrop-filter: blur(6px);
  }
  .close:hover { background: rgba(0, 0, 0, 0.55); }

  .body {
    position: relative;
    padding: 0 22px 22px;
    margin-top: -44px;
  }

  .avatar-wrap { position: relative; width: 88px; margin-bottom: 14px; }
  .avatar {
    width: 88px; height: 88px;
    border-radius: 50%;
    display: grid; place-items: center;
    font-size: 36px;
    font-weight: 800;
    color: #06070C;
    border: 5px solid var(--bg-shell);
    box-shadow: 0 8px 20px -4px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .avatar img { width: 100%; height: 100%; object-fit: cover; }
  .presence {
    position: absolute;
    right: 4px; bottom: 4px;
    width: 18px; height: 18px;
    border-radius: 50%;
    background: var(--online);
    border: 4px solid var(--bg-shell);
  }
  .presence.idle { background: #FAA61A; }
  .presence.dnd { background: #ED4245; }
  .av-edit {
    position: absolute;
    left: 50%; bottom: -6px; transform: translateX(-50%);
    padding: 3px 10px;
    background: var(--accent); color: #06070C;
    border: 2px solid var(--bg-shell); border-radius: 100px;
    font-size: 10.5px; font-weight: 700; cursor: pointer;
    white-space: nowrap;
  }
  .av-edit:hover { filter: brightness(1.08); }
  .av-remove {
    position: absolute;
    right: -2px; top: -2px;
    width: 22px; height: 22px;
    background: var(--danger); color: #fff;
    border: 2px solid var(--bg-shell); border-radius: 50%;
    font-size: 13px; line-height: 1; cursor: pointer;
  }

  .bot-tag {
    font-size: 9.5px; font-weight: 800; letter-spacing: 0.06em;
    background: var(--accent); color: #06070C;
    padding: 2px 6px; border-radius: 5px;
  }
  .pronouns {
    font-size: 11.5px; font-weight: 600; color: var(--fg-dim);
    background: var(--bg-input); border: 1px solid var(--border);
    padding: 2px 8px; border-radius: 100px;
  }
  .status-line {
    font-size: 13px; color: var(--fg-muted);
    margin: -10px 0 16px; line-height: 1.4;
  }

  .np-card {
    display: flex; align-items: center; gap: 11px;
    padding: 10px 12px; margin-bottom: 14px;
    background: var(--accent-soft); border: 1px solid var(--accent-soft);
    border-radius: 12px; text-decoration: none;
  }
  .np-card:hover { border-color: var(--accent); }
  .np-ic {
    width: 34px; height: 34px; flex-shrink: 0; border-radius: 9px;
    display: grid; place-items: center; font-size: 17px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C;
  }
  .np-info { min-width: 0; }
  .np-label { font-size: 10.5px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.06em; }
  .np-track { font-size: 13px; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .np-edit { display: flex; flex-direction: column; gap: 6px; }

  .name {
    margin: 0;
    font-size: 20px;
    font-weight: 800;
    letter-spacing: -0.01em;
    display: flex; align-items: center; gap: 6px;
  }
  .badge :global(svg) { width: 16px; height: 16px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  .username {
    font-family: var(--font-mono);
    font-size: 12.5px;
    color: var(--fg-dim);
    margin: 2px 0 16px;
  }

  .section {
    background: var(--bg-input);
    border-radius: 10px;
    padding: 10px 14px;
    margin-bottom: 8px;
  }
  .section-label {
    font-size: 11px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin-bottom: 4px;
  }
  .section-body {
    margin: 0;
    font-size: 13.5px;
    color: var(--fg);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }
  .section-body.mono { font-family: var(--font-mono); font-size: 12px; color: var(--fg-muted); }
  .section-body.small { font-size: 11px; }

  .edit-name {
    width: 100%;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-strong);
    color: var(--fg);
    font-family: inherit;
    font-size: 20px;
    font-weight: 800;
    padding: 2px 0 6px;
    outline: none;
  }
  .edit-name:focus { border-bottom-color: var(--accent); }

  .field { display: flex; flex-direction: column; gap: 4px; margin-top: 10px; }
  .field span {
    font-size: 11px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .field input, .field textarea {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-family: inherit;
    font-size: 13.5px;
    padding: 8px 11px;
    outline: none;
    resize: vertical;
  }
  .field input:focus, .field textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field input[type="color"] { padding: 2px; height: 36px; cursor: pointer; }
  .row { display: flex; gap: 10px; }
  .field.half { flex: 1; }

  .buttons {
    display: flex; justify-content: flex-end; gap: 10px;
    margin-top: 16px;
  }
  .btn-ghost, .btn-primary {
    padding: 8px 16px;
    border-radius: 10px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: none;
    font-family: inherit;
  }
  .btn-ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--fg-dim);
  }
  .btn-ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .btn-ghost.danger { color: #ff8f8f; }
  .btn-ghost.danger:hover { color: #ff6b6b; border-color: rgba(255,107,107,0.4); background: rgba(255,77,109,0.08); }
  .btn-primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .btn-primary:hover { transform: translateY(-1px); }
</style>
