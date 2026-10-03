# CI/CD для Scrobb: как устроить всё бесплатно

> Статус: **черновик** (04.10.2026). Отвечает на вопрос Ивана: «если репозиторий на GitHub приватный, там много ограничений — как организовать CI/CD бесплатно». Стек и деплой взяты из [architecture.md](architecture.md) (§6, §7): Python 3.12 + FastAPI, очередь за интерфейсом `jobs`, PostgreSQL 16, Valkey (два экземпляра), Next.js, Telegram-бот (aiogram), Docker Compose на одном VPS в РФ (152-ФЗ), Prometheus/Grafana, GlitchTip, S3-совместимое хранилище. Пометка **(не проверено)** означает, что факт не подтверждён первоисточником или не проверен на практике. Номера источников в тексте вида [N], список в конце. Цифры GitHub сверены с docs.github.com 04.10.2026.

## Коротко

1. **Минуты — не главная проблема приватного репозитория.** Self-hosted раннеры на GitHub бесплатны и без лимита минут, причём и для приватных репозиториев [1][2]. Плату $0.002/мин за self-hosted раннеры GitHub анонсировал на 01.03.2026, но 17.12.2025 отложил «для пересмотра» и пока не ввёл [3][4]. На GitHub-hosted раннерах в приватном репозитории Free даёт **2 000 минут в месяц**, 500 МБ под артефакты и пакеты и 10 ГБ кэша на репозиторий [1].
2. **Настоящие ограничения приватного репозитория на Free — в защите и безопасности.** Недоступны protected branches и rulesets, environments (вместе с environment secrets и required reviewers), CodeQL code scanning, secret scanning и push protection [5][6][7][8]. Dependabot (alerts, security updates, version updates) работает и в приватных репозиториях [9].
3. **Платить из РФ проблематично.** С марта 2022 карты Visa и Mastercard российских банков не работают за рубежом [10]. Поэтому купить Pro или Team (Team стоит от $4 за пользователя в месяц [11]) или докупить минуты сложно. Без способа оплаты GitHub не выставляет счёт за перерасход, а просто блокирует запуски после исчерпания квоты [1]. Значит, бесплатная квота — жёсткий потолок.
4. **Риски из РФ:** аккаунт могут ограничить из-за санкций. Тогда приватные репозитории недоступны, и скачать их нельзя [12]. С мая 2026 года доступ к GitHub из РФ деградирует: по данным OONI, 10–16% неудачных соединений, а тяжёлые `clone`/`push` обрываются чаще [13][14]. Поэтому **зеркало репозитория вне GitHub обязательно**.
5. **Рекомендация:** остаёмся на GitHub. Репозиторий делаем приватным, план Free. CI сначала гоняем на GitHub-hosted раннерах в пределах 2 000 минут; одна переменная репозитория переключает тяжёлые джобы на свой self-hosted раннер. Отсутствующие защиты закрываем соглашениями, проверками в CI и ограничениями на стороне VPS. Секреты приложения в GitHub не храним. Образы кладём в GHCR (хранение и трафик Container registry сейчас бесплатны [15]), а на случай проблем со связностью держим запасной реестр в РФ. Деплой — по SSH с forced command, с health check и автоматическим откатом. Каждую ночь зеркалируем репозиторий в GitVerse или в свой Forgejo. Подробности — в §6.

---

## 1. GitHub Free: что бесплатно в публичном и приватном репозитории

Аккаунт `fighxy` — личный (personal account), поэтому применяется **GitHub Free for personal accounts**. Сейчас репозиторий **публичный**.

| Возможность | Публичный репозиторий (Free) | Приватный репозиторий (Free) | Источник |
|---|---|---|---|
| Минуты GitHub-hosted раннеров (standard) | **Бесплатно, без лимита** | **2 000 мин/мес**; каждая джоба округляется вверх до целой минуты | [1][16] |
| Self-hosted раннеры | Бесплатно (но см. §5: на публичном репозитории опасно) | **Бесплатно, без лимита минут** | [1][2] |
| Цена минуты сверх квоты | — | Linux 2-core $0.006, Linux 1-core $0.002, Windows $0.010, macOS $0.062 (цены с 01.01.2026) | [1][16] |
| Поведение без способа оплаты | — | После исчерпания квоты запуски **блокируются** | [1] |
| Хранилище артефактов Actions | Бесплатно | **500 МБ**, общие с GitHub Packages (кроме Container registry) | [1] |
| Кэш Actions | 10 ГБ на репозиторий | 10 ГБ на репозиторий; записи, к которым не обращались 7 дней, удаляются | [1][17] |
| GHCR (Container registry) | Бесплатно; публичные образы можно скачивать анонимно | Хранение и трафик Container registry **«сейчас бесплатны»**; об изменениях обещают предупредить минимум за месяц | [15][18] |
| Прочие пакеты (npm, Maven и др.) | Бесплатно | 500 МБ хранения + 1 ГБ трафика в месяц; скачивание из Actions с `GITHUB_TOKEN` не считается | [15] |
| Параллельные джобы на hosted-раннерах | 20 (Free) | 20 (Free) | [16] |
| Максимальная длительность джобы | 6 ч на hosted, 5 суток на self-hosted; в очереди self-hosted — до 24 ч | то же | [16] |
| Protected branches | ✅ | ❌ (нужен Pro или Team) | [5][19] |
| Rulesets (branch/tag) | ✅ | ❌ (нужен Pro или Team); push rulesets — только Team | [6] |
| Environments, environment secrets, deployment branches | ✅ | ❌ (нужен Pro, Team или Enterprise) | [7] |
| Required reviewers и wait timer для environments | ✅ | ❌ — **даже на Pro и Team** только для публичных репозиториев | [7] |
| Repository secrets (`secrets.*`) | ✅ | ✅ | [20] |
| Dependabot alerts, security updates, version updates | ✅ | ✅ | [9] |
| Dependency graph | Включён | Включается вручную | [20] |
| Dependency review (action) | ✅ | ❌ (организация на Team + Code Security) | [8] |
| Code scanning (CodeQL) | ✅ | ❌ (организация на Team/Enterprise + Code Security) | [8] |
| Secret scanning, push protection | ✅ (push protection включён по умолчанию) | ❌ для личного аккаунта | [8] |
| Docker Hub rate limit при pull | На GitHub-hosted раннерах для публичных образов не действует | то же; на self-hosted — **действует** (100 pull / 6 ч анонимно, 200 — для авторизованного Personal) | [16][21] |

Главные выводы:

