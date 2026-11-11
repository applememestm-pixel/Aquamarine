<script lang="ts">
  /** Authenticator apps people can install for two-step codes — tappable
   *  (the names in the text above weren't links). */
  import { IS_IOS, IS_ANDROID } from "../platform";
  const APPS = [
    { name: "Google Authenticator", android: "https://play.google.com/store/apps/details?id=com.google.android.apps.authenticator2", ios: "https://apps.apple.com/app/google-authenticator/id388497605", web: "https://support.google.com/accounts/answer/1066447" },
    { name: "Aegis", android: "https://play.google.com/store/apps/details?id=com.beemdevelopment.aegis", ios: "", web: "https://getaegis.app" },
    { name: "2FAS", android: "https://play.google.com/store/apps/details?id=com.twofasapp", ios: "https://apps.apple.com/app/2fa-authenticator-2fas/id1217793794", web: "https://2fas.com" },
    { name: "Bitwarden", android: "https://play.google.com/store/apps/details?id=com.x8bit.bitwarden", ios: "https://apps.apple.com/app/bitwarden-password-manager/id1137397744", web: "https://bitwarden.com/products/authenticator/" },
  ];
  const href = (a: (typeof APPS)[number]) => (IS_ANDROID && a.android) || (IS_IOS && a.ios) || a.web;
</script>

<div class="aa">
  {#each APPS as a (a.name)}
    <a class="aa-l" href={href(a)} target="_blank" rel="noopener">{a.name} ↗</a>
  {/each}
</div>

<style>
  .aa { display: flex; flex-wrap: wrap; gap: 6px; margin: 6px 0 2px; }
  .aa-l { font-size: 12.5px; font-weight: 600; color: var(--accent); text-decoration: none; padding: 4px 10px; border-radius: 999px; background: var(--accent-soft); border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent); }
  .aa-l:hover { text-decoration: underline; }
</style>
