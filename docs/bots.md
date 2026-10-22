# Боты и мини-приложения

> Часть [технической документации](README.md). Полный перечень HTTP-методов ретранслятора — в [api.md](api.md).

Документ описывает программный интерфейс ботов ретранслятора Aquamarine (`aqua-server`), порядок создания бота, модель безопасности и ограничения. Описание соответствует версии протокола `v1` и ретранслятору версии 0.1.1 и выше (кнопки и мини-приложения — с 0.2.0).

## 1. Определения

- **Бот** — учётная запись каталога с признаком `is_bot = 1`. Ею управляет внешняя программа через HTTP, а не клиент Aquamarine.
- **Владелец** — идентичность (пара ключей X25519/Ed25519), которая создала бота. Управлять ботом (сбрасывать токен, удалять) может только владелец.
- **Токен** — строка вида `aqbot_` + 48 шестнадцатеричных символов (192 бита энтропии из `OsRng`). Предъявление токена даёт полномочия бота.
- **Обновление** (`update`) — входящее сообщение боту. Имеет порядковый номер `id`, отправителя `from`, текст `text` и время `ts` (секунды Unix).
- **Контакт бота** — пользователь, который писал боту и после этого не отправил `/stop`.

## 2. Отличия от обычных чатов

Чаты с ботами **не защищены сквозным шифрованием**. Бот — HTTP-программа без хранилища ключей, поэтому установить сеанс X3DH и Double Ratchet она не может. Текст передаётся ретранслятору открытым (`PayloadKind::Plaintext`) в пределах TLS-соединения. Его могут прочитать:

- оператор ретранслятора;
- владелец бота;
- при федерации — ретранслятор, через который проходит доставка.

Клиент показывает это предупреждение под полем ввода каждого чата с ботом. Передавать ботам пароли, ключи восстановления и иные секреты не следует.

## 3. Создание бота

### 3.1. Через клиент

1. Подключитесь к ретранслятору под своей учётной записью.
2. Откройте «Настройки → Боты».
3. Укажите имя пользователя бота (3–32 символа: `a–z`, `0–9`, `_`, `.`) и отображаемое имя (до 64 символов). Нажмите «Создать».
4. Скопируйте токен.

Ретранслятор хранит только SHA-256 токена. Сам токен показывается один раз. Клиент сохраняет копию в зашифрованном хранилище устройства (запись `bot_token:<имя>`), и она входит в зашифрованную резервную копию. Если на устройстве копии нет, токен можно только сбросить: «Сбросить токен» выпускает новый, старый перестаёт действовать немедленно.

### 3.2. Через API (для владельца)

Запросы ниже авторизуются сеансовым токеном пользователя (`Authorization: Bearer <session>`), который клиент получает при входе по запросу-ответу Ed25519.

| Метод | Путь | Тело | Ответ |
|---|---|---|---|
| `POST` | `/v1/bot/register` | `{"username", "display_name"}` | `{"ok", "username", "token", "telegram_token"}` |
| `GET` | `/v1/bot/list` | — | `[{"username", "display_name", "token": "", "hint"}]` |
| `DELETE` | `/v1/bot/{username}` | — | `204` |

Повторный `register` с именем существующего бота того же владельца сбрасывает токен. Одна идентичность может владеть не более чем 20 ботами (`MAX_BOTS_PER_OWNER`). Поле `hint` содержит последние четыре символа действующего токена.

## 4. Программный интерфейс бота

Базовый адрес — адрес ретранслятора, например `https://relay.example`. Все ответы передаются в JSON (UTF-8).

### 4.1. Аутентификация

Токен передаётся в заголовке:

```
Authorization: Bot aqbot_0123…
```

Для совместимости с ранними сценариями ретранслятор принимает токен и в параметре `token` (строка запроса или тело JSON). Этот способ не рекомендуется: адреса с параметрами попадают в журналы прокси-серверов и веб-серверов. Ответ на неверный токен — `401` с задержкой 300 мс, которая замедляет перебор.

### 4.2. `GET /v1/bot/me`