- **Публичный репозиторий** — самый щедрый бесплатный вариант. Безлимитные hosted-минуты, все защиты веток, environments с ручным подтверждением, CodeQL, secret scanning. Минусы: код виден всем, а self-hosted раннер на нём опасен (§5). Есть и неожиданный плюс: при санкционных ограничениях GitHub сохраняет доступ к публичным репозиториям, а к приватным — нет [12].
- **Приватный репозиторий на Free** — минут хватает, если пайплайн экономный (§7) или есть self-hosted раннер. Но «предохранителей» нет: в `main` может запушить любой с правом записи, все секреты видны любому workflow из любой ветки, нет кнопки «подтвердить деплой в prod». Это компенсируем сами (§6.5, §6.7).
- Даже платные Pro и Team **не дают required reviewers для приватных environments** [7], поэтому подписка ради ручного подтверждения деплоя не поможет.

## 2. Self-hosted раннеры: бесплатны ли они в 2026 году

- **Сейчас бесплатны.** Документация GitHub (04.10.2026): «GitHub Actions usage is free for self-hosted runners and for public repositories that use standard GitHub-hosted runners» [1]. То же написано в описании планов [2].
- **История с платой.** 16.12.2025 GitHub объявил: с 01.01.2026 hosted-раннеры дешевеют до 39%, а с 01.03.2026 вводится «Actions cloud platform charge» $0.002/мин для self-hosted раннеров в приватных репозиториях; эти минуты должны были списываться из квоты плана [3][4]. Публичные репозитории и GitHub Enterprise Server изменение не затрагивало. После волны критики GitHub 17.12.2025 написал: «We're postponing the announced billing change for self-hosted GitHub Actions to take time to re-evaluate our approach» [3][22]. Снижение цен на hosted-раннеры вступило в силу 01.01.2026 [1].
- **Статус на октябрь 2026.** Плата не введена: в действующей документации self-hosted указаны как бесплатные [1]. Сторонний трекер цен (cicdpipelinecost.com) на 23.09.2026 тоже пишет, что сбора нет [23]. Но GitHub **не отказался от идеи, а только отложил её** [22]. Закладываем риск: если плату введут, 2 000 минут Free при $0.002/мин хватит примерно на 6 000 self-hosted минут (2000 × $0.006 / $0.002). Это **оценка по анонсированной схеме**, а не действующее правило. В анонсе было сказано, что self-hosted минуты будут расходовать квоту «based on list price» [3].
- **Лимиты self-hosted:** джоба — до 5 суток, ожидание в очереди — до 24 ч [16].

## 3. Риски для проекта из РФ

| Риск | Что известно | Что делаем |
|---|---|---|
| **Санкционная блокировка аккаунта** | В FAQ GitHub о санкциях Россия **не входит** в список регионов с ограничениями. Там перечислены Крым, ДНР/ЛНР, Куба, Иран и КНДР; для Ирана и Кубы сервисы восстановлены [12]. Но ограничения действуют для SDN-лиц и всех, кто работает «for or on behalf of» таких лиц. Местоположение определяется в том числе по IP и истории платежей [12]. В 2022 году GitHub блокировал аккаунты разработчиков из подсанкционных компаний, в том числе бывших сотрудников [24]. **При ограничении приватные репозитории недоступны, скачать или удалить их нельзя**; публичные остаются доступны [12] | Ежедневное зеркало репозитория вне GitHub (§6.10). Не связывать аккаунт с подсанкционными организациями |
| **Оплата** | Карты Visa и Mastercard российских банков не работают за рубежом с марта 2022 [10] | Считаем бесплатные квоты жёстким потолком. Pro или Team — только если у кого-то есть зарубежная карта |
| **Связность GitHub ↔ РФ** | С 05.05.2026 OONI фиксирует 10–16% неудачных соединений к GitHub при обычном фоне ≤4%. РКН блокировку отрицает [13]. Тяжёлые `git clone`/`push` обрываются чаще лёгких запросов [14]. 01.06.2026 был разовый сбой доступа к PyPI из РФ [14] | Self-hosted раннер в РФ и pull образов из GHCR на VPS в РФ могут сбоить: ретраи, `fetch-depth: 1`, компактные образы, запасной реестр в РФ (§6.4) |
| **Docker Hub** | Лимит pull: 100 за 6 ч на IP без авторизации, 200 за 6 ч для авторизованного Personal [21]. Стабильность доступа к Docker Hub из РФ **не проверена**. У GitVerse есть публичное зеркало `dh-mirror.gitverse.ru` [25] | На VPS и self-hosted раннере: `docker login` и/или `registry-mirrors` |
| **GitLab.com** | В политике trade compliance GitLab Россия значится среди стран под эмбарго США [26]. В исходном коде GitLab `RU` входит в `COUNTRY_DENYLIST` [27] (на что именно влияет этот список — выбор страны при покупке, триалы и т. п. — **не проверено**). Для instance runners на GitLab.com Free нужна верификация личности (телефон или карта) [28] | GitLab.com как основную площадку не рассматриваем |

## 4. Альтернативы GitHub

