# PERFORMANCE, DATABASES & MEDIA PIPELINE

## 1. Цель

Провести измеряемую оптимизацию CeZeOs и Aquamarine.

Главный принцип: сначала измерить, затем найти причину, затем исправить и повторить измерение.

Не использовать субъективные утверждения об ускорении.

## 2. CeZeOs — база данных и запросы

Проверить:

* server.mjs.
* SQLite.
* SQL-запросы.
* Индексы.
* Транзакции.
* Блокировки.
* Пагинацию.
* Ленту.
* Комментарии.
* Реакции.
* Поиск.
* Сообщества.

Особенно исследовать N+1.

Оптимизировать формирование ленты:

* SQL-фильтрация.
* JOIN.
* Агрегации.
* Индексы.
* Keyset Pagination.

Не загружать целые таблицы ради последующей фильтрации в JavaScript.

## 3. Aquamarine — производительность

Проверить:

* Rust.
* SQLite.
* Mutex.
* DB Worker.
* WebSocket.
* VecDeque.
* UnboundedSender.
* UserState.
* BotLimits.
* BlobStore.
* Ratchet.
* Federation.
* History Sync.

Рассмотреть:

* Ограниченные очереди.
* Backpressure.
* TTL.
* Потоковую передачу.
* Батчевые операции.
* Разделение чтения и записи.
* Zeroization временных ключей.
* Оптимизацию rollback.

Не заменять существующую архитектуру без измерений.

## 4. Медиа CeZeOs

Целевая модель:

**Database:**

* Метаданные.
* Связи.
* Права.
* Пользователи.
* Публикации.

**Object Storage:**

* Оригиналы.
* Видео.
* Изображения.
* Аудио.
* Документы.

**Media Pipeline:**

* Thumbnail.
* Transcoding.
* Metadata extraction.
* Compression.
* Validation.
* Delivery.

Проверить:

* Base64.
* BLOB.
* Пиковую память.
* Параллельные загрузки.
* Ограничения размера.
* MIME.
* Очистку.
* Резервирование.

## 5. Обработка кадров

Обязательное исследование видеопайплайна:

Source → Decoder → Frame Queue → Processing → Encoder → Storage → Client Decoder → Renderer.

Проверить:

* Frame timestamps.
* Frame ordering.
* FPS.
* Variable FPS.
* Frame duration.
* Dropped frames.
* Duplicate frames.
* Color space.
* Orientation.
* Resolution.
* Audio/video synchronization.
* Corrupted frames.

Кадры должны обрабатываться в корректной последовательности с сохранением временных меток.

Проверить длинные видео, высокие разрешения и повреждённые источники.

## 6. Память и очереди кадров

Измерять:

* RAM.
* GPU memory.
* Buffer size.
* Queue depth.
* Decode time.
* Encode time.
* Frame processing time.
* Peak memory.
* Resource cleanup.

Проверить:

* Отсутствие утечек.
* Освобождение кадровых буферов.
* Ограничение параллельности.
* Backpressure.
* Отмену обработки.
* Закрытие ресурсов при смене экрана.

Не допускать неограниченного накопления декодированных кадров.

## 7. Изображения

Проверить:

* JPEG.
* PNG.
* WebP.
* GIF.
* Анимированные изображения.
* EXIF.
* Orientation.
* Alpha.
* Color profiles.
* Thumbnails.
* Resizing.
* Compression.
* Original preservation.

Не искажать исходные материалы при создании предпросмотра.

## 8. E2EE и мультимедиа

Разделять:

* Публичную обработку CeZeOs.
* Приватную обработку Aquamarine.

Нельзя передавать приватные незашифрованные вложения сторонним обработчикам без специально спроектированной модели доверия.

Проверить:

* Шифрование вложений.
* Хранение ключей.
* Кеш.
* Временные файлы.
* Метаданные.
* Очистку.
* Предпросмотр.

## 9. Производительность интерфейса

Измерить:

* FPS.
* Frame time.
* Long tasks.
* Layout shifts.
* DOM size.
* Component rerenders.
* WebView memory.
* Scroll performance.

Особое внимание:

* Длинным чатам.
* Ленте.
* Комментариям.
* Галереям.
* Анимациям.
* Переключению экранов.

## 10. Нагрузочное тестирование

Проверить:

* Одновременные загрузки.
* Несколько активных чатов.
* Большие истории.
* Массовую отправку сообщений.
* Несколько серверов федерации.
* Потерю соединения.
* Повторное подключение.
* Параллельное декодирование.

## 11. Сравнение до/после

Для каждого исправления сохранять:

* Baseline.
* Test scenario.
* Hardware.
* Software version.
* Measurement method.
* Result before.
* Result after.
* Regression status.

Не заявлять процент ускорения без воспроизводимых измерений.

## 12. Результаты

Создать:

* PERFORMANCE_AUDIT.md
* DATABASE_AUDIT.md
* MEDIA_PIPELINE.md
* FRAME_PROCESSING.md
* MEMORY_AUDIT.md
* BENCHMARKS.md

Приоритет — реальные причины проблем, а не косметические оптимизации.