Возвращает `{"username": "weatherbot", "display_name": "Weather", "id": 123456789}`; `id` — числовой идентификатор для слоя Telegram (п. 9.3). Используется для проверки токена при запуске.

### 4.3. `GET /v1/bot/updates`

Параметры:

| Параметр | Тип | По умолчанию | Назначение |
|---|---|---|---|
| `after` | целое ≥ 0 | 0 | Подтверждение: обновления с `id < after` удаляются из очереди |
| `timeout` | 0–30 | 0 | Длинный опрос: время ожидания (в секундах) первого обновления |

Ответ — массив обновлений в порядке возрастания `id`:

```json
[{"id": 41, "from": "alice", "text": "/start", "ts": 1792002000}]
```

Порядок работы: передавать `after = последний полученный id + 1` и `timeout = 25`. Ожидание реализовано через `tokio::sync::Notify`, поэтому ответ приходит сразу после поступления сообщения, без периодического опроса на стороне сервера.

Очередь каждого бота хранится в памяти ретранслятора и ограничена 1000 записями (при переполнении удаляются самые старые). После перезапуска ретранслятора неподтверждённые обновления теряются.

### 4.4. `POST /v1/bot/send`

Тело: `{"to": "alice", "text": "Привет"}`. Ответ при успехе — `204`.

| Код | Причина |
|---|---|
| `400` | Пустой текст |
| `401` | Неверный токен |
| `403` | Получатель не писал боту или отправил `/stop`; бот не может первым начать переписку |
| `413` | Текст длиннее 4096 символов |
| `429` | Превышен лимит частоты (п. 5) |

Получатель может находиться на другом ретрансляторе федерации: доставка выполняется тем же механизмом, что и для обычных сообщений ([federation.md](federation.md)).

### 4.5. Служебные команды пользователя

- `/start` — первое сообщение, которое клиент отправляет при открытии чата с ботом. Добавляет пользователя в контакты бота.
- `/stop` — удаляет пользователя из контактов бота. После этого `send` в его адрес возвращает `403`, пока пользователь снова не напишет боту.

Обе команды доставляются боту как обычные обновления.

### 4.6. Кнопки и форматирование в `send`

`POST /v1/bot/send` принимает, кроме `to` и `text`, необязательные поля. С любым из них ответ — `200 {"message_id"}` вместо `204`:

| Поле | Значение |
|---|---|
| `parse_mode` | `"Markdown"`, `"MarkdownV2"` или `"HTML"` — как в Telegram |
| `buttons` | Кнопки под сообщением: `["Да", "Нет"]` (один ряд) или ряды `[[{"text", "data"}], [{"text", "url"}], [{"text", "app"}]]`. Строка — кнопка, которая возвращает свой текст; `data` — данные нажатия; `url` открывает ссылку; `app` — мини-приложение |
| `keyboard` | Кнопки над полем ввода, отправляют свой текст: `[["1", "2"], ["3"]]` |
| `one_time` | Скрыть `keyboard` после нажатия |
| `remove_keyboard` | Убрать клавиатуру над полем ввода |
| `reply_markup` | Готовая разметка Telegram (если она уже есть) |

Нажатие кнопки с `data` приходит обновлением с полями `callback` (данные) и `callback_msg` (номер сообщения с кнопками).

### 4.7. Остальные методы