| Вариант | Приватный репозиторий бесплатно | CI-минуты на чужих раннерах | Свои раннеры | Реестр образов | Защита веток на бесплатном приватном | Синтаксис CI | Доступность из РФ / риски | Сколько возни |
|---|---|---|---|---|---|---|---|---|
| **GitHub, публичный** | — | Без лимита | Есть, но опасно (§5) | GHCR, бесплатно | ✅ всё | GitHub Actions | Санкции: публичные остаются доступны [12]; связность деградирует [13] | Минимум |
| **GitHub Free, приватный** | ✅ | 2 000 мин/мес | ✅ бесплатно, без лимита [1] | GHCR («сейчас бесплатно» [15]) | ❌ [5][6] | GitHub Actions | Риск блокировки приватных репозиториев [12]; оплата из РФ [10] | Минимум + свой раннер |
| **GitLab.com Free** | ✅ (до 5 пользователей в приватной группе) | 400 мин/мес [29] | ✅ (minutes не тратятся) | GitLab Registry | Protected branches есть (детали Free **не проверены**) | `.gitlab-ci.yml` | Эмбарго РФ в политике [26][27]; верификация для раннеров [28] | Средне |
| **Forgejo (self-hosted) + Forgejo Actions** | ✅ без лимитов | Нет (только свои) | ✅ Forgejo Runner (Docker, Podman, LXC, host) [30] | Встроенный OCI-реестр [31] | ✅ (свой сервер) | Похож на GitHub Actions, но «not designed to be compatible» [32] | Данные в РФ, санкции не грозят | Высокая: свой сервер, бэкапы, обновления |
| **Gitea + Gitea Actions** | ✅ | Нет | ✅ act_runner | Встроенный реестр | ✅ | Похож на GitHub Actions | Как у Forgejo | Высокая |
| **Woodpecker CI** (поверх GitHub, Gitea, Forgejo, GitLab) | Зависит от forge | Нет | ✅ агенты | Любой | Зависит от forge | Свой YAML | Self-hosted | Средне, поверх forge [33] |
| **GitVerse** (СберТех) | ✅ | Приватные 500 мин/мес, публичные 1 000; у верифицированных организаций 1 000 и 2 000; облачная джоба ≤30 мин; артефакты 500 МБ на все репозитории, хранятся 30 дней [34] | ✅ локальные раннеры (act_runner), лимиты задаёте сами [35] | Пакеты Docker (лимиты **не проверены**); зеркала Docker Hub, PyPI, npm [25] | **Не проверено** | Workflow в стиле GitHub Actions (совместимость **не проверена**) | Российский сервис; в 09.2026 отключили старый endpoint `gitverse.ru/sc` для раннеров — платформа активно меняется [35] | Низкая–средняя |
| **SourceCraft** (Яндекс) | ✅ | 1 000 мин/мес для публичных и приватных (тарифы с 01.10.2026), 5 параллельных процессов, облачный воркер 4 vCPU / 8 ГБ, задание ≤3 600 с [36][37] | ✅ self-hosted воркеры, квоту минут не расходуют [38] | Через Yandex Container Registry (платный, **не проверено**) | ❌ на Free «Политики веток» только для публичных репозиториев [36] | Свой YAML (`workflows → tasks → cubes`) [38]; заявлен автоконвертер из GitHub Actions [39] | Российский сервис, интеграция с Yandex Cloud | Низкая–средняя |

Выводы по альтернативам:

- **Forgejo + Forgejo Actions** — единственный вариант без чужих лимитов и санкционных рисков. Но это ещё один сервис, который надо держать живым, бэкапить и обновлять, причём **не на prod VPS** (иначе сбой prod уносит и CI, и код). Для команды из 1–3 человек на этапе MVP это лишняя работа. Держим его как план Б и как цель зеркала.
- **GitVerse и SourceCraft** — бесплатные приватные репозитории с CI из РФ. Хорошо подходят на роль зеркала и запасной площадки. Переезд CI на SourceCraft означает переписать workflow: у него свой формат. Для GitVerse объём правок **не проверен**.
- **GitLab.com** из-за эмбарго и верификации — нет.

## 5. Безопасность self-hosted раннеров

GitHub прямо пишет: self-hosted раннер **не гарантирует чистую эфемерную среду** и может быть скомпрометирован навсегда, поэтому «should almost never be used for public repositories» — любой может открыть PR с кодом, который выполнится на вашей машине [40][41]. Настройка «Approval for running fork pull request workflows» лишь просит подтверждения для части авторов. После одобрения или после первого принятого PR (даже опечатки) код выполняется автоматически [42].

Правила для Scrobb:

1. **Self-hosted раннер — только в приватном репозитории.** Если репозиторий остаётся публичным, используем только GitHub-hosted раннеры: они и так бесплатные и безлимитные.
2. **Не ставим раннер на prod VPS** для сборок и тестов. `pip install`/`pnpm install` выполняет код зависимостей. Скомпрометированный пакет на prod-хосте получает доступ к БД с персональными данными (152-ФЗ). Docker socket фактически даёт root на хосте. Раннер живёт на отдельной машине: домашнем ПК или мини-сервере (бесплатно), либо на отдельном дешёвом VM.
3. **Изоляция:** отдельный пользователь без sudo; раннер в режиме `--ephemeral` или JIT (одна джоба — и раннер снимается с регистрации [41]) внутри контейнера или VM, которые пересоздаются. На машине раннера нет SSH-ключей от prod, облачных токенов и домашних данных. Сеть — отдельный VLAN или хотя бы отдельный хост.
4. **Деплой-ключ** хранится в GitHub Secrets, а не на раннере. На VPS он ограничен forced command (§6.6), поэтому даже при утечке им можно только запустить `deploy.sh` с тегом.
5. **Supply chain:** пиним actions по полному SHA. 19–20.03.2026 злоумышленники перезаписали 76 из 77 тегов `aquasecurity/trivy-action` на вредонос, крадущий секреты; безопасной осталась только 0.35.0 [43][44]. Это единственный способ сделать ссылку на action неизменяемой [40]. Минимальные `permissions:` в каждом workflow (по умолчанию `contents: read`). Без `pull_request_target` с checkout кода из PR [40].
6. **Docker Hub на self-hosted:** лимиты pull действуют [16][21] — нужен `docker login` или зеркало.

## 6. Рекомендуемая бесплатная схема

### 6.1 Решение по репозиторию

| Ситуация | Что делаем |
|---|---|
| Готовы держать код открытым (лицензия, нет коммерческих секретов в коде) | **Публичный репозиторий**: hosted-минуты без лимита, protected branches, environments с required reviewers, CodeQL, secret scanning. Self-hosted раннер не используем |
| Код должен быть закрыт (коммерческий продукт, антиабуз-логика) — **ожидаемый вариант** | **Приватный репозиторий на Free** + схема ниже. Отсутствующие функции компенсируем (§6.5–6.7). Зеркало обязательно |

Дальше описан приватный вариант. Для публичного всё то же, только вместо самодельных «ворот» используются штатные environments и rulesets.

### 6.2 Где выполняются джобы

- **По умолчанию — GitHub-hosted `ubuntu-latest`**: чистая VM на каждую джобу, ничего не нужно обслуживать, связность с PyPI, npm и Docker Hub хорошая.
- **Переключатель на self-hosted.** Во всех workflow пишем `runs-on: ${{ fromJSON(vars.RUNNER || '"ubuntu-latest"') }}`. Если задать переменную репозитория `RUNNER` = `["self-hosted","linux","scrobb"]`, все джобы уедут на свой раннер. Переключаем, когда минуты подходят к концу (уведомления GitHub приходят на 90% и 100% квоты [1]) или если hosted-раннеры станут недоступны. Работа выражения `vars` в `runs-on` на нашем репозитории — **проверить при внедрении**.
- **Self-hosted раннер:** домашняя машина (бесплатно, соединения только исходящие, белый IP не нужен) или отдельный VM. Docker обязателен — нужен для service containers и сборки образов.
- **Порядок оценки бюджета.** 2 000 мин ÷ (сумма минут всех джоб одного прогона) = число прогонов в месяц. Например, при 4 джобах по ~3 мин (с округлением вверх [16]) прогон стоит ~12 мин, и в месяц помещается ~160 прогонов. Это **иллюстрация**, реальные цифры снимем после первых недель.

