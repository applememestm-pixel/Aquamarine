# Отчёты об ошибках → твой Telegram

Мини-сервис на Cloudflare Workers: бесплатно, работает всегда и не зависит от твоего сервера. Приложения (мессенджер, сервер, Aquamarine Server) отправляют сюда сбои, а он сразу пересылает их тебе в Telegram. Текста переписки в отчётах нет — только ошибка, версия и устройство.

## Один раз

1. Аккаунт на https://dash.cloudflare.com (бесплатный).
2. Бот для отчётов: @BotFather → `/newbot`. Лучше отдельный, не тот, что качает стикеры.
3. Напиши боту `/start`, открой `https://api.telegram.org/bot<ТОКЕН>/getUpdates` и найди `"chat":{"id":123456789` — это твой chat id.
4. В этой папке:
   ```
   npx wrangler login
   npx wrangler deploy
   npx wrangler secret put TG_TOKEN   # токен бота
   npx wrangler secret put TG_CHAT    # chat id
   ```
   `deploy` напишет адрес вида `https://aquamarine-reports.<имя>.workers.dev`.
5. GitHub → репозиторий → Settings → Secrets and variables → Actions → **Variables** → New: `AQUA_REPORT_URL` = `https://aquamarine-reports.<имя>.workers.dev/report`.

Следующие сборки зашьют этот адрес в приложения. Проверка: `curl -X POST <адрес> -d '{"app":"test","msg":"hello"}'` — в Telegram придёт сообщение.

Одинаковая ошибка присылается не чаще раза в 10 минут, с одного адреса — не больше 30 отчётов в минуту.

## Telegram для серверов, где он заблокирован (по желанию)

Тот же Worker умеет пересылать запросы к Telegram Bot API — для стикеров из Telegram и входа через Telegram на сервере в стране, где Telegram закрыт.

1. Придумайте длинный случайный ключ и задайте его: `npx wrangler secret put TG_RELAY_KEY`, затем `npx wrangler deploy`.
2. Aquamarine Server → Стикеры → **Сеть Telegram** → «Адрес Bot API»: `https://aquamarine-reports.<имя>.workers.dev/tg/<ключ>` → «Сохранить».

Без `TG_RELAY_KEY` пересылка выключена: чужие боты через ваш Worker не пройдут. Токен бота проходит через Worker (он ваш), но нигде не сохраняется.