| Метод и путь | Тело / параметры | Ответ |
|---|---|---|
| `POST /v1/bot/edit` | `{"to", "text", "message_id"?, "buttons"?, "parse_mode"?}` | `{"message_id"}`; правка показывается новой версией сообщения |
| `POST /v1/bot/broadcast` | `{"text", "parse_mode"?, "buttons"?}` | `202 {"queued": N}`; рассылка всем контактам идёт в фоне в пределах лимитов (повтор при 429) |
| `GET /v1/bot/contacts` | — | `[{"username", "display_name", "id"}]` |
| `GET /v1/bot/user/{имя}` | — | `{"username", "display_name", "id", "is_bot", "is_contact"}`; `404`, если такого пользователя нет |
| `GET /v1/bot/commands` | — | `[{"command", "description"}]` |
| `POST /v1/bot/commands` | `[{"command", "description"}]` или `{"help": "что умею", …}` | `204`; до 100 команд, имя — `a–z`, `0–9`, `_`, до 32 символов |
| `DELETE /v1/bot/commands` | — | `204` |
| `GET /v1/bot/webhook` | — | как `getWebhookInfo` |
| `POST /v1/bot/webhook` | `{"url", "secret"?, "drop_pending"?}` | `204`; обновления отправляются на адрес (раздел 9.5) |
| `DELETE /v1/bot/webhook` | — | `204`; снова работает `updates` |
| `GET /v1/bot/profile` | — | `{"username", "display_name", "description", "short_description"}` |
| `POST /v1/bot/profile` | `{"description"?, "short_description"?}` | `204`; до 512 и 120 символов |
| `GET /v1/bot/stats` | — | `{"username", "contacts", "pending_updates", "webhook", "commands"}` |
| `POST /v1/bot/call/{метод}` | параметры метода Telegram (JSON) | ответ Telegram `{"ok", "result"}` — любой метод из раздела 9.4 с заголовком `Authorization: Bot` |

## 5. Ограничения

| Ограничение | Значение |
|---|---|
| Отправка, на бота | 30 сообщений/с (корзина маркеров ёмкостью 30) |
| Отправка, на пару бот → получатель | 1 сообщение/с, всплеск до 5 |
| Опрос `updates` | 5 запросов/с, всплеск до 20 |
| Длина текста | 4096 символов |
| Длинный опрос | не более 30 с |
| Очередь входящих | 1000 обновлений |
| Ботов на владельца | 20 |

При ответе `429` следует повторить запрос не ранее чем через 1 секунду.

## 6. Модель безопасности

1. **Хранение токенов.** Ретранслятор хранит `SHA-256(token)` (таблица `bot_keys`). Утечка базы данных не раскрывает действующие токены. Соль не нужна: токен — случайное 192-битное значение, а не пароль, поэтому словарный перебор неприменим. Токены из таблицы `bot_tokens` прежних версий хешируются при первом запуске, после чего таблица удаляется.
2. **Передача.** Токен передаётся только в заголовке `Authorization` по TLS. Примеры в `tools/` и `sdk/python/examples/` читают его из переменной окружения `AQUA_BOT_TOKEN` или из скрытого ввода и не принимают в аргументах командной строки, которые видны в списке процессов.
3. **Разрешённые получатели.** Бот пишет только своим контактам (п. 4.5) и владельцу. Это исключает рассылку незнакомым пользователям, даже если токен похищен.
4. **Ограничение частоты** — п. 5.
5. **Отзыв.** Сброс токена или удаление бота мгновенно лишают силы прежний токен. При удалении стираются и записи контактов бота.
6. **Изоляция.** Токен бота не даёт доступа к пользовательским эндпоинтам (`/v1/send`, `/v1/backup`, профиль): сеансовые токены пользователей и токены ботов проверяются раздельно.
7. **Федерация.** Учётные записи ботов не реплицируются на другие ретрансляторы (`fed_users_since` исключает `is_bot = 1`). Бот обслуживается только своим ретранслятором.

Рекомендации разработчику бота:

- хранить токен вне репозитория (переменная окружения, менеджер секретов);
- не записывать входящие тексты в журналы, если в этом нет необходимости;
- обрабатывать `401` как сигнал отзыва токена и завершать работу;
- выполнять HTTPS-запросы только к ретранслятору с действующим сертификатом.

## 7. Примеры

### 7.1. curl

```sh
export AQUA_BOT_TOKEN=aqbot_…
R=https://relay.example
curl -s -H "Authorization: Bot $AQUA_BOT_TOKEN" "$R/v1/bot/me"
curl -s -H "Authorization: Bot $AQUA_BOT_TOKEN" "$R/v1/bot/updates?after=0&timeout=25"
curl -s -H "Authorization: Bot $AQUA_BOT_TOKEN" -H 'Content-Type: application/json' \
     -d '{"to":"alice","text":"Привет"}' "$R/v1/bot/send"
```