### 6.3 Стадии пайплайна

**На каждый PR в `main` (и на push в `main`):**

| Джоба | Что делает | Инструменты |
|---|---|---|
| `changes` | Определяет затронутые части: `backend/**`, `web/**`, `migrations/**`, `docker/**`, `docs/**` | `dorny/paths-filter` (по SHA) |
| `backend` | Линт, типы, тесты. Тесты используют **service containers**: `postgres:16` и `valkey/valkey` (та же версия, что в prod). Отдельный контейнер на каждый экземпляр Valkey (очередь и кэш) — как в prod | `ruff check` (включая правила `S` из flake8-bandit), `ruff format --check`, `mypy`, `pytest` |
| `migrations` | `alembic upgrade head` на пустой БД; проверка, что head-ревизия одна; `alembic check` (autogenerate не находит расхождений модели и схемы) | Alembic, Postgres 16 service |
| `web` | Линт, типы, сборка | `eslint`, `tsc --noEmit`, `next build` |
| `security` | Секреты в диффе, уязвимые зависимости | `gitleaks` (CLI), `osv-scanner` или `pip-audit` + `pnpm audit` |
| `ci-gate` | `if: always()`, падает, если какая-то из нужных джоб упала. **Единственная проверка, на которую смотрим перед merge** | — |

В architecture.md (§6, таблица стека) для тестов упомянут testcontainers-postgres. В CI проще service containers: их поднимает сам раннер, Docker-in-Docker не нужен. testcontainers остаётся удобным локально. Выбрать одно — **на этапе кода**.

**На push в `main` (после зелёного CI):**

1. **Сборка образов** `docker buildx`: `scrobb-backend` (один образ, роль задаётся `ROLE=api|worker|scheduler|bot`, architecture.md §7.6) и `scrobb-web`. Тег — `sha-<7 символов>`, неизменяемый. `latest` в prod не используем.
2. **Сканирование образа:** `trivy image --severity CRITICAL,HIGH --exit-code 1 --ignore-unfixed`. Trivy ставим бинарником с проверкой checksum или через action, закреплённый по SHA (после инцидента [43]). Альтернатива — Grype.
3. **Push в реестр** (§6.4).
4. **Деплой в staging** (§6.6) и smoke-тест: `/healthz`, `/readyz`, тестовый скроббл через ListenBrainz-совместимый API.

**Релиз в prod:**

1. Тег `vX.Y.Z` (или `vYYYY.MM.DD`) на коммите из `main`, который уже прошёл staging.
2. Workflow `promote` **перетегирует тот же digest** (`sha-…` → `vX.Y.Z`) без пересборки: в prod попадает ровно то, что проверено на staging.
3. Ручной запуск `deploy-prod` (`workflow_dispatch`, вход — тег). Дальше на VPS: бэкап → pull → миграции → `up --wait` → health check → откат при ошибке.

**По расписанию (раз в неделю, ночью):** полное сканирование `trivy fs` и `trivy image` для текущего prod-тега, проверка восстановления бэкапа (architecture.md, раздел о бэкапах) и зеркалирование репозитория (§6.10) — его лучше запускать ежедневно.

### 6.4 Реестр образов

| Вариант | Стоимость | Плюсы | Минусы |
|---|---|---|---|
| **GHCR** — основной | Container registry «currently free» [15] | Push из Actions через `GITHUB_TOKEN`, привязка к репозиторию, права наследуются от репозитория [18] | Для pull на VPS нужен **classic PAT** с `read:packages`: fine-grained токены GitHub Packages не поддерживает [18]. Classic PAT читает все пакеты аккаунта, поэтому лучше завести отдельного machine-user. Pull больших слоёв из РФ может сбоить [13][14]. Политика «бесплатно» может измениться (обещают предупредить за месяц [15]) |
| **Свой реестр** (CNCF Distribution `registry` или Zot) за Caddy с TLS и auth — запасной | ₽0 сверх диска VPS | Данные в РФ, никаких квот | Обслуживание: garbage collection старых тегов, бэкап не нужен (образы можно пересобрать), но нужен доступ на push снаружи |
| **Реестр GitVerse** | Бесплатно (лимиты **не проверены**) | В РФ | Ещё одна учётка и токен |
| **Без реестра:** `docker save \| ssh vps docker load` | ₽0 | Работает при любых проблемах с реестрами | Каждый раз передаётся весь образ, нет истории в реестре |

**Откат не должен зависеть от реестра:** на VPS храним локально предыдущие 3–5 тегов образов и чистим только более старые (`docker image prune` с фильтром).

### 6.5 Ветки и правила (без branch protection)

- **Trunk-based:** `main` всегда готов к деплою, короткоживущие ветки `feat/*`, `fix/*`, merge только через PR в режиме squash. Релизы — git-теги.
- **На приватном Free `main` защитить нельзя** [5][19]. Компенсируем так:
  - договорённость «в `main` только через PR с зелёным `ci-gate`»; в настройках репозитория включить auto-delete веток;
  - CI запускается и на `push` в `main`, а деплой в staging стоит в `needs:` от CI: прямой push с ошибкой не уедет даже в staging;
  - `deploy-prod` перед запуском через API проверяет, что тег указывает на коммит из `main`, что у этого SHA зелёный CI и что существует образ `sha-…`. Это **мягкая** проверка: участник с правом записи может изменить workflow;
  - **жёсткие** проверки — на VPS: `deploy.sh` для prod принимает только теги вида `v*`, существующие в реестре (§6.6).
- Если появится возможность платить, Pro или Team за $4/пользователь/мес [11] вернёт protected branches и rulesets в приватном репозитории [5][6], но не required reviewers для environments [7].

### 6.6 Деплой на VPS: SSH, compose, health check, откат

Схема: CI → `ssh deploy@vps <env> <tag>` → на VPS выполняется **только** `/opt/scrobb/bin/deploy.sh`.

