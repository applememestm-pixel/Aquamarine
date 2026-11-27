#!/usr/bin/env python3
"""Write updates.json for the site (pages.yml).

The apps check for updates through GitHub's API first, but without a login it
allows 60 requests an hour per address, and phones on mobile networks share
one address with thousands of others: there it answers "403 Forbidden" for
hours, and github.com's own pages (the releases feed) do the same. GitHub
Pages sits behind a CDN with no such limit, so this file carries the same
answer:

  latest   — the highest numbered release, pre-releases included (testers;
             the old moving "nightly" is left out — no more nightly builds),
  stable   — the latest stable release (what /releases/latest gives),
  releases — the last 20 releases (the server app picks the highest version).

Each one is the API's release object (fields the apps read), plus `sums`, the
text of its SHA256SUMS.txt, and `builds`, its BUILDS.json — so the apps fetch
nothing else from GitHub before the download itself.

Only releases whose publication is finished are listed (review of 1.21.0-25,
27.11: the feed was built between SHA256SUMS.txt and its signature, and
listed the release with sums but no signature): SHA256SUMS.txt signed by the
release key (UPDATE_PUBKEY), naming this release (its VERSION line), every
file it lists attached, none of them newer than it, the signature not older
than it. Until then the feed keeps the previous ready release. pages.yml runs
again once the signature is up (release.yml, checksums job).

Usage: GITHUB_TOKEN=… UPDATE_PUBKEY=<hex> updates_feed.py owner/repo out.json
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.request

REPO = sys.argv[1] if __name__ == "__main__" else ""
TOKEN = os.environ.get("GITHUB_TOKEN", "")


def get(url, accept="application/vnd.github+json"):
    req = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "aquamarine-pages"})
    if TOKEN and url.startswith("https://api.github.com/"):
        req.add_header("Authorization", f"Bearer {TOKEN}")
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def api(path):
    return json.loads(get(f"https://api.github.com/repos/{REPO}/{path}"))


def asset_text(rel, name):
    a = next((a for a in rel.get("assets", []) if a["name"] == name), None)
    if not a:
        return None
    try:
        return get(a["browser_download_url"], "application/octet-stream").decode("utf-8", "replace")
    except Exception as e:  # a file still uploading: leave it out
        print(f"{rel['tag_name']}: {name}: {e}", file=sys.stderr)
        return None


cache = {}


def files(rel):
    """(sums, builds, sums_sig) of a release, fetched once."""
    if rel["id"] not in cache:
        sums = asset_text(rel, "SHA256SUMS.txt")
        sums_sig = asset_text(rel, "SHA256SUMS.txt.sig")
        builds = asset_text(rel, "BUILDS.json")
        try:
            builds = json.loads(builds) if builds else None
        except ValueError:
            builds = None
        cache[rel["id"]] = (sums, builds, sums_sig)
    return cache[rel["id"]]


def signed(text, sig_hex, pubkey_hex):
    """Ed25519 signature `sig_hex` by `pubkey_hex` over the exact text (openssl,
    as tools/release-sign.sh signs it)."""
    try:
        sig = bytes.fromhex(sig_hex.strip())
        der = bytes.fromhex("302a300506032b6570032100" + pubkey_hex.strip().lower())
    except ValueError:
        return False
    if len(sig) != 64 or len(der) != 44:
        return False
    with tempfile.TemporaryDirectory() as d:
        paths = {n: os.path.join(d, n) for n in ("pub.der", "sig.bin", "sums.txt")}
        open(paths["pub.der"], "wb").write(der)
        open(paths["sig.bin"], "wb").write(sig)
        open(paths["sums.txt"], "wb").write(text.encode("utf-8"))
        r = subprocess.run(["openssl", "pkeyutl", "-verify", "-pubin", "-keyform", "DER", "-inkey", paths["pub.der"],
                            "-rawin", "-in", paths["sums.txt"], "-sigfile", paths["sig.bin"]], capture_output=True)
        return r.returncode == 0


def ready(rel, sums, sig, pubkey):
    """Why `rel` can't be listed yet ("" — it can): what every app checks
    before it installs (aqua-core/src/release.rs), and that the upload is over."""
    v = version(rel)
    if not v:
        return "no version"
    if not sums or not sig:
        return "no signed SHA256SUMS.txt yet"
    if not signed(sums, sig, pubkey):
        return "SHA256SUMS.txt isn't signed by the release key"
    want = hashlib.sha256(v.encode()).hexdigest()
    listed = {}
    for line in sums.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            listed[parts[1].lstrip("*")] = parts[0].lower()
    if listed.get("VERSION") != want:
        return "SHA256SUMS.txt doesn't name this release (VERSION)"
    assets = {a["name"]: a for a in rel.get("assets", [])}
    missing = [n for n in listed if n not in assets]
    if missing:
        return "files listed but not attached: " + ", ".join(sorted(missing))
    sums_at = assets.get("SHA256SUMS.txt", {}).get("updated_at", "")
    later = [n for n in listed if assets[n].get("updated_at", "") > sums_at]
    if later:
        return "files changed after SHA256SUMS.txt: " + ", ".join(sorted(later))
    if assets.get("SHA256SUMS.txt.sig", {}).get("updated_at", "") < sums_at:
        return "the signature is older than SHA256SUMS.txt"
    return ""


def trim(rel, body=True):
    sums, builds, sums_sig = files(rel)
    out = {
        "tag_name": rel["tag_name"],
        "name": rel.get("name") or rel["tag_name"],
        "published_at": rel.get("published_at") or "",
        "html_url": rel["html_url"],
        "prerelease": rel.get("prerelease", False),
        "assets": [
            {"name": a["name"], "browser_download_url": a["browser_download_url"], "size": a["size"], "updated_at": a["updated_at"]}
            for a in rel.get("assets", [])
        ],
        "sums": sums,
        "sums_sig": sums_sig,
        "builds": builds,
    }
    if body:
        out["body"] = rel.get("body") or ""
    return out


def newer(a, b):
    """The apps' numbering (ui/src/lib/updates.ts::newer)."""
    import re
    sa, sb = re.split(r"[.-]", a), re.split(r"[.-]", b)
    num = lambda s: int(s) if s.isdigit() else 0
    for i in range(max(len(sa), len(sb))):
        x = sa[i] if i < len(sa) else "0"
        y = sb[i] if i < len(sb) else "0"
        if i == 2 and len(x) != len(y):
            short = min(len(x), len(y))
            if num(x[:short]) != num(y[:short]):
                return num(x[:short]) > num(y[:short])
            return len(x) > len(y)
        if num(x) != num(y):
            return num(x) > num(y)
    return False