### 7.2. Python (стандартная библиотека)

Полный пример — [`sdk/python/examples/echo_bot.py`](../sdk/python/examples/echo_bot.py). Основной цикл:

```python
after = 0
while True:
    for u in bot.updates(after):          # GET /v1/bot/updates?after=…&timeout=25
        after = u["id"] + 1
        bot.send(u["from"], u["text"])    # POST /v1/bot/send
```

### 7.2.1. Библиотека aquabot (Python)

[`sdk/python`](../sdk/python) — готовая библиотека без зависимостей. В ней есть:

- команды, обработчики и состояние на пользователя;
- повторы при 429 и 5xx, разбиение длинных ответов;
- подключение нейросетей: llama.cpp, Ollama, OpenAI-совместимые серверы, Anthropic;
- безопасный калькулятор и математика.

```sh
pip install "git+https://github.com/applememestm-pixel/Aquamarine#subdirectory=sdk/python"
```

```python
from aquabot import Bot, llm
bot = Bot()                                   # $AQUA_SERVER, $AQUA_BOT_TOKEN
bot.use_help().use_calc().use_llm(llm.LlamaCpp("http://127.0.0.1:8080"))
bot.run()
```

Готовый бот без кода — `aquabot run`; модель задаётся через `--llm ollama:llama3.1` или `$AQUA_LLM`. Подробнее — в [sdk/python/README.md](../sdk/python/README.md).

### 7.3. JavaScript (Node.js ≥ 18)

```js
const R = "https://relay.example", H = { Authorization: `Bot ${process.env.AQUA_BOT_TOKEN}` };
let after = 0;
for (;;) {
  const ups = await (await fetch(`${R}/v1/bot/updates?after=${after}&timeout=25`, { headers: H })).json();
  for (const u of ups) {
    after = u.id + 1;
    await fetch(`${R}/v1/bot/send`, { method: "POST", headers: { ...H, "Content-Type": "application/json" },
      body: JSON.stringify({ to: u.from, text: `Получено: ${u.text}` }) });
  }
}
```

### 7.4. Консольный бот

`tools/aqua-console.py` превращает бота в консольный чат: входящие сообщения выводятся в терминал, введённые строки отправляются собеседнику. Режим комнаты (`/room`) пересылает каждое сообщение всем присоединившимся. Сценарий можно сохранить из клиента: «Настройки → Боты → Сохранить aqua-console.py».

## 8. Проверка

Поведение, описанное в документе, проверяется интеграционным тестом `crates/aqua-server/tests/e2e.rs::bot_lifecycle_and_console_script`. Тест проверяет:

- создание бота и аутентификацию заголовком и параметром;
- сокрытие токена в списке;
- отказ `403` для незнакомого получателя, `413` для длинного текста и `429` при превышении частоты;
- длинный опрос с немедленным пробуждением;
- работу консольного сценария;
- отзыв токена при сбросе и права на удаление.

## 9. Совместимость с Telegram Bot API

Ретранслятор принимает запросы в формате Telegram Bot API по адресу `/bot<токен>/<метод>` (как `https://api.telegram.org/bot<токен>/<метод>`). Бот, написанный для Telegram, работает в Aquamarine без изменения кода: достаточно указать библиотеке адрес ретранслятора и токен бота Aquamarine.

### 9.1. Токен

Библиотеки Telegram проверяют вид токена `<число>:<секрет>`. Поэтому ретранслятор принимает токен Aquamarine в таком же виде: `<id бота>:aqbot_…`. Его выдают при создании бота (`telegram_token` в ответе `POST /v1/bot/register`) и кнопка «Токен Telegram» в настройках ботов. Числовая часть служит оформлением; проверяется только секрет.

### 9.2. Подключение библиотек

```python
# python-telegram-bot ≥ 20
app = Application.builder().token(TOKEN).base_url("http://relay:7777/bot").build()

# aiogram 3
from aiogram.client.session.aiohttp import AiohttpSession
from aiogram.client.telegram import TelegramAPIServer
bot = Bot(TOKEN, session=AiohttpSession(api=TelegramAPIServer.from_base("http://relay:7777")))

# pyTelegramBotAPI
telebot.apihelper.API_URL = "http://relay:7777/bot{0}/{1}"
```