- В `~deploy/.ssh/authorized_keys` ключ CI прописан с ограничениями: `command="/opt/scrobb/bin/deploy.sh",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty`. Скрипт берёт аргументы из `SSH_ORIGINAL_COMMAND` и валидирует их по белому списку (`staging|prod`, тег по регулярному выражению).
- Для staging и prod — **разные ключи**: `STAGING_SSH_KEY` и `PROD_SSH_KEY` в repository secrets, плюс `SSH_KNOWN_HOSTS`. Без known_hosts от `StrictHostKeyChecking` не отключаемся.
- Пользователь `deploy` состоит в группе `docker` (это фактически root, отсюда forced command). Вход по паролю выключен, `fail2ban` включён. Порт SSH открыт для мира, потому что диапазоны IP GitHub-hosted раннеров огромные. Альтернатива — деплой с self-hosted раннера через VPN или WireGuard до VPS.

Логика `deploy.sh` (набросок):

```bash
#!/usr/bin/env bash
set -euo pipefail
read -r ENV TAG <<<"${SSH_ORIGINAL_COMMAND:-}"
[[ "$ENV" =~ ^(staging|prod)$ ]] || { echo "bad env"; exit 2; }
if [[ "$ENV" == prod ]]; then [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 2
else [[ "$TAG" =~ ^sha-[0-9a-f]{7,40}$ ]] || exit 2; fi

cd "/opt/scrobb/$ENV"
exec 9>.deploy.lock; flock -n 9 || { echo "deploy in progress"; exit 3; }
PREV=$(cat .current_tag 2>/dev/null || echo "")
export IMAGE_TAG="$TAG"                      # compose.yaml: image: …/scrobb-backend:${IMAGE_TAG}

docker compose pull
[[ "$ENV" == prod ]] && ./bin/backup-before-migrate.sh   # pg_dump -Fc или отметка для PITR
docker compose run --rm --no-deps migrate              # ROLE=migrate → alembic upgrade head

if docker compose up -d --wait --wait-timeout 120 && ./bin/smoke.sh; then
  echo "$TAG" > .current_tag; echo "$PREV" > .previous_tag
else
  echo "health check failed, rolling back to $PREV" >&2
  [[ -n "$PREV" ]] && IMAGE_TAG="$PREV" docker compose up -d --wait --wait-timeout 120
  ./bin/notify.sh "deploy $ENV $TAG failed, rolled back to $PREV"
  exit 1
fi
```

- `docker compose up --wait` ждёт, пока сервисы станут `running` или `healthy` [45]. Поэтому у каждого сервиса в `compose.yaml` есть `healthcheck`: `api` — `GET /healthz` (жив ли процесс), `web` — свой health-роут, `worker`/`scheduler`/`bot` — heartbeat-ключ в Valkey или файл с меткой времени.
- `smoke.sh` проверяет снаружи через Caddy: `/readyz` (БД и оба Valkey отвечают, ревизия Alembic = head), статус-код главной страницы, тестовый скроббл на staging.
- **Простой при деплое:** на одном VPS `up -d` перезапускает контейнеры, это секунды простоя (**не измерено**). Клиенты-скробблеры повторяют неудачные отправки (поведение отдельных клиентов **не проверено**). Blue/green через два набора контейнеров за Caddy — позже, если понадобится.
- **Откат кода не откатывает БД** — см. §6.8.

### 6.7 Секреты

Основная проблема приватного Free: environment secrets нет [7], а любой с правом записи может прочитать все repository secrets через workflow [40]. Поэтому:

- **В GitHub — только секреты доставки:** `STAGING_SSH_KEY`, `PROD_SSH_KEY`, `SSH_KNOWN_HOSTS`, при необходимости токен зеркала (§6.10). Для GHCR достаточно `GITHUB_TOKEN` с `packages: write` в одной джобе.
- **Секреты приложения в GitHub не попадают** (`DATABASE_URL`, ключи ЮKassa, токен Telegram-бота, S3, Last.fm API key, `SECRET_KEY`). Они живут на VPS:
  - вариант 1 (проще): `/opt/scrobb/<env>/.env`, права `0600`, владелец `deploy`; изменения вручную по SSH;
  - вариант 2 (лучше): **sops + age** — зашифрованный `secrets.<env>.enc.env` лежит в репозитории (видно, кто и когда менял), приватный age-ключ есть **только на VPS**, `deploy.sh` расшифровывает файл перед `up`.
- `permissions:` по умолчанию `contents: read`; `packages: write` — только в джобе push. Не печатать секреты в лог и не хранить в одном секрете JSON или YAML: маскирование логов это ломает [40].
- Ротация: деплой-ключи — при уходе участника и не реже раза в год (**политика, предложение**).
- Пути к ПДн: в CI используются только синтетические данные. Дампы prod-БД в CI и на staging не попадают (152-ФЗ).

### 6.8 Миграции Alembic

1. **Отдельный шаг деплоя**, а не при старте контейнера (architecture.md §7.6): одноразовый контейнер `migrate` из **нового** образа выполняет `alembic upgrade head` до `up`.
2. **Expand → migrate → contract.** Любая миграция совместима с предыдущей версией кода: сначала добавляем колонку или таблицу (nullable или с default), выкатываем код, который пишет в обе схемы, и только в следующем релизе удаляем старое. Тогда откат образа (§6.6) безопасен без `alembic downgrade`.
3. **`downgrade` в prod автоматически не запускаем.** Восстановление — из бэкапа, сделанного перед миграцией, или через PITR.
4. **Защита от долгих блокировок:** в `env.py` задаём `SET lock_timeout = '5s'` и `statement_timeout`. Индексы на больших таблицах — `CREATE INDEX CONCURRENTLY` в `autocommit_block()`. Секции `listens` создаёт `pg_partman`, а не Alembic (architecture.md).
5. **Бэкфилл данных** — фоновая задача в очереди, а не в миграции.
6. **В CI:** `upgrade head` на пустой БД, одна head-ревизия, `alembic check`. Перед релизом с миграциями — прогон на копии схемы staging.

### 6.9 Окружения staging и prod

- **staging** — на том же VPS отдельный compose-проект `scrobb-staging`: свои контейнеры Postgres и двух Valkey, ограничения `mem_limit`/`cpus`, поддомен `staging.…` за basic auth в Caddy, только синтетические данные. Деплоится автоматически после каждого зелёного `main`. Если ресурсов VPS не хватает (оценка нагрузки в architecture.md), staging поднимаем только на время проверки релиза или откладываем до беты.
- **prod** — только из тега, ручным запуском, с бэкапом перед миграцией.
- На приватном Free штатных environments нет [7], поэтому окружения разделены на уровне отдельных workflow (`deploy-staging.yml`, `deploy-prod.yml`), отдельных SSH-ключей и проверок в `deploy.sh`. Для публичного репозитория — environments `staging` и `prod` с required reviewer на `prod` [7].

