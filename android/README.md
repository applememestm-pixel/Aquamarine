# Aquamarine для Android — проект для Android Studio

Сам Gradle-проект лежит в `crates/aqua-client-ui/gen/android` (это место
Tauri выбирает сам, переносить нельзя). Эта папка — пульт управления им.

## Один раз установить
1. **Android Studio** (https://developer.android.com/studio), пройти первый запуск.
2. Больше ничего ставить руками не нужно: скрипты сами находят SDK, Java (JBR из Android Studio)
   и Studio на любом диске (у тебя `D:\Android\Sdk`, `D:\AndroidStudio`), а недостающий **NDK**
   `1-setup.bat` ставит сам — в отдельном окне, где нужно один раз нажать `y` (лицензия Google).
3. **Rust** (https://rustup.rs) и **Node.js 22+** (https://nodejs.org) — для ПК-сборки они у тебя уже есть.

## Linux (Arch)
Те же три шага — `.sh`-версии (запускать из терминала):

```bash
sudo pacman -S --needed rustup nodejs npm jdk21-openjdk unzip && rustup default stable
android/1-setup.sh          # SDK + NDK ставит сам в ~/Android/Sdk, если их нет
android/2-android-studio.sh # нужен Android Studio: yay -S android-studio
android/3-build-apk.sh      # → android/out/Aquamarine-debug.apk
```

`env.sh` ищет SDK в `$ANDROID_HOME`, `~/Android/Sdk`, `/opt/android-sdk` и в настройках
Android Studio; Java — JBR из Studio или JDK 17–21 из `/usr/lib/jvm`. SDK/NDK, скопированные
с Windows (`adb.exe`, NDK `windows-x86_64`), на Linux не работают — нужен Linux-SDK.
Сгенерированный на Windows `gen/android` пригоден: `patch-project.sh` перепишет
`local.properties` и `.cargo/config.toml` под Linux.

## Скрипты
| Файл | Что делает |
|---|---|
| `1-setup.bat` / `.sh` | Ставит Rust-цели под Android, Tauri CLI, зависимости UI, создаёт и настраивает проект (разрешения на микрофон/камеру, иконка). Запускать один раз. |
| `2-android-studio.bat` / `.sh` | Запускает dev-сессию и **открывает проект в Android Studio**. Там выбираешь телефон/эмулятор сверху и жмёшь ▶ Run. Окно скрипта не закрывать — через него Studio собирает Rust-часть. Правки в UI видны сразу (hot reload). |
| `3-build-apk.bat` / `.sh` | Собирает готовый `out\Aquamarine-debug.apk` — его можно просто скинуть на телефон и установить. |

> Саму папку `android\` в Android Studio открывать не нужно — это только скрипты.
> Открывать `gen/android` в Android Studio напрямую (File → Open) не стоит:
> Gradle ждёт файлы, которые Tauri CLI пишет при запуске. Всегда через `2-android-studio.bat`.

## Телефон по USB
*Настройки → О телефоне* → 7 раз нажать «Номер сборки» → *Для разработчиков* → **Отладка по USB**.
Подключить кабелем, разрешить отладку на телефоне.

## Релизная версия
Подписанный `Aquamarine.apk` собирает GitHub при каждом теге `v*`
(см. `.github/workflows/release.yml`) и кладёт в Releases.
