# Требования к аудиту CeZeOs + Aquamarine

Комплект из семи документов, переданный владельцем 25.11.2026. Это единый комплект требований, а не семь конкурирующих заданий.

- **Главный документ** — [00_MASTER_PROMPT.md](00_MASTER_PROMPT.md).
- Остальные шесть содержат специализированные технические требования.
- Все документы обязательны к исполнению. Разделы не пропускаются, и требования не заменяются упрощёнными решениями.
- Порядок работы: инвентаризация репозиториев → план → согласование критических архитектурных решений → реализация по приоритетам.
- Работа не объявляется завершённой без тестирования и подготовки материалов для независимого аудита GPT-6.2.

| № | Документ | Тема |
|---|---|---|
| 0 | [00_MASTER_PROMPT.md](00_MASTER_PROMPT.md) | MASTER ENGINEERING TASK — полный аудит, исправление, оптимизация и развитие экосистемы |
| 1 | [01_ARCHITECTURE_AUDIT.md](01_ARCHITECTURE_AUDIT.md) | ARCHITECTURE AUDIT — архитектура обоих проектов |
| 2 | [02_SECURITY_CRYPTOGRAPHY_FEDERATION.md](02_SECURITY_CRYPTOGRAPHY_FEDERATION.md) | SECURITY, CRYPTOGRAPHY & FEDERATION |
| 3 | [03_PERFORMANCE_DATABASES_MEDIA.md](03_PERFORMANCE_DATABASES_MEDIA.md) | PERFORMANCE, DATABASES & MEDIA PIPELINE |
| 4 | [04_UI_UX_ANIMATIONS_ANDROID.md](04_UI_UX_ANIMATIONS_ANDROID.md) | UI/UX, ANIMATIONS & ANDROID QUALITY |
| 5 | [05_FUNCTIONAL_AUDIT.md](05_FUNCTIONAL_AUDIT.md) | COMPLETE FUNCTIONAL AUDIT — AQUAMARINE |
| 6 | [06_QA_INDEPENDENT_GPT_AUDIT.md](06_QA_INDEPENDENT_GPT_AUDIT.md) | QUALITY ASSURANCE & INDEPENDENT GPT AUDIT |

Где исполнение: результаты лежат в [`docs/audit/`](../README.md).