### 6.10 Зеркало и план Б

- Ежедневный workflow (и при push тегов) делает `git push --mirror` в **GitVerse** (или в свой Forgejo). Токен зеркала — repository secret с правом записи только в репозиторий-зеркало.
- Раз в месяц проверяем, что из зеркала можно собрать проект.
- Если GitHub ограничит аккаунт или станет недоступен из РФ: переключаем origin на зеркало, раннеры регистрируем там (у GitVerse act_runner [35]), реестр — свой или GitVerse. Workflow придётся адаптировать: насколько — **не проверено**.

### 6.11 Обновление зависимостей: Dependabot или Renovate

- **Dependabot** бесплатен и в приватных репозиториях: alerts, security updates, version updates [9]. Конфиг `.github/dependabot.yml`: экосистемы `pip` (или `uv`), `npm` (pnpm), `docker`, `github-actions`. Расписание — раз в неделю, обновления **группируем** (`groups:`), чтобы не плодить PR и не тратить минуты. Поддержку `uv` и `docker-compose` в Dependabot проверить при настройке (**не проверено**).
- Dependabot обновляет и actions, закреплённые по SHA, если версия указана комментарием на той же строке. Но alerts для SHA-pinned actions он **не создаёт** [40].
- **Renovate** — если Dependabot будет шуметь: гибче группировка, есть automerge для patch-версий, обновление digest в `compose.yaml` и lockfile maintenance [46]. Можно запускать self-hosted из Actions по расписанию (тратит минуты) или через приложение Mend (условия 2026 года **не проверены**).
- На приватном Free нет dependency review и CodeQL [8]. Их заменяют `osv-scanner`/`pip-audit` в PR и правила `S` в ruff. Semgrep CE — по желанию.

## 7. Как экономить минуты и место

1. **Фильтры по путям.** Изменения в `docs/**` не запускают тесты. Используем `dorny/paths-filter` внутри одного workflow плюс джобу `ci-gate`, а не `on.push.paths`: если весь workflow пропущен, обязательная проверка зависает в Pending [47]. Для приватного Free это не так критично (обязательных проверок нет), но схема будет готова к Pro или публичному репозиторию.
2. **Отмена устаревших прогонов:** `concurrency: { group: ${{ github.workflow }}-${{ github.ref }}, cancel-in-progress: true }` для PR. Для деплоя `cancel-in-progress: false`: деплои встают в очередь, а не обрываются на середине.
3. **Кэш.** `astral-sh/setup-uv` с `enable-cache` (или кэш pip), `actions/setup-node` с `cache: pnpm`, `docker buildx` с `cache-from/to: type=gha,mode=max`. Общий лимит 10 ГБ на репозиторий; записи, к которым не обращались 7 дней, удаляются [17]. На self-hosted слой-кэш Docker сохраняется между запусками сам.
4. **`timeout-minutes`** на каждой джобе (5–20 мин). Зависший тест не должен съедать до 6 ч квоты [16].
5. **Меньше мелких джоб.** Каждая джоба округляется вверх до минуты и тратит время на старт [16]. Линт и типы Python — одной джобой.
6. **Образы не собираем на PR.** Сборка только на `main`; на PR — `docker build` без push и только если менялись `Dockerfile`/`docker/**`.
7. **Артефакты:** `retention-days: 1–3`, образы как артефакты не загружаем (лимит 500 МБ общий с Packages [1]). Логи в лимит не входят [1].
8. **Тяжёлое — по расписанию:** полное сканирование, проверка восстановления бэкапа — раз в неделю.
9. **Dependabot** — группировка и недельное расписание (§6.11).
10. **Переключатель `vars.RUNNER`** (§6.2) — когда квота заканчивается, работу берёт свой раннер.

## 8. Схема пайплайна

Диаграмма проверена рендером через mermaid-cli (`mmdc`).

```mermaid
flowchart TD
  DEV["Разработчик: ветка feat/* → PR в main"] --> PR

  subgraph PR["CI на pull_request (concurrency: cancel-in-progress)"]
    direction TB
    CH["changes: какие части затронуты (paths-filter)"]
    CH --> PY["backend: ruff, mypy, pytest + сервисы Postgres 16 и Valkey"]
    CH --> MIG["migrations: alembic upgrade head на пустой БД, одна head-ревизия"]
    CH --> WEB["web: eslint, tsc, next build"]
    CH --> SEC["security: gitleaks, osv-scanner / pip-audit"]
    PY --> GATE["ci-gate: единственная обязательная проверка"]
    MIG --> GATE
    WEB --> GATE
    SEC --> GATE
  end

  GATE -->|"merge (squash)"| MAIN["push в main"]

  subgraph BUILD["Сборка (только main и теги v*)"]
    direction TB
    IMG["docker buildx: образы backend и web, тег sha-короткий"]
    IMG --> SCAN["trivy image: CRITICAL/HIGH → fail"]
    SCAN --> PUSH["push в registry (GHCR или свой в РФ)"]
  end

  MAIN --> IMG
  PUSH --> STG["deploy staging по SSH (forced command)"]
  STG --> SMOKE["smoke: /healthz, /readyz, тестовый скроббл"]

  SMOKE -->|"git tag vX.Y.Z"| PROMO["promote: тот же digest → тег vX.Y.Z"]
  PROMO --> APPROVE["ручной запуск prod (workflow_dispatch)"]

  subgraph PROD["deploy.sh на prod VPS"]
    direction TB
    BK["pg_dump / снимок перед миграцией"]
    BK --> PULL["docker compose pull"]
    PULL --> M["alembic upgrade head (одноразовый контейнер)"]
    M --> UP["docker compose up -d --wait"]
    UP --> HC{"health check OK?"}
    HC -->|"да"| DONE["запомнить тег как текущий"]
    HC -->|"нет"| RB["rollback: предыдущий тег, up -d --wait"]
  end

  APPROVE --> BK
  RB --> ALERT["алерт в Telegram / Grafana"]
```

## 9. Набросок workflow (для старта, не финальная версия)

Версии и SHA actions подставляем при внедрении. В репозитории — только полный SHA с комментарием-версией [40]. Пакетный менеджер Python (`uv` в примере), структура каталогов (`backend/`, `web/`, `migrations/`) и pnpm — допущения наброска: их выбираем на этапе кода.

