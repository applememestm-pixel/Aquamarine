# Aquamarine для Android — проект для Android Studio

Сам Gradle-проект лежит в `crates/aqua-client-ui/gen/android` (это место
Tauri выбирает сам, переносить нельзя). Эта папка — пульт управления им.

## Один раз установить
1. **Android Studio** (https://developer.android.com/studio), пройти первый запуск.
2. В Android Studio: *Settings → Languages & Frameworks → Android SDK → SDK Tools* →
   отметить **NDK (Side by side)** и **Android SDK Command-line Tools** → *Apply*.
3. **Rust** (https://rustup.rs) и **Node.js 22+** (https://nodejs.org) — для ПК-сборки они у тебя уже есть.

## Скрипты
| Файл | Что делает |
|---|---|
| `1-setup.bat` | Ставит Rust-цели под Android, Tauri CLI, зависимости UI, создаёт и настраивает проект (разрешения на микрофон/камеру, иконка). Запускать один раз. |
| `2-android-studio.bat` | Запускает dev-сессию и **открывает проект в Android Studio**. Там выбираешь телефон/эмулятор сверху и жмёшь ▶ Run. Окно скрипта не закрывать — через него Studio собирает Rust-часть. Правки в UI видны сразу (hot reload). |
| `3-build-apk.bat` | Собирает готовый `out\Aquamarine-debug.apk` — его можно просто скинуть на телефон и установить. |

> Открывать `gen/android` в Android Studio напрямую (File → Open) не стоит:
> Gradle ждёт файлы, которые Tauri CLI пишет при запуске. Всегда через `2-android-studio.bat`.

## Телефон по USB
*Настройки → О телефоне* → 7 раз нажать «Номер сборки» → *Для разработчиков* → **Отладка по USB**.
Подключить кабелем, разрешить отладку на телефоне.

## Релизная версия
Подписанный `Aquamarine.apk` собирает GitHub при каждом теге `v*`
(см. `.github/workflows/release.yml`) и кладёт в Releases.
