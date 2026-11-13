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

Usage: GITHUB_TOKEN=… updates_feed.py owner/repo out.json
"""

import json
import os
import sys
import urllib.request

REPO, OUT = sys.argv[1], sys.argv[2]
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


def trim(rel, body=True):
    if rel["id"] not in cache:
        sums = asset_text(rel, "SHA256SUMS.txt")
        builds = asset_text(rel, "BUILDS.json")
        try:
            builds = json.loads(builds) if builds else None
        except ValueError:
            builds = None
        cache[rel["id"]] = (sums, builds)
    sums, builds = cache[rel["id"]]
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


releases = [r for r in api("releases?per_page=20") if not r.get("draft") and version(r)]
latest = None
for r in releases:
    if latest is None or newer(version(r), version(latest)):
        latest = r
try:
    stable = api("releases/latest")
except Exception:
    stable = None

feed = {
    "latest": trim(latest) if latest else None,
    "stable": trim(stable) if stable else None,
    "releases": [trim(r, body=False) for r in releases],
}
with open(OUT, "w", encoding="utf-8") as f:
    json.dump(feed, f, ensure_ascii=False, separators=(",", ":"))
print(f"updates.json: latest {feed['latest'] and feed['latest']['tag_name']}, stable {feed['stable'] and feed['stable']['tag_name']}, {len(releases)} releases")