```js
// telegraf
const bot = new Telegraf(TOKEN, { telegram: { apiRoot: "http://relay:7777" } });
// grammY
const bot = new Bot(TOKEN, { client: { apiRoot: "http://relay:7777" } });
```

```sh
curl "http://relay:7777/bot$TOKEN/getMe"
```

### 9.3. Соответствие понятий

| Telegram | Aquamarine |
|---|---|
| `chat.id`, `from.id` | Число, вычисляемое из `@имени` (первые 6 байт SHA-256); постоянно для имени. В `chat_id` можно передать и `"@имя"` |
| Тип чата | Только `private` |
| `update_id` | Номер в очереди бота + 1; `offset` подтверждает предыдущие обновления, как в Telegram |
| `message.entities` | `bot_command` для текста, начинающегося с `/` |
| `parse_mode` | HTML, MarkdownV2 и Markdown переводятся в разметку Aquamarine (`**жирный**`, `__курсив__`, `++подчёркнутый++`, `~~зачёркнутый~~`, `||скрытый||`, `` `код` ``, `[текст](ссылка)`) |
| Встроенная клавиатура | Приложение рисует настоящие кнопки под сообщением. Нажатие кнопки с `callback_data` даёт боту `callback_query` с `message.message_id` того сообщения. URL-кнопки открывают ссылку, кнопки `web_app` — мини-приложение. Старые версии приложения видят нумерованный список (1️⃣ 2️⃣ …) и могут ответить номером или текстом кнопки |
| Обычная клавиатура | Кнопки показываются над полем ввода (`one_time_keyboard`, `remove_keyboard`, `input_field_placeholder` учитываются); нажатие отправляет текст кнопки обычным сообщением, как в Telegram |
| Мини-приложения | Кнопки `web_app` (во встроенной и обычной клавиатуре) и кнопка меню (`setChatMenuButton` с `type: web_app`) открывают страницу так же, как Telegram Web: во фрейме, с параметрами запуска в `#hash`. Официальный `telegram-web-app.js` работает без изменений. `initData` подписывается ретранслятором ключом бота по алгоритму Telegram (HMAC-SHA256 с ключом `HMAC-SHA256("WebAppData", токен)`), поэтому обычная проверка подписи в коде бота проходит. Ключ ретранслятор узнаёт при первом обращении бота к API после запуска. `sendData` из мини-приложения, открытого кнопкой обычной клавиатуры, приходит боту как `message.web_app_data`; `answerWebAppQuery` отправляет пользователю текст результата. Поддерживаются MainButton, SecondaryButton, BackButton, SettingsButton, всплывающие окна, CloudStorage (хранится на устройстве), полноэкранный режим, тема приложения |
| `answerCallbackQuery` | Текст, если задан, приходит пользователю сообщением (`ℹ️ …`, при `show_alert` — `⚠️ …`) |
| `editMessageText` | Новая версия отправляется новым сообщением |
| Медиа (`sendPhoto`, `sendDocument`…) | Отправляются подпись и ссылка (если файл задан URL); загруженные файлы пока не пересылаются |

### 9.4. Поддерживаемые методы