```yaml
# .github/workflows/ci.yml
name: ci
on:
  pull_request:
  push:
    branches: [main]
permissions:
  contents: read
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

jobs:
  changes:
    runs-on: ${{ fromJSON(vars.RUNNER || '"ubuntu-latest"') }}
    timeout-minutes: 5
    outputs:
      backend: ${{ steps.f.outputs.backend }}
      web: ${{ steps.f.outputs.web }}
    steps:
      - uses: actions/checkout@<SHA>
      - id: f
        uses: dorny/paths-filter@<SHA>
        with:
          filters: |
            backend: ['backend/**', 'migrations/**', 'docker/**', 'pyproject.toml', 'uv.lock']
            web: ['web/**']

  backend:
    needs: changes
    if: needs.changes.outputs.backend == 'true'
    runs-on: ${{ fromJSON(vars.RUNNER || '"ubuntu-latest"') }}
    timeout-minutes: 20
    services:
      postgres:
        image: postgres:16
        env: { POSTGRES_PASSWORD: test, POSTGRES_DB: scrobb_test }
        ports: ['5432:5432']
        options: >-
          --health-cmd "pg_isready -U postgres" --health-interval 5s --health-retries 20
      valkey-queue:
        image: valkey/valkey:8          # та же версия, что в prod compose
        ports: ['6379:6379']
      valkey-cache:
        image: valkey/valkey:8
        ports: ['6380:6379']
    env:
      DATABASE_URL: postgresql+asyncpg://postgres:test@localhost:5432/scrobb_test
      VALKEY_QUEUE_URL: redis://localhost:6379/0
      VALKEY_CACHE_URL: redis://localhost:6380/0
    steps:
      - uses: actions/checkout@<SHA>
      - uses: astral-sh/setup-uv@<SHA>
        with: { enable-cache: true }
      - run: uv sync --frozen
      - run: uv run ruff check . && uv run ruff format --check .
      - run: uv run mypy backend
      - run: uv run alembic upgrade head && uv run alembic check
      - run: uv run pytest -q

  web:
    needs: changes
    if: needs.changes.outputs.web == 'true'
    runs-on: ${{ fromJSON(vars.RUNNER || '"ubuntu-latest"') }}
    timeout-minutes: 15
    defaults: { run: { working-directory: web } }
    steps:
      - uses: actions/checkout@<SHA>
      - uses: pnpm/action-setup@<SHA>
      - uses: actions/setup-node@<SHA>
        with: { node-version-file: web/.nvmrc, cache: pnpm, cache-dependency-path: web/pnpm-lock.yaml }
      - run: pnpm install --frozen-lockfile
      - run: pnpm lint && pnpm tsc --noEmit && pnpm build

  ci-gate:
    if: always()
    needs: [changes, backend, web]
    runs-on: ubuntu-latest
    timeout-minutes: 2
    steps:
      - run: |
          echo '${{ toJSON(needs) }}' | jq -e 'all(.[]; .result == "success" or .result == "skipped")'
```

Сборка и деплой (`build-deploy.yml`, на `push` в `main` после `ci` через `workflow_run` или `needs` в одном файле) — по §6.3–6.6: `docker/login-action` с `GITHUB_TOKEN` → `docker/build-push-action` с `cache-from/to: type=gha` → `trivy` → `ssh -i STAGING_SSH_KEY deploy@vps staging sha-XXXXXXX`.

## 10. План внедрения

1. Решить: публичный или приватный (§6.1). Если приватный — сразу настроить зеркало в GitVerse (§6.10).
2. `ci.yml` на hosted-раннерах, `dependabot.yml`, pinning по SHA, `permissions: contents: read`.
3. `compose.yaml` с healthcheck, `deploy.sh` с forced command, staging.
4. Сборка образов → GHCR → деплой в staging; затем prod по тегу.
5. Подготовить self-hosted раннер (домашняя машина или VM, ephemeral, Docker, зеркало Docker Hub) и проверить переключатель `vars.RUNNER`.
6. Через 2–4 недели снять фактический расход минут и решить, что оставить на hosted.

## Источники

GitHub — тарифы и лимиты:
- [1] GitHub Docs — GitHub Actions billing (бесплатность self-hosted, квоты Free, цены, блокировка без способа оплаты, общий лимит артефактов и Packages): https://docs.github.com/en/billing/concepts/product-billing/github-actions
- [2] GitHub Docs — GitHub's plans: https://docs.github.com/en/get-started/learning-about-github/githubs-plans
- [3] GitHub Changelog — «Update to GitHub Actions pricing» (16.12.2025, обновлено: перенос платы за self-hosted): https://github.blog/changelog/2025-12-16-coming-soon-simpler-pricing-and-a-better-experience-for-github-actions/
- [4] GitHub — «Pricing changes for GitHub Actions»: https://github.com/resources/insights/2026-pricing-changes-for-github-actions ; roadmap issue #1197: https://github.com/github/roadmap/issues/1197
- [5] GitHub Docs — About protected branches: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches ; условия доступности (исходник docs): https://github.com/github/docs/blob/main/data/reusables/gated-features/protected-branches.md
- [6] GitHub Docs — About rulesets: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets ; доступность: https://github.com/github/docs/blob/main/data/reusables/gated-features/repo-rules.md
- [7] GitHub Docs — Deployments and environments (required reviewers, wait timer, environment secrets на Free/Pro/Team): https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments ; доступность: https://github.com/github/docs/blob/main/data/reusables/gated-features/environments.md
- [8] Доступность code scanning, secret scanning, dependency review, push protection (исходник docs): https://github.com/github/docs/tree/main/data/reusables/gated-features (`code-scanning.md`, `secret-scanning.md`, `dependency-review.md`, `push-protection-for-users.md`)
- [9] Доступность Dependabot alerts, security updates, version updates: https://github.com/github/docs/blob/main/data/reusables/gated-features/dependabot-version-updates.md (а также `dependabot-alerts.md`, `dependabot-security-updates.md` в той же папке)
- [11] GitHub Pricing (Team — $4 USD за пользователя в месяц, на 04.10.2026): https://github.com/pricing
- [15] GitHub Docs — GitHub Packages billing (Container registry «currently free», квоты Free 500 МБ / 1 ГБ): https://docs.github.com/en/billing/concepts/product-billing/github-packages
- [16] GitHub Docs — Actions limits (20 параллельных джоб на Free, 6 ч / 5 суток / 24 ч очереди, Docker Hub rate limit на раннерах): https://docs.github.com/en/actions/reference/limits ; Actions runner pricing (округление до минуты): https://docs.github.com/en/billing/reference/actions-runner-pricing
- [17] GitHub Docs — Dependency caching (10 ГБ, вытеснение через 7 дней): https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- [18] GitHub Docs — Working with the Container registry (только classic PAT, `GITHUB_TOKEN`, анонимный pull публичных образов): https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry
- [19] GitHub Docs — GitHub's plans (protected branches в приватных репозиториях — с Pro/Team): https://docs.github.com/en/get-started/learning-about-github/githubs-plans
- [20] GitHub Docs — Secure use reference (секреты, dependency graph): https://docs.github.com/en/actions/reference/security/secure-use
- [22] The Register — «GitHub walks back plan to charge for self-hosted runners» (17.12.2025): https://www.theregister.com/software/2025/12/17/github-walks-back-plan-to-charge-for-self-hosted-runners/2064954
- [23] cicdpipelinecost.com — GitHub Actions Pricing 2026 (сторонний трекер; статус платы за self-hosted на 23.09.2026): https://cicdpipelinecost.com/github-actions-pricing

