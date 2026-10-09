# Building the Windows .exe binaries

Cross-compiling from Linux is not possible in this project's current
build environment — the toolchain host blocks the Rust standard
library download for the `x86_64-pc-windows-*` target. There are two
reliable ways to produce the binaries:

## Option A — build locally on Windows (fastest)

Prerequisites (one-time):

* Rust stable: <https://rustup.rs>
* Node.js 22+: <https://nodejs.org>
* MSVC build tools: <https://visualstudio.microsoft.com/visual-cpp-build-tools/>
  (select "Desktop development with C++")
* WebView2 (pre-installed on Windows 10/11)
* `cargo install tauri-cli --version '^2' --locked`

Then from the repo root:

```powershell
# Server — a lightweight .exe suitable for both hosts.
cargo build --release -p aqua-server
# → target\release\aqua-server.exe   (~ 8–12 MB)

# Desktop app — Tauri 2 + Svelte frontend.
cd crates\aqua-client-ui\ui
npm ci
npm run build
cd ..
cargo tauri build --no-bundle
# → target\release\aqua-client-ui.exe  (~ 10–15 MB)
```

First build takes 5–8 minutes. Subsequent incremental rebuilds are
seconds.

If you also want an MSI installer, drop `--no-bundle` from the last
step; Tauri will produce `target\release\bundle\msi\aquamarine_x.y.z_x64_en-US.msi`
(needs WiX in your PATH — `winget install WiXToolset.WiXCLI`).

## Option B — build in CI (hands-off, reproducible)

A ready-to-go workflow is shipped as `ci-windows-build.yml.txt` at the
repo root. GitHub's own policy blocks writing to `.github/workflows/`
through remote tools, so once you add the git remote, move it into
place yourself:

```powershell
mkdir .github\workflows
Move-Item ci-windows-build.yml.txt .github\workflows\windows-build.yml
git add .github\workflows\windows-build.yml
git commit -m "ci: build Windows binaries on push"
git push
```

Pushing to `main` runs both builds on a GitHub-hosted `windows-latest`
runner and uploads two artifacts you can download from the workflow
run:

* `aqua-server-windows-x64` → `aqua-server.exe`
* `aquamarine-windows-x64`  → `aquamarine.exe`

Tagging the release (`git tag v0.1.0 && git push --tags`) additionally
attaches both binaries to a GitHub Release.

## Running the pair end to end

On the host that will run the server:

```powershell
aqua-server.exe
# listens on 127.0.0.1:7777 by default
# set AQUA_LISTEN=0.0.0.0:7777 to accept connections from other machines
```

On each client machine, double-click `aquamarine.exe`. Then in each
app:

1. Click the plug icon at the bottom-left → enter the server URL
   (`http://<server-host>:7777`) and pick a short username.
2. One client clicks "+" in the chat list → types the other's
   username and a first message.
3. The other client sees the chat appear and the message decrypted.

Vault + master key are stored under
`%APPDATA%\app.aquamarine.desktop\` and persist across restarts.
