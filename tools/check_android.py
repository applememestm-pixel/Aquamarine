#!/usr/bin/env python3
"""Android pieces that only break in the release APK or on a phone.

- A Tauri plugin class without @TauriPlugin is thrown out of the shrunk
  release APK (1.21.0-23: background messages and the Keystore stopped).
- Arguments read with parseArgs need @InvokeArg for the same reason.
- Every service / receiver in the Kotlin sources must be declared both by the
  release workflow and by android/patch-project.sh / .ps1 (local and CI builds).
- Every build excludes the whole app from Android's backup (android/backup/).
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sources = sorted((ROOT / "android").glob("*.kt")) + sorted((ROOT / "android" / "widget").glob("*.kt"))
manifests = [
    ROOT / ".github" / "workflows" / "release.yml",
    ROOT / "android" / "patch-project.sh",
    ROOT / "android" / "patch-project.ps1",
]
errors = []


def annotations(lines, i):
    """Annotations directly above line i (comments and blank lines allowed)."""
    found = []
    i -= 1
    while i >= 0:
        line = lines[i].strip()
        if line.startswith("@"):
            found.append(line.split("(")[0])
        elif line and not line.startswith(("//", "/*", "*")):
            break
        i -= 1
    return found


components = []
for path in sources:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    parsed = set(re.findall(r"parseArgs\((\w+)::class\.java\)", text))
    for i, line in enumerate(lines):
        m = re.match(r"\s*(?:public )?class (\w+)", line)
        if not m:
            continue
        name = m.group(1)
        if re.search(r":\s*Plugin\(", line) and "@TauriPlugin" not in annotations(lines, i):
            errors.append(f"{path.name}:{i + 1}: {name} needs @TauriPlugin directly above it")
        if name in parsed and "@InvokeArg" not in annotations(lines, i):
            errors.append(f"{path.name}:{i + 1}: {name} needs @InvokeArg directly above it")
        if "@TauriPlugin" in annotations(lines, i) and not re.search(r":\s*Plugin\(", line):
            errors.append(f"{path.name}:{i + 1}: @TauriPlugin on {name}, which is not a plugin")
        if re.search(r":\s*(Service|BroadcastReceiver|AppWidgetProvider)\(\)", line):
            components.append(name)

for manifest in manifests:
    text = manifest.read_text(encoding="utf-8")
    for name in components:
        if f'android:name=".{name}"' not in text:
            errors.append(f"{manifest.relative_to(ROOT)}: {name} is not declared in the Android manifest")

# Android's backup would carry keys and the vault off the phone: every build
# turns it off and ships the rules that exclude everything.
DOMAINS = [
    "root", "file", "database", "sharedpref", "external",
    "device_root", "device_file", "device_database", "device_sharedpref",
]
BACKUP_ATTRS = [
    'android:allowBackup="false"',
    'android:fullBackupContent="@xml/backup_rules"',
    'android:dataExtractionRules="@xml/data_extraction_rules"',
]
# (file, how many manifests it patches)
patchers = [(m, 2 if m.name == "release.yml" else 1) for m in manifests]
patchers.append((ROOT / ".github" / "workflows" / "server-app-android.yml", 1))
for path, count in patchers:
    # (the shell patcher writes "\@" for Perl)
    text = path.read_text(encoding="utf-8").replace("\\@", "@")
    for attr in BACKUP_ATTRS:
        if text.count(f"<application {BACKUP_ATTRS[0]}") < count or attr not in text:
            errors.append(f"{path.relative_to(ROOT)}: the manifest isn't given {attr}")
            break
for name, sections in (("backup_rules.xml", 1), ("data_extraction_rules.xml", 2)):
    text = (ROOT / "android" / "backup" / name).read_text(encoding="utf-8")
    if "<include" in text:
        errors.append(f"android/backup/{name}: nothing may be included in a backup")
    for domain in DOMAINS:
        if text.count(f'<exclude domain="{domain}" path="." />') != sections:
            errors.append(f"android/backup/{name}: domain {domain} must be excluded in every section")

if errors:
    print("\n".join(errors))
    sys.exit(1)
print(f"android: {len(sources)} files, {len(components)} services and receivers declared")