Безопасность Actions:
- [40] GitHub Docs — Secure use reference (hardening self-hosted runners, pinning по SHA, `pull_request_target`, Dependabot и SHA): https://docs.github.com/en/actions/reference/security/secure-use
- [41] GitHub Docs — Adding self-hosted runners («only use self-hosted runners with private repositories»): https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners
- [42] GitHub Docs — Managing GitHub Actions settings for a repository (approval for fork PR workflows): https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository
- [43] Aqua Security — GHSA-69fq-xp46-6x23 «Trivy ecosystem supply chain temporarily compromised»: https://github.com/aquasecurity/trivy/security/advisories/GHSA-69fq-xp46-6x23 ; NVD CVE-2026-33634: https://nvd.nist.gov/vuln/detail/CVE-2026-33634
- [44] Microsoft Security Blog — Trivy supply chain compromise (24.03.2026): https://www.microsoft.com/en-us/security/blog/2026/03/24/detecting-investigating-defending-against-trivy-supply-chain-compromise/
- [47] GitHub Docs — Troubleshooting required status checks (пропущенный workflow и Pending): https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks ; dorny/paths-filter: https://github.com/dorny/paths-filter

Риски РФ:
- [10] Visa — «Visa Suspends All Russia Operations» (05.03.2022): https://usa.visa.com/about-visa/newsroom/press-releases.releaseId.18871.html ; Reuters: https://www.reuters.com/business/finance/visa-suspends-operations-russia-over-ukraine-invasion-2022-03-05/
- [12] GitHub Docs — GitHub and Trade Controls: https://docs.github.com/en/site-policy/other-site-policies/github-and-trade-controls
- [13] Meduza — «GitHub access deteriorates in Russia» (08.05.2026, данные OONI через «Вёрстку»): https://meduza.io/en/news/2026/05/08/github-access-deteriorates-in-russia-as-internet-regulator-denies-blocking ; Meduza (08.06.2026): https://meduza.io/en/feature/2026/06/08/russia-s-internet-crackdown-is-cutting-developers-off-from-github-and-python-the-government-s-proposed-fix-a-vpn-controlled-by-the-government
- [14] Хабр — «GitHub и PyPI сбоят в России» (15.06.2026; автор сам помечает, что подтверждено, а что нет): https://habr.com/ru/articles/1047444/
- [21] Docker Docs — Docker Hub usage and limits: https://docs.docker.com/docker-hub/usage/
- [24] BleepingComputer — «GitHub suspends accounts of Russian devs at sanctioned companies» (2022): https://www.bleepingcomputer.com/news/security/github-suspends-accounts-of-russian-devs-at-sanctioned-companies/
- [25] GitVerse — зеркало Docker Hub и других реестров: https://gitverse.ru/docs/artifactory/registry-mirrors/dh-mirror/
- [26] GitLab Handbook — Trade Compliance: https://handbook.gitlab.com/handbook/legal/trade-compliance/
- [27] GitLab, `ee/lib/world.rb` (`COUNTRY_DENYLIST`): https://gitlab.com/gitlab-org/gitlab/-/blob/master/ee/lib/world.rb
- [28] GitLab Docs — Identity verification: https://docs.gitlab.com/security/identity_verification/

Альтернативы:
- [29] GitLab Docs — Compute minutes (400 мин на Free): https://docs.gitlab.com/ci/pipelines/compute_minutes/ ; GitLab Pricing (5 пользователей, 10 GiB): https://about.gitlab.com/pricing/ ; Storage: https://docs.gitlab.com/user/storage_usage_quotas/
- [30] Forgejo — Actions Security: https://forgejo.org/docs/latest/user/actions/security/ ; Docker access in Actions: https://forgejo.org/docs/latest/admin/actions/docker-access/
- [31] Forgejo — Container Registry: https://forgejo.org/docs/latest/user/packages/container/
- [32] Forgejo — Actions user guide («designed to be familiar… not designed to be compatible»): https://forgejo.org/docs/latest/user/actions/overview/
- [33] Woodpecker CI — Forges: https://woodpecker-ci.org/docs/administration/configuration/forges/overview
- [34] GitVerse — Лимиты CI/CD: https://gitverse.ru/docs/cicd/docs/limits
- [35] GitVerse — Локальные раннеры: https://gitverse.ru/docs/cicd/docs/runners/self-hosted/
- [36] SourceCraft — Правила тарификации (с 01.10.2026): https://sourcecraft.dev/portal/docs/ru/sourcecraft/pricing
- [37] SourceCraft — Квоты и лимиты: https://sourcecraft.dev/portal/docs/ru/sourcecraft/concepts/limits
- [38] SourceCraft — Воркеры CI/CD: https://sourcecraft.dev/portal/docs/ru/sourcecraft/concepts/workers.md
- [39] Yandex Cloud — блог об обновлениях SourceCraft (конвертация GitHub Actions): https://yandex.cloud/ru/blog/sourcecraft-neuro-scale-2025

Инструменты:
- [45] Docker Docs — `docker compose up` (`--wait`, `--wait-timeout`): https://docs.docker.com/reference/cli/docker/compose/up/
- [46] Renovate Docs: https://docs.renovatebot.com/
- CNCF Distribution (registry): https://distribution.github.io/distribution/ ; Zot: https://zotregistry.dev/
- Внутренние документы: [architecture.md](architecture.md) (§6, §7), [stack-rationale.md](stack-rationale.md)
