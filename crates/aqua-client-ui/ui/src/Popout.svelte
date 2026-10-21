<script lang="ts">
  /**
   * One chat in its own window (Ctrl+click a chat in the list). Same app
   * state as the main window — this only shows a different view of it, and
   * leaves connecting, reminders, backups and calls to the main window.
   */
  import { onMount } from "svelte";
  import { api, events } from "./lib/tauri";
  import { info, chats, activeId, loadActiveMessages } from "./lib/stores";
  import { initTheme } from "./lib/theme";
  import { initShortcuts } from "./lib/shortcuts";
  import { listen } from "@tauri-apps/api/event";
  import TitleBar from "./lib/components/TitleBar.svelte";
  import ChatWindow from "./lib/components/ChatWindow.svelte";
  import Toast from "./lib/components/Toast.svelte";
  import Confirm from "./lib/components/Confirm.svelte";
  import SendDialog from "./lib/components/SendDialog.svelte";
  import InputPrompt from "./lib/components/InputPrompt.svelte";
  import Lightbox from "./lib/components/Lightbox.svelte";
  import MediaPicker from "./lib/components/MediaPicker.svelte";
  import ForwardPicker from "./lib/components/ForwardPicker.svelte";
  import ProfileModal from "./lib/components/ProfileModal.svelte";
  import PackSheet from "./lib/components/PackSheet.svelte";
  import MiniApp from "./lib/components/MiniApp.svelte";
  import PlannerComposer from "./lib/components/PlannerComposer.svelte";
  import ChatCalendar from "./lib/components/ChatCalendar.svelte";
  import PollComposer from "./lib/components/PollComposer.svelte";
  import ArticleComposer from "./lib/components/ArticleComposer.svelte";
  import ArticleReader from "./lib/components/ArticleReader.svelte";
  import PhotoEditor from "./lib/components/PhotoEditor.svelte";
  import { loadReactionPack } from "./lib/reactionPack";

  let { chat }: { chat: string } = $props();

  onMount(() => {
    document.documentElement.classList.add("popout");
    document.body.classList.add("chat-open");
    initTheme();
    void loadReactionPack();
    const stop = initShortcuts();
    activeId.set(chat);
    void (async () => {
      try {
        const [i, c] = await Promise.all([api.initApp(), api.listChats()]);
        info.set(i);
        chats.set(c);
        await loadActiveMessages();
        const name = c.find((x) => x.id === chat)?.name;
        if (name) document.title = `${name} — Aquamarine`;
      } catch (e) { console.error(e); }
      void import("./lib/groups").then((m) => m.initGroups());
    })();
    const offs: Promise<() => void>[] = [
      events.onIncoming(async (p) => {
        chats.set(await api.listChats());
        if (p.peer_id === chat) await loadActiveMessages();
      }),
      events.onChatsUpdated(async () => { chats.set(await api.listChats()); }),
      listen<{ peer_id: string }>("aqua://own_sent", async (e) => { if (e.payload.peer_id === chat) await loadActiveMessages(); }),
      listen<{ peer_id: string }>("aqua://expired", async (e) => { if (e.payload.peer_id === chat) await loadActiveMessages(); }),
    ];
    // The main window keeps the connection: mirror its state.
    const poll = setInterval(() => { void api.initApp().then((i) => info.set(i)).catch(() => {}); }, 15_000);
    return () => {
      clearInterval(poll);
      stop?.();
      for (const o of offs) void o.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="root">
  <TitleBar />
  <div class="pop-main">
    <ChatWindow />
  </div>
</div>

<InputPrompt />
<MediaPicker />
<ProfileModal />
<PhotoEditor />
<ArticleComposer />
<ArticleReader />
<Lightbox />
<ForwardPicker />
<PollComposer />
<PackSheet />
<MiniApp />
<PlannerComposer />
<ChatCalendar />
<Toast />
<Confirm />
<SendDialog />

<style>
  .root { height: 100%; display: flex; flex-direction: column; min-height: 0; }
  .pop-main { flex: 1; min-height: 0; display: flex; }
  .pop-main :global(.active-chat) { flex: 1 1 auto; min-width: 0; }
  /* No "back to the list" in a window that is only this chat. */
  :global(html.popout .back-btn) { display: none !important; }
  :global(html.popout .chat-header) { padding-left: 14px !important; }
</style>