| Группа | Методы |
|---|---|
| Получение обновлений | `getUpdates` (`offset`, `limit`, `timeout` до 50 с), `setWebhook` (`url`, `secret_token`, `drop_pending_updates`), `deleteWebhook`, `getWebhookInfo` |
| Отправка | `sendMessage`, `sendPhoto`, `sendDocument`, `sendVideo`, `sendAudio`, `sendVoice`, `sendAnimation`, `sendVideoNote`, `sendSticker`, `sendMediaGroup`, `sendLocation`, `sendVenue`, `sendContact`, `sendDice`, `sendPoll` |
| Кнопки | `answerCallbackQuery`, `editMessageText`, `editMessageCaption`, `editMessageReplyMarkup` |
| Мини-приложения | `setChatMenuButton`, `getChatMenuButton`, `answerWebAppQuery` |
| Сведения | `getMe`, `getChat`, `getChatMember`, `getChatMemberCount`, `getChatAdministrators`, `getUserProfilePhotos`, `getMyName`, `getMyDescription`, `getMyShortDescription`, `getMyDefaultAdministratorRights` |
| Настройка бота | `setMyCommands`, `getMyCommands`, `deleteMyCommands`, `setMyDescription`, `setMyShortDescription` |
| Принимаются без действия | `sendChatAction`, `deleteMessage(s)`, `setMyName`, `setMyDefaultAdministratorRights`, `setMessageReaction`, `pinChatMessage`, `unpinChatMessage`, `unpinAllChatMessages`, `leaveChat`, `close`, `logOut` |
| Не поддерживаются | `forwardMessage`, `copyMessage`, `getFile` (ответ 400); остальные методы — 404 `Not Found`, как у Telegram для неизвестного метода |

Ошибки возвращаются в формате Telegram: `{"ok": false, "error_code": …, "description": …}`; при превышении частоты — 429 с `parameters.retry_after`.

### 9.5. Вебхуки

После `setWebhook` ретранслятор отправляет каждое обновление отдельным запросом `POST` на указанный адрес. Если задан `secret_token`, он передаётся в заголовке `X-Telegram-Bot-Api-Secret-Token`. Обновление удаляется из очереди после ответа 2xx; при ошибке доставка повторяется с паузой от 1 до 60 с, а `getWebhookInfo` показывает последнюю ошибку. Пока вебхук задан, `getUpdates` отвечает 409, как в Telegram. Вебхуки сохраняются в `aqua-tg-webhooks.json` и возобновляются после перезапуска.

Принимаются адреса `http://` и `https://`. Запросы к служебному адресу облачных метаданных (`169.254.x.x`) отклоняются.

### 9.6. Ограничения

Действуют ограничения раздела 5: бот пишет только тем, кто написал ему (иначе 403 `Forbidden: bot can't initiate conversation with a user`), частота отправки — 30 сообщений/с на бота и 1 сообщение/с на чат. Группы, каналы, инлайн-режим и платежи не поддерживаются. Переписка с ботом не защищена сквозным шифрованием.

Проверка: интеграционный тест `crates/aqua-server/tests/e2e.rs::telegram_bot_api_compat` (вход, длинный опрос, команда, клавиатура → `callback_query`, `answerCallbackQuery`, вебхук с секретом, 409 для `getUpdates`).

## 10. Справочник всех функций

Сводка всего, что умеет бот: родной API ретранслятора, слой Telegram и библиотека `aquabot`. Авторизация родного API — заголовок `Authorization: Bot aqbot_…`; слоя Telegram — токен в пути `/bot<id>:aqbot_…/<метод>`.

### 10.1. Родной API

