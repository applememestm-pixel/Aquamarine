#!/usr/bin/env python3
"""Android versionCode for a version, in the same order as the in-app
updater's `newer()` (ui/src/lib/updates.ts, aqua-server-app/src/updates.rs).

Tauri's own rule — major·1 000 000 + minor·1 000 + patch — doesn't follow our
numbering, where the third number reads like a decimal fraction (1.2.2 comes
after 1.2.11 and 1.2.12-2): it gave 1.2.2 a lower code (1002002) than 1.2.11
(1002011), and Android refuses to install a lower code over a higher one
("App not installed"), so in-app updates from 1.2.11 / 1.2.12-x failed.

Here: major·10^8 + minor·10^6 + patch-as-fraction (3 digits: "2"→200,
"11"→110, "12"→120, "21"→210)·10^3 + pre-release number (-N). Every code is
above all of Tauri's old ones (< 10^8 for 1.x), so it installs over them.

Usage: android_version_code.py 1.2.21-1      → 102210001
       android_version_code.py --check       → self-test
"""

import re
import sys


def code(version: str) -> int:
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:[-+](\d+))?", version.strip().lstrip("v"))
    if not m:
        raise SystemExit(f"can't read the version {version!r}")
    major, minor, patch, pre = int(m[1]), int(m[2]), m[3], int(m[4] or 0)
    if minor > 99 or len(patch) > 3 or pre > 999 or major > 20:
        raise SystemExit(f"{version}: out of the versionCode range")
    frac = int(patch.ljust(3, "0"))
    return major * 100_000_000 + minor * 1_000_000 + frac * 1_000 + pre


def check() -> None:
    # The order newer() gives (a "-N" counts as one more number: 1.2.21-1 is
    # after 1.2.21 — our releases never reuse a pre-release's base anyway).
    order = ["1.2.0-5", "1.2.1", "1.2.11", "1.2.12-1", "1.2.12-2", "1.2.2", "1.2.21", "1.2.21-1", "1.2.22-1", "1.2.3", "1.3.0-1", "2.0.0"]
    codes = [code(v) for v in order]
    assert codes == sorted(codes) and len(set(codes)) == len(codes), list(zip(order, codes))
    assert code("1.2.21-1") > 1002012, "must install over Tauri's old codes"
    print("ok", list(zip(order, codes)))


if __name__ == "__main__":
    if sys.argv[1:] == ["--check"]:
        check()
    else:
        print(code(sys.argv[1]))
