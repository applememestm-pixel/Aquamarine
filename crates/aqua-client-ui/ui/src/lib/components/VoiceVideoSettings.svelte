<script lang="ts">
  import Select from "./Select.svelte";
  import { t } from "../i18n";
  import { onMount, onDestroy } from "svelte";
  import type { Action } from "svelte/action";
  import { callSettings, patchCallSettings, keyLabel, type CallSettings } from "../callSettings";
  import { RES_LABEL, targetVideoKbps, micConstraints, cameraConstraints, type ResPreset, type FpsPreset, type CodecPref } from "../rtcQuality";
  import { showToast } from "./Toast.svelte";

  let mics = $state<MediaDeviceInfo[]>([]);
  let speakers = $state<MediaDeviceInfo[]>([]);
  let cams = $state<MediaDeviceInfo[]>([]);
  let labelled = $state(false);

  async function refreshDevices() {
    try {
      const d = await navigator.mediaDevices.enumerateDevices();
      mics = d.filter((x) => x.kind === "audioinput" && x.deviceId);
      speakers = d.filter((x) => x.kind === "audiooutput" && x.deviceId);
      cams = d.filter((x) => x.kind === "videoinput" && x.deviceId);
      labelled = d.some((x) => !!x.label);
    } catch { /* no mediaDevices */ }
  }

  async function grantAccess() {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: true, video: true })
        .catch(() => navigator.mediaDevices.getUserMedia({ audio: true }));
      s.getTracks().forEach((t) => t.stop());
    } catch (e) {
      showToast(`Access denied: ${e}`);
    }
    await refreshDevices();
  }

  function devLabel(d: MediaDeviceInfo, i: number, kind: string): string {
    if (d.deviceId === "default") return d.label ? d.label : "System default";
    return d.label || `${kind} ${i + 1}`;
  }

  // ---- Mic test ----
  let testing = $state(false);
  let loopback = $state(false);
  let levelDb = $state(-100);
  let tStream: MediaStream | null = null;
  let tCtx: AudioContext | null = null;
  let tGain: GainNode | null = null;
  let tMonitor: GainNode | null = null;
  let tRaf = 0;

  async function startTest() {
    stopTest();
    try {
      tStream = await navigator.mediaDevices.getUserMedia({ audio: micConstraints($callSettings) });
      tCtx = new AudioContext();
      const src = tCtx.createMediaStreamSource(tStream);
      tGain = tCtx.createGain();
      tGain.gain.value = $callSettings.inputVolume;
      tMonitor = tCtx.createGain();
      tMonitor.gain.value = loopback ? 1 : 0;
      const an = tCtx.createAnalyser();
      an.fftSize = 1024;
      const buf = new Float32Array(an.fftSize);
      src.connect(tGain);
      tGain.connect(an);
      tGain.connect(tMonitor);
      tMonitor.connect(tCtx.destination);
      const sink = $callSettings.speakerId !== "default" ? $callSettings.speakerId : "";
      const ctxAny = tCtx as AudioContext & { setSinkId?: (id: string) => Promise<void> };
      if (sink && ctxAny.setSinkId) await ctxAny.setSinkId(sink).catch(() => {});
      testing = true;
      const loop = () => {
        an.getFloatTimeDomainData(buf);
        let sum = 0;
        for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
        const db = 20 * Math.log10(Math.sqrt(sum / buf.length) + 1e-9);
        levelDb = Math.max(db, levelDb - 1.5); // fast attack, slow release
        tRaf = requestAnimationFrame(loop);
      };
      loop();
    } catch (e) {
      showToast(`Mic test failed: ${e}`);
      stopTest();
    }
  }

  function stopTest() {
    cancelAnimationFrame(tRaf);
    tStream?.getTracks().forEach((t) => t.stop());
    tStream = null;
    if (tCtx) void tCtx.close().catch(() => {});
    tCtx = null; tGain = null; tMonitor = null;
    testing = false;
    levelDb = -100;
  }

  $effect(() => { if (tGain) tGain.gain.value = $callSettings.inputVolume; });
  $effect(() => { if (tMonitor) tMonitor.gain.value = loopback ? 1 : 0; });

  // Meter: -80 dB … 0 dB → 0 … 100 %
  const pct = (db: number) => Math.max(0, Math.min(100, ((db + 80) / 80) * 100));

  // ---- Camera preview ----
  let preview = $state.raw<MediaStream | null>(null);
  async function togglePreview() {
    if (preview) { preview.getTracks().forEach((t) => t.stop()); preview = null; return; }
    try {
      preview = await navigator.mediaDevices.getUserMedia({ video: cameraConstraints($callSettings.camId, $callSettings.camRes) });
    } catch (e) { showToast(`Camera unavailable: ${e}`); }
  }
  const srcObject: Action<HTMLVideoElement, MediaStream | null> = (node, s) => {
    node.srcObject = s ?? null;
    return { update: (n) => { node.srcObject = n ?? null; } };
  };

  // ---- Push-to-talk binding ----
  let capturing = $state(false);
  function onCaptureKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code !== "Escape") patchCallSettings({ pttKey: e.code });
    capturing = false;
  }
  function onCaptureMouse(e: MouseEvent) {
    if (!capturing || (e.button !== 3 && e.button !== 4)) return;
    e.preventDefault();
    patchCallSettings({ pttKey: e.button === 3 ? "Mouse3" : "Mouse4" });
    capturing = false;
  }

  onMount(() => {
    void refreshDevices();
    navigator.mediaDevices?.addEventListener?.("devicechange", refreshDevices);
    window.addEventListener("keydown", onCaptureKey, true);
    window.addEventListener("mousedown", onCaptureMouse, true);
  });
  onDestroy(() => {
    stopTest();
    preview?.getTracks().forEach((t) => t.stop());
    navigator.mediaDevices?.removeEventListener?.("devicechange", refreshDevices);
    window.removeEventListener("keydown", onCaptureKey, true);
    window.removeEventListener("mousedown", onCaptureMouse, true);
  });

  function set<K extends keyof CallSettings>(k: K, v: CallSettings[K]) {
    patchCallSettings({ [k]: v } as Partial<CallSettings>);
  }

  const RESES: ResPreset[] = ["720", "1080", "1440", "source"];
  const FPSES: FpsPreset[] = [15, 30, 60];
  const CODECS: { id: CodecPref; label: string }[] = [
    { id: "auto", label: "Auto" }, { id: "h264", label: "H.264" }, { id: "vp9", label: "VP9" },
    { id: "av1", label: "AV1" }, { id: "vp8", label: "VP8" },
  ];
  const CAPS = [0, 4000, 8000, 15000, 25000];
  const VOICE = [64, 96, 128, 256];
