/**
 * Files in a paste: a copied picture or a file copied in Explorer / Finder
 * arrives as File items; a file copied in a Linux file manager arrives as
 * file:// URIs (text/uri-list, or GNOME's "copy\nfile:///…" text).
 */
export function clipboardFiles(dt: DataTransfer | null): { files: File[]; paths: string[] } {
  if (!dt) return { files: [], paths: [] };
  const files: File[] = [];
  for (const it of dt.items ?? []) {
    if (it.kind !== "file") continue;
    const f = it.getAsFile();
    if (f) files.push(f);
  }
  if (!files.length) for (const f of dt.files ?? []) files.push(f);
  const paths: string[] = [];
  if (!files.length) {
    const raw = dt.getData("text/uri-list") || dt.getData("text/plain") || "";
    const lines = raw.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith("#") && l !== "copy" && l !== "cut");
    // Only when every line is a file URI: a pasted link to a file stays text.
    if (lines.length && lines.every((l) => l.startsWith("file://"))) {
      for (const l of lines) {
        try {
          const u = new URL(l);
          let p = decodeURIComponent(u.pathname);
          if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1).replace(/\//g, "\\");  // file:///C:/…
          paths.push(p);
        } catch { /* not a URL */ }
      }
    }
  }
  return { files, paths };
}