| Метод | Путь | Параметры | Ответ | Ошибки | Пример |
|---|---|---|---|---|---|
| `GET` | `/v1/bot/me` | — | `{"username", "display_name", "id"}` | 401 | проверка токена при запуске |
| `GET` | `/v1/bot/updates` | `after`, `timeout` (0–30) | `[{"id", "from", "text", "ts", "callback"?, "callback_msg"?}]` | 401, 409 при вебхуке, 429 | длинный опрос: `after=последний id + 1`, `timeout=25` |
| `POST` | `/v1/bot/send` | `to`, `text`, `parse_mode`?, `buttons`?, `keyboard`?, `one_time`?, `remove_keyboard`?, `reply_markup`? | `204` или `{"message_id"}` | 400, 403, 413, 429 | `{"to":"alice","text":"Выбери","buttons":["Да","Нет"]}` |
| `POST` | `/v1/bot/edit` | `to`, `text`, `message_id`?, `parse_mode`?, `buttons`? | `{"message_id"}` | 400, 403, 429 | обновить карточку после нажатия |
| `POST` | `/v1/bot/broadcast` | `text`, `parse_mode`?, `buttons`? | `202 {"queued"}` | 400, 413 | новости всем подписчикам |
| `GET` | `/v1/bot/contacts` | — | `[{"username", "display_name", "id"}]` | 401 | кому можно написать |
| `GET` | `/v1/bot/user/{имя}` | — | `{"username", "display_name", "id", "is_bot", "is_contact"}` | 404 | проверить собеседника |
| `GET` | `/v1/bot/commands` | — | `[{"command", "description"}]` | 401 | — |
| `POST` | `/v1/bot/commands` | список или объект `{"команда": "описание"}` | `204` | 400 | меню «/» в приложении |
| `DELETE` | `/v1/bot/commands` | — | `204` | 401 | — |
| `GET` | `/v1/bot/webhook` | — | `{"url", "pending_update_count", "last_error_*"?}` | 401 | — |
| `POST` | `/v1/bot/webhook` | `url`, `secret`?, `drop_pending`? | `204` | 400 | бот за HTTPS без опроса |
| `DELETE` | `/v1/bot/webhook` | — | `204` | 401 | вернуться к опросу |
| `GET` | `/v1/bot/profile` | — | `{"username", "display_name", "description", "short_description"}` | 401 | — |
| `POST` | `/v1/bot/profile` | `description`?, `short_description`? | `204` | 401 | — |
| `GET` | `/v1/bot/stats` | — | `{"username", "contacts", "pending_updates", "webhook", "commands"}` | 401 | мониторинг |
| `POST` | `/v1/bot/call/{метод}` | параметры метода Telegram | `{"ok", "result"}` | как у Telegram | `call/sendPoll` с `{"chat_id":"@alice","question":"?","options":["a","b"]}` |
| `POST` | `/v1/bot/register` | `username`, `display_name` (сеанс владельца) | `{"ok", "username", "token", "telegram_token"}` | 400, 409 | создать бота или сбросить токен |
| `GET` | `/v1/bot/list` | — (сеанс владельца) | `[{"username", "display_name", "hint"}]` | 401 | — |
| `DELETE` | `/v1/bot/{имя}` | — (сеанс владельца) | `204` | 403, 404 | удалить бота |

### 10.2. Слой Telegram Bot API

| Метод | Важные параметры | Что делает в Aquamarine |
|---|---|---|
| `getMe` | — | Карточка бота; `can_join_groups` и прочие возможности — `false` |
| `getUpdates` | `offset`, `limit` (1–100), `timeout` (до 50) | Длинный опрос; 409, пока задан вебхук |
| `setWebhook` | `url`, `secret_token`, `drop_pending_updates` | Обновления отправляются `POST` на адрес, повтор с паузой 1–60 с |
| `deleteWebhook` | `drop_pending_updates` | Снимает вебхук |
| `getWebhookInfo` | — | Адрес, очередь, последняя ошибка |
| `sendMessage` | `chat_id`, `text`, `parse_mode`, `reply_markup` | Сообщение с разметкой и кнопками |
| `editMessageText` | `chat_id`, `message_id`, `text`, `reply_markup` | Новая версия отправляется новым сообщением |
| `editMessageCaption` | `caption` | То же для подписи |
| `editMessageReplyMarkup` | `reply_markup` | Обновляет кнопки |
| `sendPhoto`, `sendDocument`, `sendVideo`, `sendAudio`, `sendVoice`, `sendAnimation`, `sendVideoNote`, `sendSticker`, `sendMediaGroup` | файл ссылкой, `caption` | Значок, подпись и ссылка; загруженные файлы пока не пересылаются |
| `sendLocation`, `sendVenue` | `latitude`, `longitude`, `title` | Ссылка на карту OpenStreetMap |
| `sendContact` | `first_name`, `phone_number` | Карточка контакта текстом |
| `sendDice` | `emoji` | Бросок 1–6 |
| `sendPoll` | `question`, `options` | Вопрос с пронумерованными вариантами |
| `answerCallbackQuery` | `callback_query_id`, `text`, `show_alert` | Текст приходит пользователю (`ℹ️`/`⚠️`) |
| `setChatMenuButton`, `getChatMenuButton` | `menu_button` (`web_app`) | Кнопка мини-приложения в чате |
| `answerWebAppQuery` | `web_app_query_id`, `result` | Результат мини-приложения сообщением |
| `getChat`, `getChatMember` | `chat_id` | Личный чат; участник — `member` |
| `getChatMemberCount` | `chat_id` | 2 (личный чат) |
| `getChatAdministrators` | `chat_id` | Пустой список |
| `getUserProfilePhotos` | `user_id` | Пустой список |
| `setMyCommands`, `getMyCommands`, `deleteMyCommands` | `commands` | Меню команд |
| `setMyDescription`, `getMyDescription` | `description` | Описание бота (до 512 символов) |
| `setMyShortDescription`, `getMyShortDescription` | `short_description` | Короткое описание (до 120 символов) |
| `getMyName` | — | Отображаемое имя |
| `getMyDefaultAdministratorRights` | — | Все права `false` |
| `sendChatAction`, `deleteMessage(s)`, `setMyName`, `setMyDefaultAdministratorRights`, `setMessageReaction`, `pinChatMessage`, `unpinChatMessage`, `unpinAllChatMessages`, `leaveChat`, `close`, `logOut` | — | Принимаются, ответ `true` |
| `forwardMessage(s)`, `copyMessage(s)`, `getFile` | — | 400: пока не поддерживаются |
| остальные | — | 404 `Not Found`, как у Telegram |