</script>

<section class="group">
  <div class="label">{$t("Voice")}</div>
  {#if !labelled}
    <div class="note">
      {$t("Device names are hidden until Aquamarine is allowed to use the microphone.")}
      <button class="link" onclick={grantAccess}>{$t("Allow access")}</button>
    </div>
  {/if}
  <div class="row">
    <span class="row-label">{$t("Input")}</span>
    <Select class="sel" value={$callSettings.micId}
      options={[...(mics.some((d) => d.deviceId === "default") ? [] : [{ value: "default", label: $t("System default") }]), ...mics.map((d, i) => ({ value: d.deviceId, label: devLabel(d, i, "Microphone") }))]}
      onchange={(v) => { set("micId", v); if (testing) void startTest(); }} />
  </div>
  <div class="row">
    <span class="row-label">{$t("Output")}</span>
    <Select class="sel" value={$callSettings.speakerId}
      options={[...(speakers.some((d) => d.deviceId === "default") ? [] : [{ value: "default", label: $t("System default") }]), ...speakers.map((d, i) => ({ value: d.deviceId, label: devLabel(d, i, "Speaker") }))]}
      onchange={(v) => set("speakerId", v)} />
  </div>
  <div class="row">
    <span class="row-label">{$t("Input volume")}</span>
    <div class="range">
      <input type="range" min="0" max="2" step="0.05" value={$callSettings.inputVolume}
        oninput={(e) => set("inputVolume", parseFloat((e.target as HTMLInputElement).value))} />
      <span class="range-val">{Math.round($callSettings.inputVolume * 100)}%</span>
    </div>
  </div>
  <div class="row">
    <span class="row-label">{$t("Output volume")}</span>
    <div class="range">
      <input type="range" min="0" max="1" step="0.01" value={$callSettings.outputVolume}
        oninput={(e) => set("outputVolume", parseFloat((e.target as HTMLInputElement).value))} />
      <span class="range-val">{Math.round($callSettings.outputVolume * 100)}%</span>
    </div>
  </div>

  <div class="mictest">
    <button class="btn" class:active={testing} onclick={() => (testing ? stopTest() : startTest())}>
      {$t(testing ? "Stop test" : "Let's check")}
    </button>
    <div class="meter" title={`${Math.round(levelDb)} dB`}>
      <div class="meter-fill" class:gated={$callSettings.gateDb > -99 && levelDb < $callSettings.gateDb} style:width="{pct(levelDb)}%"></div>
      {#if $callSettings.gateDb > -99}<div class="meter-mark" style:left="{pct($callSettings.gateDb)}%"></div>{/if}
    </div>
    <label class="chk"><input type="checkbox" bind:checked={loopback} /> {$t("Hear myself")}</label>
  </div>
</section>

<section class="group">
  <div class="label">{$t("Input sensitivity")}</div>
  <button class="toggle-row" onclick={() => set("gateDb", $callSettings.gateDb > -99 ? -100 : -50)}>
    <div class="tr-text">
      <div class="tr-title">{$t("Noise gate")}</div>
      <div class="tr-sub">{$t("Silence the mic below a level — keyboard and fan noise never reach your contact")}</div>
    </div>
    <span class="switch" class:on={$callSettings.gateDb > -99}></span>
  </button>
  {#if $callSettings.gateDb > -99}
    <div class="row">
      <span class="row-label">{$t("Threshold")}</span>
      <div class="range">
        <input type="range" min="-80" max="-10" step="1" value={$callSettings.gateDb}
          oninput={(e) => set("gateDb", parseFloat((e.target as HTMLInputElement).value))} />
        <span class="range-val">{$callSettings.gateDb} dB</span>
      </div>
    </div>
    <div class="note small">{$t("Use \"Let's check\" above — the bar turns grey while you're below the threshold.")}</div>
  {/if}
</section>

<section class="group">
  <div class="label">{$t("Voice processing")}</div>
  <button class="toggle-row" onclick={() => set("echoCancellation", !$callSettings.echoCancellation)}>
    <div class="tr-text"><div class="tr-title">{$t("Echo cancellation")}</div><div class="tr-sub">{$t("Needed when you use speakers instead of headphones")}</div></div>
    <span class="switch" class:on={$callSettings.echoCancellation}></span>
  </button>
  <button class="toggle-row" onclick={() => set("noiseSuppression", !$callSettings.noiseSuppression)}>
    <div class="tr-text"><div class="tr-title">{$t("Noise suppression")}</div><div class="tr-sub">{$t("Filters steady background noise")}</div></div>
    <span class="switch" class:on={$callSettings.noiseSuppression}></span>
  </button>
  <button class="toggle-row" onclick={() => set("autoGainControl", !$callSettings.autoGainControl)}>
    <div class="tr-text"><div class="tr-title">{$t("Automatic gain control")}</div><div class="tr-sub">{$t("Keeps your voice at a steady loudness")}</div></div>
    <span class="switch" class:on={$callSettings.autoGainControl}></span>
  </button>
  <div class="note small">{$t("Changes apply from the next call.")}</div>
</section>

<section class="group">
  <div class="label">{$t("Input mode")}</div>
  <div class="row">
    <span class="row-label">{$t("Mode")}</span>
    <div class="seg">
      <button class="seg-opt" class:active={!$callSettings.pushToTalk} onclick={() => set("pushToTalk", false)}>{$t("Voice activity")}</button>
      <button class="seg-opt" class:active={$callSettings.pushToTalk} onclick={() => set("pushToTalk", true)}>{$t("Push to talk")}</button>
    </div>
  </div>
  {#if $callSettings.pushToTalk}
    <div class="row">
      <span class="row-label">{$t("Key")}</span>
      <button class="btn keybind" class:active={capturing} onclick={() => (capturing = true)}>
        {capturing ? "Press a key or mouse side button…" : keyLabel($callSettings.pttKey)}
      </button>
    </div>
    <div class="note small">{$t("Works while the Aquamarine window is focused. Ctrl+Shift+M mutes, Ctrl+Shift+D deafens.")}</div>
  {/if}
</section>

<section class="group">
  <div class="label">{$t("Voice quality")}</div>
  <div class="row">
    <span class="row-label">{$t("Bitrate")}</span>
    <div class="seg">
      {#each VOICE as k}
        <button class="seg-opt" class:active={$callSettings.voiceKbps === k} onclick={() => set("voiceKbps", k)}>{k} kbps</button>
      {/each}
    </div>
  </div>
  <div class="note small">{$t("Opus with packet-loss recovery. 64 kbps is Discord's default, 96 kbps sounds noticeably fuller.")}</div>
</section>

<section class="group">
  <div class="label">{$t("Camera")}</div>
  <div class="row">
    <span class="row-label">{$t("Device")}</span>
    <Select class="sel" value={$callSettings.camId}
      options={[...(cams.some((d) => d.deviceId === "default") ? [] : [{ value: "default", label: $t("System default") }]), ...cams.map((d, i) => ({ value: d.deviceId, label: devLabel(d, i, "Camera") }))]}
      onchange={(v) => set("camId", v)} />
  </div>
  <div class="row">
    <span class="row-label">{$t("Resolution")}</span>
    <div class="seg">
      {#each (["720", "1080"] as const) as r}
        <button class="seg-opt" class:active={$callSettings.camRes === r} onclick={() => set("camRes", r)}>{r}p</button>
      {/each}
    </div>
    <button class="btn" class:active={!!preview} onclick={togglePreview}>{$t(preview ? "Stop preview" : "Preview")}</button>
  </div>
  {#if preview}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video class="preview" use:srcObject={preview} autoplay playsinline muted data-call-media></video>
  {/if}
</section>

<section class="group">
  <div class="label">{$t("Screen share (Go Live defaults)")}</div>
  <div class="row">
    <span class="row-label">{$t("Resolution")}</span>
    <div class="seg">
      {#each RESES as r}
        <button class="seg-opt" class:active={$callSettings.screenRes === r} onclick={() => set("screenRes", r)}>{RES_LABEL[r]}</button>
      {/each}
    </div>
  </div>
  <div class="row">
    <span class="row-label">{$t("Frame rate")}</span>
    <div class="seg">
      {#each FPSES as f}
        <button class="seg-opt" class:active={$callSettings.screenFps === f} onclick={() => set("screenFps", f)}>{f} fps</button>
      {/each}
    </div>
  </div>
  <div class="row">
    <span class="row-label">{$t("Optimize")}</span>
    <div class="seg">
      <button class="seg-opt" class:active={$callSettings.screenHint === "motion"} onclick={() => set("screenHint", "motion")}>{$t("Motion")}</button>
      <button class="seg-opt" class:active={$callSettings.screenHint === "detail"} onclick={() => set("screenHint", "detail")}>Text &amp; detail</button>
    </div>
  </div>
  <button class="toggle-row" onclick={() => set("screenAudio", !$callSettings.screenAudio)}>
    <div class="tr-text"><div class="tr-title">{$t("Share system audio")}</div><div class="tr-sub">{$t("Game / video sound goes with the stream (whole-screen shares on Windows)")}</div></div>
    <span class="switch" class:on={$callSettings.screenAudio}></span>
  </button>
  <div class="note small">
    {RES_LABEL[$callSettings.screenRes]} at {$callSettings.screenFps} fps streams at up to
    {(targetVideoKbps($callSettings.screenRes, $callSettings.screenFps, $callSettings.maxVideoKbps) / 1000).toFixed(1)} Mbps.
    "Motion" keeps the frame rate smooth for games; "Text &amp; detail" keeps code and documents sharp.
  </div>
</section>

<section class="group">
  <div class="label">{$t("Encoding")}</div>
  <div class="row">
    <span class="row-label">{$t("Codec")}</span>
    <div class="seg">
      {#each CODECS as c}
        <button class="seg-opt" class:active={$callSettings.codec === c.id} onclick={() => set("codec", c.id)}>{c.label}</button>
      {/each}
    </div>
  </div>
  <div class="row">
    <span class="row-label">{$t("Max bitrate")}</span>
    <div class="seg">
      {#each CAPS as k}
        <button class="seg-opt" class:active={$callSettings.maxVideoKbps === k} onclick={() => set("maxVideoKbps", k)}>{k === 0 ? "Auto" : `${k / 1000} Mbps`}</button>
      {/each}
    </div>
  </div>
  <div class="note small">
    {$t("Auto prefers H.264, which most GPUs encode in hardware — smooth 1080p60 without loading the CPU. Lower the max bitrate if your upload is slow. Codec changes apply from the next call.")}
  </div>
</section>

<section class="group">
  <div class="label">{$t("Connectivity")}</div>
  <div class="note small">
    {$t("Calls go peer-to-peer. If two people behind strict NAT (mobile internet, some ISPs) can't connect, add a TURN relay (e.g. coturn on your server).")}
  </div>
  <div class="row">
    <span class="row-label">{$t("TURN URL")}</span>
    <input class="txt" placeholder={$t("turn:your-server:3478")} value={$callSettings.turnUrl}
      onchange={(e) => set("turnUrl", (e.target as HTMLInputElement).value.trim())} />
  </div>
  <div class="row">
    <span class="row-label">{$t("Username")}</span>
    <input class="txt" value={$callSettings.turnUser} onchange={(e) => set("turnUser", (e.target as HTMLInputElement).value)} />
  </div>
  <div class="row">
    <span class="row-label">{$t("Password")}</span>
    <input class="txt" type="password" value={$callSettings.turnPass} onchange={(e) => set("turnPass", (e.target as HTMLInputElement).value)} />
  </div>
</section>

<section class="group">
  <div class="label">Call sounds &amp; info</div>
  <button class="toggle-row" onclick={() => set("ringtone", !$callSettings.ringtone)}>
    <div class="tr-text"><div class="tr-title">{$t("Ringtone")}</div><div class="tr-sub">{$t("Ring on incoming calls, ringback while calling")}</div></div>
    <span class="switch" class:on={$callSettings.ringtone}></span>
  </button>
  <button class="toggle-row" onclick={() => set("showStats", !$callSettings.showStats)}>
    <div class="tr-text"><div class="tr-title">{$t("Show connection info")}</div><div class="tr-sub">{$t("Resolution, fps, bitrate, codec, ping and packet loss during calls")}</div></div>
    <span class="switch" class:on={$callSettings.showStats}></span>
  </button>
</section>

<style>
  /* Same grouped-card look as the rest of Settings. */
  .group { margin: 12px 16px 0; padding: 12px 16px 14px; background: var(--bg-surface); border-radius: calc(14px * var(--rk, 1)); }
  .label { font-size: 12px; font-weight: 700; color: var(--accent); margin-bottom: 10px; }
  .row { display: flex; align-items: center; gap: 14px; margin-bottom: 10px; flex-wrap: wrap; }
  .row-label { flex: 0 0 110px; color: var(--fg-muted); font-size: 13px; }
  .seg { display: flex; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); padding: 3px; gap: 2px; flex-wrap: wrap; }
  .seg-opt {
    padding: 5px 11px; background: transparent; border: none; border-radius: calc(7px * var(--rk, 1));
    color: var(--fg-muted); font-size: 12.5px; font-weight: 600; cursor: pointer; white-space: nowrap;
  }
  .seg-opt:hover { color: var(--fg); }
  .seg-opt.active { background: var(--accent-soft); color: var(--accent); }
  .range { display: flex; align-items: center; gap: 10px; flex: 1; }
  .range input[type="range"] { flex: 1; accent-color: var(--accent); }
  .range-val { min-width: 48px; text-align: right; font-variant-numeric: tabular-nums; color: var(--fg-muted); font-size: 12px; }
  .sel, .txt {
    flex: 1; min-width: 0;
    background: var(--bg-input); color: var(--fg);
    border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1));
    padding: 7px 9px; font-size: 13px; font-family: inherit;
  }
  .sel:focus, .txt:focus { outline: none; border-color: var(--accent); }
  .btn {
    background: var(--bg-input); color: var(--fg);
    border: 1px solid var(--border-strong); border-radius: calc(8px * var(--rk, 1));
    padding: 6px 12px; font-size: 12.5px; font-weight: 600; cursor: pointer; white-space: nowrap;
  }
  .btn:hover { border-color: var(--accent); }
  .btn.active { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
  .keybind { min-width: 120px; font-family: var(--font-mono, monospace); }
  .link { background: none; border: none; color: var(--accent); cursor: pointer; font-weight: 600; padding: 0; font-size: inherit; }
  .note { font-size: 12.5px; color: var(--fg-muted); margin-bottom: 10px; line-height: 1.5; }
  .note.small { font-size: 11.5px; color: var(--fg-dim); margin: 2px 0 6px; }

  .mictest { display: flex; align-items: center; gap: 12px; margin-top: 4px; }
  .meter { position: relative; flex: 1; height: 10px; border-radius: calc(5px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); overflow: hidden; }
  .meter-fill { height: 100%; background: linear-gradient(90deg, #3BA55D, #FAA61A 80%, #ED4245); transition: width 0.05s linear; }
  .meter-fill.gated { background: var(--fg-dim); opacity: 0.5; }
  .meter-mark { position: absolute; top: -2px; bottom: -2px; width: 2px; background: var(--fg); }
  .chk { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--fg-muted); white-space: nowrap; cursor: pointer; }
  .chk input { accent-color: var(--accent); }

  .toggle-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    width: 100%; padding: 9px 0; background: transparent; border: none; cursor: pointer;
    text-align: left; color: inherit; border-bottom: 1px solid var(--border);
  }
  .toggle-row:last-child { border-bottom: none; }
  .tr-text { min-width: 0; }
  .tr-title { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .tr-sub { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; }
  .switch {
    flex-shrink: 0; width: 40px; height: 23px; border-radius: 100px;
    background: var(--bg-input); border: 1px solid var(--border-strong); position: relative;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .switch::after {
    content: ''; position: absolute; top: 2px; left: 2px; width: 17px; height: 17px; border-radius: 50%;
    background: var(--fg-dim); transition: transform 0.15s ease, background 0.15s ease;
  }
  .switch.on { background: var(--accent-soft); border-color: var(--accent); }
  .switch.on::after { transform: translateX(17px); background: var(--accent); }
  .preview { width: 100%; max-width: 360px; border-radius: calc(10px * var(--rk, 1)); background: #000; transform: scaleX(-1); margin-top: 4px; }
</style>