def version(r):
    t = r.get("tag_name") or ""
    return t[1:] if t.startswith("v") and t[1:2].isdigit() else None


def choose(releases, stable, pubkey, files_of):
    """The feed from GitHub's answers: only ready releases; `latest` the
    highest of them, `stable` GitHub's latest when ready, else the highest
    ready release that isn't a pre-release."""
    ok = []
    for r in releases:
        if r.get("draft") or not version(r):
            continue
        sums, _, sig = files_of(r)
        why = ready(r, sums, sig, pubkey)
        if why:
            print(f"{r.get('tag_name')}: left out — {why}", file=sys.stderr)
            continue
        ok.append(r)
    latest = None
    for r in ok:
        if latest is None or newer(version(r), version(latest)):
            latest = r
    if stable is not None:
        sums, _, sig = files_of(stable)
        why = ready(stable, sums, sig, pubkey)
        if why:
            print(f"stable {stable.get('tag_name')}: left out — {why}", file=sys.stderr)
            stable = None
    if stable is None:
        for r in ok:
            if not r.get("prerelease") and (stable is None or newer(version(r), version(stable))):
                stable = r
    return latest, stable, ok


def main():
    out = sys.argv[2]
    pubkey = os.environ.get("UPDATE_PUBKEY", "").strip().lower()
    if len(pubkey) != 64:
        sys.exit("UPDATE_PUBKEY (the release key, 64 hex digits) is not set: no feed without it")
    try:
        stable = api("releases/latest")
    except Exception:
        stable = None
    latest, stable, releases = choose(api("releases?per_page=20"), stable, pubkey, files)
    feed = {
        "latest": trim(latest) if latest else None,
        "stable": trim(stable) if stable else None,
        "releases": [trim(r, body=False) for r in releases],
    }
    with open(out, "w", encoding="utf-8") as f:
        json.dump(feed, f, ensure_ascii=False, separators=(",", ":"))
    print(f"updates.json: latest {feed['latest'] and feed['latest']['tag_name']}, stable {feed['stable'] and feed['stable']['tag_name']}, {len(releases)} releases")


if __name__ == "__main__":
    main()