### 10.3. Библиотека `aquabot` (Python)

| Вызов | Что делает |
|---|---|
| `Bot(server?, token?)` | Бот; адрес и токен по умолчанию из `$AQUA_SERVER`, `$AQUA_BOT_TOKEN` |
| `@bot.command("start", help="…")` | Обработчик команды; строки `help` становятся меню «/» в приложении |
| `@bot.message(r"regex", when=…)` | Обработчик обычных сообщений (`ctx.match`) |
| `@bot.callback("data")` / `@bot.callback(r"^c:")` / `@bot.callback()` | Нажатие кнопки: точные данные, шаблон или любое |
| `@bot.default` | Всё, что не обработано |
| `@bot.on_error` | Ошибки обработчиков |
| `bot.use_help()`, `bot.use_calc()`, `bot.use_llm(llm)` | Готовые `/help`, калькулятор, чат с нейросетью |
| `bot.run()` / `bot.stop()` | Запуск длинного опроса / остановка |
| `ctx.reply(text, buttons=…, keyboard=…, parse_mode=…)` | Ответ отправителю |
| `ctx.sender`, `ctx.text`, `ctx.command`, `ctx.args`, `ctx.callback`, `ctx.state` | Отправитель, текст, команда, аргументы, данные кнопки, состояние пользователя |
| `bot.send(to, text, **opts)` | Сообщение пользователю; `False`, если он не писал боту или отправил `/stop` |
| `bot.api.send(...)`, `edit(...)`, `broadcast(...)` | Отправка, правка, рассылка |
| `bot.api.contacts()`, `user(name)`, `stats()` | Контакты, карточка пользователя, статистика |
| `bot.api.set_commands({...})`, `commands()`, `delete_commands()` | Меню команд |
| `bot.api.set_profile(description=…)`, `profile()` | Описание бота |
| `bot.api.set_webhook(url, secret=…)`, `webhook()`, `delete_webhook()` | Вебхук |
| `bot.api.call("sendPoll", chat_id="@alice", …)` | Любой метод Telegram |

```python
from aquabot import Bot
bot = Bot()

@bot.command("pick", help="выбрать цвет")
def pick(ctx):
    ctx.reply("**Какой цвет?**", parse_mode="Markdown",
              buttons=[[{"text": "Красный", "data": "c:red"}, {"text": "Синий", "data": "c:blue"}],
                       [{"text": "Сайт", "url": "https://example.org"}]])

@bot.callback(r"^c:")
def chosen(ctx):
    ctx.reply(f"Вы выбрали {ctx.callback[2:]}")

bot.run()
```

Проверка: `crates/aqua-server/tests/e2e.rs::native_bot_api_extras` и `sdk/python/tests/test_bot.py`.
