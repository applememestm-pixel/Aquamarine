#!/usr/bin/env python3
"""Write addons.json for the site's add-on catalogue (pages.yml).

Authors publish a plugin by putting a .js file with a `// @name …` header in a
public GitHub repository and giving the repository the topic
`aquamarine-plugin`; plugins in this repository's addons/ folder are listed
too. Every run (each deploy and every 6 hours) picks up new ones and changes
— no pull requests to us needed.

Each entry: name, version, author, description, connect (hosts it asks for),
repo, path, url (raw file), page (on GitHub), stars, updated, official.

Usage: GITHUB_TOKEN=… addons_feed.py owner/repo out.json
"""

import json
import os
import re
import sys
import urllib.parse
import urllib.request

REPO, OUT = sys.argv[1], sys.argv[2]
TOKEN = os.environ.get("GITHUB_TOKEN", "")
TOPIC = "aquamarine-plugin"
MAX_SIZE = 256 * 1024
MAX_FILES_PER_REPO = 20
SKIP_DIRS = ("node_modules/", "dist/", "build/", ".github/", "test/", "tests/")


def get(url, accept="application/vnd.github+json"):
    req = urllib.request.Request(url, headers={"Accept": accept, "User-Agent": "aquamarine-pages"})
    if TOKEN and url.startswith("https://api.github.com/"):
        req.add_header("Authorization", f"Bearer {TOKEN}")
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def api(path):
    return json.loads(get(f"https://api.github.com/{path}"))


def meta(code):
    """The same header rules as the app (plugins.ts::parseMeta)."""
    def tag(k):
        m = re.search(rf"^\s*//\s*@{k}\s+(.+)$", code, re.M)
        return m.group(1).strip() if m else ""
    name = tag("name")
    if not name:
        return None
    connect = [h for h in re.split(r"[\s,]+", tag("connect").lower()) if h][:16]
    connect = [re.sub(r"/.*$", "", re.sub(r"^https?://", "", h)) for h in connect]
    return {
        "name": name[:80],
        "version": tag("version")[:20] or "1.0",
        "author": tag("author")[:80],
        "description": tag("description")[:400],
        "connect": connect,
    }


def plugins_in(full, branch, prefix=""):
    tree = api(f"repos/{full}/git/trees/{urllib.parse.quote(branch)}?recursive=1")
    files = [
        t for t in tree.get("tree", [])
        if t["type"] == "blob" and t["path"].endswith((".js", ".mjs")) and t["path"].startswith(prefix)
        and not any(d in "/" + t["path"] for d in ["/" + s for s in SKIP_DIRS]) and t.get("size", 0) <= MAX_SIZE
    ][:MAX_FILES_PER_REPO * 3]
    out = []
    for t in files:
        raw = f"https://raw.githubusercontent.com/{full}/{urllib.parse.quote(branch)}/{urllib.parse.quote(t['path'])}"
        try:
            code = get(raw, "text/plain").decode("utf-8", "replace")
        except Exception as e:
            print(f"{full}/{t['path']}: {e}", file=sys.stderr)
            continue
        m = meta(code[:4096])
        if not m:
            continue
        m.update(path=t["path"], url=raw, page=f"https://github.com/{full}/blob/{branch}/{t['path']}")
        out.append(m)
        if len(out) >= MAX_FILES_PER_REPO:
            break
    return out


def main():
    items = []
    # Ours first (addons/ in this repository).
    try:
        own = api(f"repos/{REPO}")
        for p in plugins_in(REPO, own["default_branch"], "addons/"):
            p.update(repo=REPO, stars=own.get("stargazers_count", 0), updated=own.get("pushed_at", ""), official=True)
            items.append(p)
    except Exception as e:
        print(f"{REPO}: {e}", file=sys.stderr)

    # Everyone else: public repositories with the topic.
    try:
        q = urllib.parse.quote(f"topic:{TOPIC} fork:false")
        found = api(f"search/repositories?q={q}&sort=updated&per_page=100").get("items", [])
    except Exception as e:
        print(f"search: {e}", file=sys.stderr)
        found = []
    for r in found:
        if r["full_name"].lower() == REPO.lower() or r.get("archived") or r.get("private"):
            continue
        try:
            for p in plugins_in(r["full_name"], r["default_branch"]):
                if not p["author"]:
                    p["author"] = r["owner"]["login"]
                p.update(repo=r["full_name"], stars=r.get("stargazers_count", 0), updated=r.get("pushed_at", ""), official=False)
                items.append(p)
        except Exception as e:
            print(f"{r['full_name']}: {e}", file=sys.stderr)

    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"topic": TOPIC, "items": items}, f, ensure_ascii=False, indent=1)
    print(f"{len(items)} add-ons from {len({i['repo'] for i in items})} repositories")


main()
