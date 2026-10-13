<script module lang="ts">
  /** Parsed Lottie JSON per URL — a pack's emoji are reused all over the UI. */
  const lottieCache = new Map<string, Promise<unknown>>();
  interface LottiePlayer {
    loadAnimation(opts: Record<string, unknown>): { play(): void; pause(): void; goToAndStop(v: number, f?: boolean): void; destroy(): void };
  }
  let lottieLib: Promise<LottiePlayer> | null = null;

  /** The Lottie player is vendored as a plain script (public/vendor) and
   *  loaded on first use — no npm dependency, so the build never needs
   *  `npm install`. */
  export function loadLottie(): Promise<LottiePlayer> {
    lottieLib ??= new Promise<LottiePlayer>((resolve, reject) => {
      const w = window as unknown as { lottie?: LottiePlayer };
      if (w.lottie) return resolve(w.lottie);
      const s = document.createElement("script");
      s.src = "/vendor/lottie_light_canvas.min.js";
      s.async = true;
      s.onload = () => (w.lottie ? resolve(w.lottie) : reject(new Error("lottie missing")));
      s.onerror = () => { lottieLib = null; reject(new Error("could not load lottie")); };
      document.head.appendChild(s);
    });
    return lottieLib;
  }

  /** Telegram .tgs = gzipped Lottie JSON. */
  export function loadTgs(url: string): Promise<unknown> {
    let p = lottieCache.get(url);
    if (!p) {
      p = (async () => {
        const res = await fetch(url);
        const buf = new Uint8Array(await res.arrayBuffer());
        let text: string;
        if (buf[0] === 0x1f && buf[1] === 0x8b) {
          const stream = new Blob([buf]).stream().pipeThrough(new DecompressionStream("gzip"));
          text = await new Response(stream).text();
        } else {
          text = new TextDecoder().decode(buf);
        }
        return JSON.parse(text);
      })();
      lottieCache.set(url, p);
      p.catch(() => lottieCache.delete(url));
    }
    return p;
  }

  const snapCache = new Map<string, Promise<string>>();
  /** A still PNG of a .tgs's first frame — for places that can't host a
   *  live player (the rich text composer). */
  export function tgsSnapshot(url: string, size = 64): Promise<string> {
    let p = snapCache.get(url);
    if (!p) {
      p = (async () => {
        const [lottie, data] = await Promise.all([loadLottie(), loadTgs(url)]);
        const host = document.createElement("div");
        host.style.cssText = `position:fixed;left:-10000px;top:0;width:${size}px;height:${size}px;`;
        document.body.appendChild(host);
        try {
          const a = lottie.loadAnimation({ container: host, renderer: "canvas", loop: false, autoplay: false, animationData: data, rendererSettings: { clearCanvas: true } });
          a.goToAndStop(0, true);
          const c = host.querySelector("canvas");
          const out = c ? c.toDataURL("image/png") : "";
          a.destroy();
          return out;
        } finally {
          host.remove();
        }
      })();
      snapCache.set(url, p);
      p.catch(() => snapCache.delete(url));
    }
    return p;
  }

  export function mediaKind(src: string, mime?: string | null): "tgs" | "video" | "image" {
    const m = (mime ?? "").toLowerCase();
    const s = src.toLowerCase().split("?")[0];
    if (m === "application/x-tgsticker" || s.endsWith(".tgs")) return "tgs";
    if (m.startsWith("video/") || s.endsWith(".webm") || s.endsWith(".mp4")) return "video";
    return "image";
  }
</script>

<script lang="ts">
  import { mediaSupport } from "../media";
  /**
   * One element for every kind of sticker / emoji / GIF we show: static
   * images, Telegram animated stickers (.tgs → Lottie on a canvas) and video
   * stickers / GIFs (.webm / .mp4, muted loop).
   *
   * play: "auto"  — loop while on screen
   *       "hover" — first frame; plays while hovered (picker grids)
   *       "never" — first frame only
   */
  import { onMount } from "svelte";

  interface Props {
    src: string;
    mime?: string | null;
    alt?: string;
    play?: "auto" | "hover" | "never";
    fit?: "contain" | "cover";
    class?: string;
  }
  let { src, mime = null, alt = "", play = "auto", fit = "contain", class: klass = "" }: Props = $props();

  const kind = $derived(mediaKind(src, mime));
  let box: HTMLDivElement | undefined = $state();
  let video: HTMLVideoElement | undefined = $state();
  // Linux without gst-plugins-good: any <video> hangs WebKitGTK — show the
  // fallback instead (lib/media.ts::mediaSupport).
  let videoOk = $state(false);
  $effect(() => {
    if (kind !== "video") return;
    void mediaSupport().then((m) => { if (m.blocking.length) failed = true; else videoOk = true; });
  });
  let failed = $state(false);
  let visible = false;
  let hovering = false;
  let anim: { play(): void; pause(): void; goToAndStop(v: number, f?: boolean): void; destroy(): void } | null = null;

  function shouldPlay() {
    return play === "auto" ? visible : play === "hover" ? hovering : false;
  }
  function sync() {
    const on = shouldPlay();
    if (anim) { if (on) anim.play(); else if (play !== "auto") anim.goToAndStop(0, true); else anim.pause(); }
    if (video) { if (on) video.play().catch(() => {}); else { video.pause(); if (play !== "auto") video.currentTime = 0; } }
  }

  // (Re)build the Lottie player whenever the source changes.
  $effect(() => {
    const url = src;
    if (kind !== "tgs" || !box) return;
    let cancelled = false;
    failed = false;
    (async () => {
      try {
        const [lottie, data] = await Promise.all([loadLottie(), loadTgs(url)]);
        if (cancelled || !box) return;
        anim?.destroy();
        anim = lottie.loadAnimation({
          container: box,
          renderer: "canvas",
          loop: true,
          autoplay: false,
          animationData: data,
          rendererSettings: { preserveAspectRatio: "xMidYMid meet", clearCanvas: true },
        });
        anim.goToAndStop(0, true);
        sync();
      } catch (e) {
        console.warn("tgs", url, e);
        failed = true;
      }
    })();
    return () => { cancelled = true; anim?.destroy(); anim = null; };
  });

  onMount(() => {
    if (play !== "auto" || !box) return;
    const io = new IntersectionObserver((ents) => {
      visible = ents.some((e) => e.isIntersecting);
      sync();
    });
    io.observe(box);
    return () => io.disconnect();
  });
</script>

<div
  bind:this={box}
  class="am {klass}"
  class:cover={fit === "cover"}
  role="img"
  aria-label={alt}
  onmouseenter={() => { hovering = true; if (play === "hover") sync(); }}
  onmouseleave={() => { hovering = false; if (play === "hover") sync(); }}
>
  {#if kind === "image" || failed}
    {#if !failed}<img {src} {alt} draggable="false" loading="lazy" />{:else}<span class="fallback">{alt}</span>{/if}
  {:else if kind === "video" && videoOk}
    <video bind:this={video} {src} muted loop playsinline preload="metadata" autoplay={play === "auto"}
      onloadeddata={() => sync()}></video>
  {/if}
</div>

<style>
  .am { position: relative; width: 100%; height: 100%; display: grid; place-items: center; overflow: hidden; }
  .am img, .am video { width: 100%; height: 100%; object-fit: contain; display: block; }
  .am.cover img, .am.cover video { object-fit: cover; }
  .am :global(canvas) { width: 100% !important; height: 100% !important; }
  .fallback { font-size: 1.2em; line-height: 1; }
</style>
