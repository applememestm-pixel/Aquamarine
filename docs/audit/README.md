# Аудит CeZeOs + Aquamarine

Документы аудита по комплекту требований владельца: MASTER_PROMPT и шесть специализированных документов.
Общие документы обоих проектов лежат здесь. Часть CeZeOs — в `cezeos/docs/audit/`.

## Снимки кода

| Репозиторий | База аудита | Ветка исправлений |
|---|---|---|
| Aquamarine | `1268b604` (затем слит `main` до `42b2fdc9`, PR #80) | `ccr-b9de7763-pujvr8` → PR #81 |
| CeZeOs | `182c5b9` | `ccr-b9de7763-pujvr8` → PR #1 |

## Статусы

- **CONFIRMED** — дефект прочитан в коде лично и воспроизведён тестом.
- **CONFIRMED (code)** — дефект прочитан в коде, теста пока нет.
- **REPORTED** — найден при инвентаризации, лично ещё не перепроверен.
- **NOT TESTED / UNVERIFIED** — проверка невозможна в этой среде; причина указана.

Предположения не выдаются за дефекты.

## Ограничения среды

Ограничения действуют на всё время работы. Каждое указывается как причина статуса NOT TESTED.

- npm и crates.io локально закрыты. Rust собирается офлайн из кэша без `aqua-stt` (нет крейта `ort`). Клиент (`aqua-client-ui`, Svelte) проверяется только в CI.
- Android-устройств нет, GPU нет, замерить FPS на железе нельзя.
- Прямого доступа к GPT-6.2 нет. Пакет для независимого аудита — в `gpt/`. Запускает аудит владелец.
- Branch protection и required checks настраиваются в GitHub владельцем.

## Документы

| Документ | Статус |
|---|---|
| [SECURITY_AUDIT.md](SECURITY_AUDIT.md) — реестр дефектов безопасности (P0–P2) | этап 1 |
| [ARCHITECTURE_MAP.md](ARCHITECTURE_MAP.md) — карта, диаграммы, границы проектов | этап 1 |
| [MODULE_INVENTORY.md](MODULE_INVENTORY.md) — карточки модулей | этап 1 |
| [TEST_RESULTS.md](TEST_RESULTS.md) — что запускалось и с каким результатом | этап 1 |
| [RELEASE_READINESS.md](RELEASE_READINESS.md) — release gate | этап 1 |
| [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md) — порядок P1→P3 | этап 1 |
| [gpt/GPT_AUDIT_PACKAGE.md](gpt/GPT_AUDIT_PACKAGE.md) — задание и материалы для GPT-6.2 | этап 1 |

Остальные документы из требований создаются по мере работ, каждый с фактами, а не шаблоном:

- CRYPTOGRAPHY_AUDIT, FEDERATION_SECURITY, KEY_LIFECYCLE, PQ_ROADMAP, SECURITY_TESTS;
- PERFORMANCE_AUDIT, DATABASE_AUDIT, MEDIA_PIPELINE, FRAME_PROCESSING, MEMORY_AUDIT, BENCHMARKS;
- UI_AUDIT, DESIGN_SYSTEM_AUDIT, ANIMATION_AUDIT, RENDERING_BENCHMARKS, ANDROID_AUDIT, NOTIFICATIONS_AUDIT;
- FUNCTIONAL_MATRIX, FEATURE_DETAILS, COMPETITOR_COMPARISON, AQUAMARINE_MISSING_FEATURES;
- DATA_FLOW, DEPENDENCY_MAP, ARCHITECTURE_ISSUES, TARGET_ARCHITECTURE;
- TEST_COVERAGE, SECURITY_TEST_RESULTS, LOAD_TEST_RESULTS, PERFORMANCE_REGRESSION, GPT_AUDIT_FINDINGS, GPT_REVIEW_RESOLUTION.

Пока документа нет, его тема считается **не проверенной**.
