#!/usr/bin/env python3
"""Test of tools/updates_feed.py (review of 1.21.0-25, 27.11): a release is
listed only once its publication is finished — signed sums naming it, every
file attached and not newer than the sums. A key of its own; openssl signs as
tools/release-sign.sh does. Run: python3 tools/updates_feed_test.py"""

import hashlib
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import updates_feed as uf  # noqa: E402

d = tempfile.mkdtemp()
key = os.path.join(d, "k.pem")
subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", key], check=True, capture_output=True)
der = subprocess.run(["openssl", "pkey", "-in", key, "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout
PUB = der[-32:].hex()


def sign(text):
    p = os.path.join(d, "s.txt")
    open(p, "w").write(text)
    return subprocess.run(["openssl", "pkeyutl", "-sign", "-inkey", key, "-rawin", "-in", p], check=True, capture_output=True).stdout.hex()


def release(tag, *, n, sig=True, at="2026-11-27T06:00:00Z", sums_at="2026-11-27T06:10:00Z", sig_at="2026-11-27T06:10:01Z", pre=True, extra=(), version=None):
    v = version or tag[1:]
    sums = f"{'a' * 64}  Aquamarine.apk\n{hashlib.sha256(v.encode()).hexdigest()}  VERSION\n" + "".join(f"{'b' * 64}  {x}\n" for x in extra)
    assets = [{"name": "Aquamarine.apk", "updated_at": at, "size": 1, "browser_download_url": "u"},
              {"name": "VERSION", "updated_at": sums_at, "size": 1, "browser_download_url": "u"},
              {"name": "SHA256SUMS.txt", "updated_at": sums_at, "size": 1, "browser_download_url": "u"}]
    if sig:
        assets.append({"name": "SHA256SUMS.txt.sig", "updated_at": sig_at, "size": 1, "browser_download_url": "u"})
    r = {"id": n, "tag_name": tag, "prerelease": pre, "assets": assets, "html_url": "h"}
    return r, (sums, None, sign(sums) if sig else None)


cases = {}


def add(r_files):
    r, f = r_files
    cases[r["id"]] = f
    return r


files_of = lambda r: cases[r["id"]]  # noqa: E731
fails = 0


def check(what, got, want):
    global fails
    ok = got == want
    fails += not ok
    print(("ok: " if ok else "FAIL: ") + what + ("" if ok else f" (got {got!r}, want {want!r})"))


old = add(release("v1.21.0-24", n=24))
stable_old = add(release("v1.21.0", n=21, pre=False))

# Mid-publication (what happened to -25): sums up, signature not yet.
mid = add(release("v1.21.0-25", n=25, sig=False))
latest, stable, ok = uf.choose([mid, old, stable_old], stable_old, PUB, files_of)
check("mid-publication: the previous ready release stays latest", latest["tag_name"], "v1.21.0-24")
check("mid-publication: not listed at all", [r["tag_name"] for r in ok], ["v1.21.0-24", "v1.21.0"])

# Signed, finished: listed.
done = add(release("v1.21.0-25", n=125))
latest, _, ok = uf.choose([done, old], None, PUB, files_of)
check("finished: listed and latest", latest["tag_name"], "v1.21.0-25")

# A file re-uploaded after the sums (a re-run job): not until signed again.
late = add(release("v1.21.0-26", n=26, at="2026-11-27T06:20:00Z"))
check("a file newer than the sums", bool(uf.ready(late, *files_of(late)[::2], PUB)), True)
# Sums replaced after the signature (release_sums_add.sh mid-run): stale signature.
stale = add(release("v1.21.0-26", n=126, sig_at="2026-11-27T06:09:00Z"))
check("a signature older than the sums", bool(uf.ready(stale, *files_of(stale)[::2], PUB)), True)
# Signed by another key, or naming another release, or listing a missing file.
other_key = uf.ready(done, files_of(done)[0], files_of(done)[2], "11" * 32)
check("another key", bool(other_key), True)
wrong_v = add(release("v1.21.0-27", n=27, version="1.21.0-26"))
check("sums of another release", bool(uf.ready(wrong_v, *files_of(wrong_v)[::2], PUB)), True)
missing = add(release("v1.21.0-28", n=28, extra=("aqua-server-linux-x64",)))
check("a listed file not attached", bool(uf.ready(missing, *files_of(missing)[::2], PUB)), True)
# Tampered sums with the original signature.
s, _, g = files_of(done)
check("sums changed after signing", bool(uf.ready(done, s.replace("a" * 64, "c" * 64), g, PUB)), True)

# GitHub's "latest" stable not ready: the highest ready stable, or none.
unsigned_stable = add(release("v1.21.1", n=211, pre=False, sig=False))
_, stable, _ = uf.choose([unsigned_stable, stable_old, old], unsigned_stable, PUB, files_of)
check("stable not ready: the previous ready stable", stable and stable["tag_name"], "v1.21.0")

print("updates feed: all good" if not fails else f"updates feed: {fails} failed")
sys.exit(1 if fails else 0)
