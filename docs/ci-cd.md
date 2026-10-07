# CI/CD для Scrobb: экономный режим на GitHub Free

> Статус: **черновик** (04.10.2026; стек обновлён 07.10.2026 по [ревью](review.md), §2). **Решение Ивана:** репозиторий приватный, план GitHub Free. CI запускается редко — только для крупных проверок — и укладывается в бесплатные 2 000 минут GitHub-hosted раннеров. Отдельного VPS под раннер пока нет. Стек и деплой — из [architecture.md](architecture.md) (§6, §7): Python 3.14 + FastAPI, очередь procrastinate в PostgreSQL за интерфейсом `jobs`, PostgreSQL 18, Valkey 9 (один экземпляр — кэш), Next.js, Telegram-бот (aiogram), Docker Compose на одном VPS в РФ (152-ФЗ). Пометка **(не проверено)** — факт не подтверждён первоисточником или на практике; **(оценка)** — расчёт, а не измерение. Источники — в конце, номера в тексте вида [N]. Цифры GitHub сверены с docs.github.com 04.10.2026.

## Коротко

- **Бюджет.** В приватном репозитории на Free — **2 000 минут в месяц** на GitHub-hosted раннерах, 500 МБ под артефакты и пакеты (без Container registry), 10 ГБ кэша на репозиторий. Каждая джоба округляется вверх до целой минуты. В начале месяца счётчик обнуляется [1][16].
- **Перерасхода не будет — будет остановка.** Без способа оплаты GitHub после исчерпания квоты просто блокирует запуски [1]. Карты Visa и Mastercard российских банков с марта 2022 за рубежом не работают [10]. Значит, 2 000 минут — жёсткий потолок, и за ним надо следить (§4).
- **Когда запускается CI:** PR в `main` (кроме черновиков и изменений только в документации), push тега `v*` и ручной `workflow_dispatch`. **На каждый push CI не запускается**, на push в `main` — тоже.
- **Дешёвые проверки — локально.** Хуки pre-commit (ruff, mypy, eslint, gitleaks) и `make check` до PR. CI подтверждает, а не ищет опечатки.
- **Бюджет (оценка):** полный прогон 5–10 мин, то есть **200–400 полных прогонов в месяц**.
- **Если минуты кончатся** — переключаемся одной переменной на self-hosted раннер на домашней машине (для приватного репозитория бесплатно и без лимита минут [1]) или гоняем CI в GitVerse на зеркале (500 мин/мес для приватных [26]). Позже можно взять отдельный VPS: **Selectel VDS 4-8-80 за 1 100 ₽/мес** [28] (§8).
- **Деплой:** тег `v*` → сборка или перетегирование образа → staging. Prod — вручную через `workflow_dispatch`. На VPS: бэкап, миграции, `compose up --wait`, health check, автоматический откат (§6).

---

## 1. Что даёт и чего не даёт GitHub Free в приватном репозитории

Аккаунт `fighxy` — личный, применяется **GitHub Free for personal accounts**.

| Возможность | Приватный репозиторий на Free | Источник |
|---|---|---|
| Минуты GitHub-hosted раннеров (standard) | **2 000 мин/мес**, каждая джоба округляется вверх до минуты, сброс в начале месяца | [1][16] |
| Цена минуты сверх квоты | Linux 2-core $0.006, Linux 1-core $0.002 (с 01.01.2026), но без способа оплаты запуски **блокируются** | [1][16] |
| Self-hosted раннеры | **Бесплатно, без лимита минут** (план Б, §8) | [1][2] |
| Артефакты Actions | **500 МБ**, общие с GitHub Packages (кроме Container registry) | [1] |
| Кэш Actions | 10 ГБ на репозиторий; записи, к которым **не обращались 7 дней**, удаляются | [1][17] |
| GHCR (Container registry) | Хранение и трафик «сейчас бесплатны»; об изменениях обещают предупредить минимум за месяц | [15][18] |
| Параллельные джобы | 20 | [16] |
| Длительность джобы | До 6 ч на hosted (поэтому всегда задаём `timeout-minutes`) | [16] |
| Protected branches, rulesets | ❌ (нужен Pro или Team) | [5][6] |
| Environments, environment secrets, deployment branches | ❌ (нужен Pro, Team или Enterprise) | [7] |
| Required reviewers и wait timer для environments | ❌ — даже на Pro и Team только для публичных репозиториев | [7] |
| Repository secrets | ✅ | [29] |
| Dependabot alerts, security updates, version updates | ✅ | [9] |
| Code scanning (CodeQL), secret scanning, push protection, dependency review | ❌ | [8] |
| Docker Hub rate limit | На GitHub-hosted раннерах для публичных образов не действует; на self-hosted действует: 100 pull за 6 ч анонимно, 200 для авторизованного Personal | [16][21] |

**Self-hosted раннеры и плата за них.** 16.12.2025 GitHub анонсировал с 01.03.2026 плату $0.002/мин за self-hosted раннеры в приватных репозиториях, а 17.12.2025 отложил её «для пересмотра» [3][4][22]. На октябрь 2026 года плата не введена: документация называет self-hosted бесплатными [1]. Сторонний трекер на 23.09.2026 подтверждает отсутствие сбора [23]. От идеи GitHub не отказался — учитываем как риск для плана Б.

**Риски из РФ** (подробности — в источниках):
- **Санкции.** Если аккаунт ограничат, приватные репозитории станут недоступны и скачать их будет нельзя [12]. В 2022 году GitHub блокировал аккаунты разработчиков, связанных с подсанкционными компаниями [24]. Поэтому нужно **зеркало** (§7.4).
- **Связность.** С мая 2026 года доступ к GitHub из РФ деградирует (OONI: 10–16% неудачных соединений), тяжёлые `clone`/`push` обрываются чаще [13][14]. Это касается и pull образов из GHCR на VPS: нужны ретраи и запасной вариант (§6.2).
- **Оплата.** Pro или Team (Team — от $4 за пользователя в месяц [11]) без зарубежной карты не купить [10].

## 2. Когда запускается CI

| Workflow | Триггер | Что делает | Минуты (оценка) |
|---|---|---|---|
| `ci.yml` | `pull_request` в `main` (типы `opened`, `synchronize`, `reopened`, `ready_for_review`), кроме draft; `workflow_dispatch`; `workflow_call` из release | Одна джоба `check`: линт, типы, тесты, миграции, сборка web | 5–10 |
| `release.yml` | push тега `v*` | `check` → образы (сборка или перетегирование) → trivy → push в GHCR → деплой в staging → smoke | 8–15 |
| `deploy-prod.yml` | только `workflow_dispatch` (вход — тег) | Проверка, что образ есть в GHCR → SSH на prod → `deploy.sh` | 1–2 |
| `mirror.yml` | `schedule` раз в неделю + `workflow_dispatch` | `git push --mirror` в GitVerse | ~1 |

Чего **нет**: CI на push в рабочие ветки, на push в `main` и ежедневных расписаний.

Почему без CI на `main` не страшно:
- Событие `pull_request` проверяет **merge-коммит** PR с текущим `main`, то есть почти то, что окажется в `main`.
- Если два PR смёржены друг за другом и вместе ломают `main`, это поймает `release.yml`: перед сборкой он заново запускает `check` на коммите тега.
- Прямой push в `main` на Free не запретить [5]. Договорённость — только через PR (§7.1).

Ручной запуск: `gh workflow run ci.yml --ref <ветка>` — например, проверить ветку до открытия PR.

## 3. Как экономим минуты

1. **Локальные проверки вместо CI (главная экономия).** Хуки [pre-commit](https://pre-commit.com/) [35]:

   ```yaml
   # .pre-commit-config.yaml (набросок; rev — актуальные версии при внедрении)
   repos:
     - repo: https://github.com/astral-sh/ruff-pre-commit
       rev: <версия>
       hooks:
         - id: ruff-check
           args: [--fix]
         - id: ruff-format
     - repo: https://github.com/gitleaks/gitleaks
       rev: <версия>
       hooks:
         - id: gitleaks
     - repo: local
       hooks:
         - id: mypy
           name: mypy
           entry: uv run mypy backend
           language: system
           types: [python]
           pass_filenames: false
           stages: [pre-push]          # mypy медленнее — на pre-push
         - id: eslint
           name: eslint
           entry: pnpm -C web exec eslint --max-warnings=0
           language: system
           files: ^web/.*\.(ts|tsx|js|jsx)$
   ```

   Установка: `pre-commit install && pre-commit install --hook-type pre-push`. Дополнительно — `make check`, который повторяет CI локально: поднимает `postgres:18` и Valkey через `docker compose -f compose.test.yaml`, затем запускает `pytest`, `alembic upgrade head && alembic check` и `pnpm build`. Правило: PR открываем (или переводим из draft) **после** зелёного `make check`. Хук-идентификаторы `ruff-check` и `gitleaks` сверить с README репозиториев при внедрении (**не проверено**).
2. **Draft PR не тратят минуты:** у джобы условие `if: github.event.pull_request.draft == false` (для `pull_request`). Работаем в draft, переводим в «Ready for review», когда готово. Push в готовый PR — новый прогон, поэтому коммиты в готовый PR лучше пушить пачкой.
3. **Только документация — CI не запускается:** `paths-ignore: ['docs/**', '**/*.md', 'LICENSE', '.github/ISSUE_TEMPLATE/**']`. На приватном Free обязательных проверок нет, поэтому пропущенный workflow ничего не блокирует. С появлением required checks (Pro или публичный репозиторий) пропущенный workflow оставит проверку в Pending [19] — тогда перейдём на `ci-gate`.
4. **Внутри джобы — фильтр по частям:** шаг `dorny/paths-filter` определяет, менялись ли `backend/**` или `web/**`, и шаги другой части пропускаются (`if: steps.f.outputs.web == 'true'`).
5. **`[skip ci]`** в сообщении HEAD-коммита PR полностью пропускает `pull_request`-прогон [19] (например, если в коммите поменялись только комментарии в коде).
6. **Отмена устаревших прогонов:** `concurrency: { group: ci-${{ github.event.pull_request.number || github.ref }}, cancel-in-progress: true }`. Новый push в PR отменяет предыдущий прогон; уже потраченные минуты отменённого прогона не возвращаются (**не проверено**, исходим из посекундного учёта с округлением джобы [16]). Для `deploy-prod` — `cancel-in-progress: false`: деплой не обрывается.
7. **Одна джоба вместо пяти.** Каждая джоба округляется вверх до минуты и тратит 20–40 с на старт (**оценка**). Поэтому backend и web проверяются в одной джобе `check` последовательно. Параллельность на 2–3 джобы оправдана, только если прогон заметно дольше 10 минут.
8. **Кэш зависимостей:** `astral-sh/setup-uv` с `enable-cache: true`, `actions/setup-node` с `cache: pnpm`. Ограничение: записи кэша, к которым не обращались **7 дней**, удаляются [17]. При редких запусках кэш может остывать — это ожидаемо.
9. **Кэш слоёв Docker — в реестре, а не в Actions cache:** `cache-from/cache-to: type=registry,ref=ghcr.io/fighxy/scrobb-backend:buildcache,mode=max`. Он живёт в GHCR (хранение сейчас бесплатно [15]) и не вытесняется через 7 дней, как `type=gha` [17].
10. **Не пересобирать один и тот же коммит.** Образы тегируются и `sha-<коммит>`, и `vX.Y.Z`. Если для коммита тега образ уже есть (например, после `v1.2.0-rc.1`), `release.yml` выполняет `docker buildx imagetools create` (перетегирование за секунды) вместо сборки.
11. **`timeout-minutes` везде** (`check` — 15, `release` — 25, `deploy-prod` — 10): зависший тест не съест до 6 ч квоты [16].
12. **Артефакты не храним** (или `retention-days: 1`): лимит 500 МБ общий с Packages [1]. Образы как артефакты не загружаем.
13. **Dependabot** — раз в месяц, обновления сгруппированы (`groups:`), `open-pull-requests-limit: 3`. CI на PR Dependabot тратит минуты как обычный PR. Бесплатны только собственные задачи Dependabot по обновлению [1], а прогоны наших workflow на его PR — **не проверено**, исходим из того, что считаются.

## 4. Бюджет минут и как за ним следить

**Оценка** (не измерение, уточним через 2–4 недели):

| Что | Минут за раз | Раз в месяц (предположение) | Итого |
|---|---|---|---|
| `ci.yml` на PR | 5–10 | 40 | 200–400 |
| `release.yml` | 8–15 | 6 | 48–90 |
| `deploy-prod.yml` | 1–2 | 6 | 6–12 |
| `mirror.yml` | ~1 | 4–5 | 4–5 |
| **Итого** | | | **≈ 260–510 из 2 000** |

Предел: 2 000 мин ÷ 5–10 мин = **200–400 полных прогонов в месяц** (оценка).

**Как следить:**
- **Веб-интерфейс:** https://github.com/settings/billing → **Usage** (или Metered usage), фильтр по продукту Actions, группировка по репозиторию [20]. Смотрим раз в неделю, например в понедельник.
- **Письма:** GitHub сам присылает уведомления при 90% и 100% включённой квоты [1].
- **API (для скрипта или бота в Telegram):** `GET /users/{username}/settings/billing/usage/summary?product=actions` и `GET /users/{username}/settings/billing/usage?year=&month=` [20]. Токену нужен scope `user`: `gh auth refresh -h github.com -s user`, затем `gh api "/users/fighxy/settings/billing/usage/summary?product=actions"`. Проверено только, что без scope `user` API отвечает 404 с подсказкой про scope; формат ответа на нашем аккаунте **не проверен**.
- **Длительность прогонов:** `gh run list --limit 30 --json workflowName,conclusion,createdAt,updatedAt` — видно, какой workflow дорожает.
- **Правило:** на 75% (1 500 минут) до конца месяца — `ci.yml` только вручную через `workflow_dispatch` или переключение на план Б (§8). 1 числа счётчик обнуляется [1].

## 5. Набросок workflow

Версии actions — только полный SHA с комментарием-версией [29]. Пакетный менеджер (`uv`), pnpm и структура каталогов (`backend/`, `web/`, `migrations/`) — допущения наброска, их выбираем на этапе кода.

```yaml
# .github/workflows/ci.yml
name: ci
on:
  pull_request:
    branches: [main]
    types: [opened, synchronize, reopened, ready_for_review]
    paths-ignore: ['docs/**', '**/*.md', 'LICENSE']
  workflow_dispatch:
  workflow_call:
permissions:
  contents: read
concurrency:
  group: ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  check:
    if: github.event_name != 'pull_request' || github.event.pull_request.draft == false
    runs-on: ${{ fromJSON(vars.RUNNER || '"ubuntu-latest"') }}   # план Б: §8
    timeout-minutes: 15
    services:
      postgres:
        image: postgres:18          # pg_partman не нужен: секции создаёт своя задача (architecture.md §2.3)
        env: { POSTGRES_PASSWORD: test, POSTGRES_DB: scrobb_test }
        ports: ['5432:5432']
        options: >-
          --health-cmd "pg_isready -U postgres" --health-interval 5s --health-retries 20
      valkey:
        image: valkey/valkey:9          # та же версия, что в prod compose
        ports: ['6379:6379']
    env:
      DATABASE_URL: postgresql+psycopg://postgres:test@localhost:5432/scrobb_test   # psycopg 3 — общий драйвер с procrastinate
      VALKEY_URL: redis://localhost:6379/0
    steps:
      - uses: actions/checkout@<SHA>
      - id: f
        uses: dorny/paths-filter@<SHA>
        with:
          filters: |
            backend: ['backend/**', 'migrations/**', 'pyproject.toml', 'uv.lock', 'docker/**']
            web: ['web/**']
      # на release (workflow_call) проверяем всё, без фильтра
      - if: steps.f.outputs.backend == 'true' || github.event_name != 'pull_request'
        uses: astral-sh/setup-uv@<SHA>
        with: { enable-cache: true }
      - if: steps.f.outputs.backend == 'true' || github.event_name != 'pull_request'
        run: |
          uv sync --frozen
          uv run ruff check . && uv run ruff format --check .
          uv run mypy backend
          uv run alembic upgrade head && uv run alembic check
          uv run pytest -q
      - if: steps.f.outputs.web == 'true' || github.event_name != 'pull_request'
        uses: pnpm/action-setup@<SHA>
      - if: steps.f.outputs.web == 'true' || github.event_name != 'pull_request'
        uses: actions/setup-node@<SHA>
        with: { node-version-file: web/.nvmrc, cache: pnpm, cache-dependency-path: web/pnpm-lock.yaml }
      - if: steps.f.outputs.web == 'true' || github.event_name != 'pull_request'
        working-directory: web
        run: pnpm install --frozen-lockfile && pnpm lint && pnpm tsc --noEmit && pnpm build
```

`release.yml` (на `push: tags: ['v*']`):
1. Джоба `check`: `uses: ./.github/workflows/ci.yml`.
2. Джоба `release` (`needs: check`, `permissions: { contents: read, packages: write }`, `timeout-minutes: 25`):
   - `docker/login-action` с `GITHUB_TOKEN`;
   - проверка `docker manifest inspect ghcr.io/fighxy/scrobb-backend:sha-${GITHUB_SHA::7}`;
   - если образа нет — `docker/build-push-action` с `cache-from/to: type=registry,…:buildcache`, затем `trivy image --severity CRITICAL,HIGH --exit-code 1 --ignore-unfixed`;
   - push тегов `sha-…` и `vX.Y.Z`; если образ был — только `docker buildx imagetools create -t …:vX.Y.Z …:sha-…`;
   - `ssh deploy@vps staging vX.Y.Z` и smoke-тест.

`deploy-prod.yml`: `workflow_dispatch` с `inputs.tag`. Шаги:
- проверить формат `^v[0-9]+\.[0-9]+\.[0-9]+$`;
- проверить, что тег указывает на коммит из `main` (`git merge-base --is-ancestor`);
- проверить, что образ есть в GHCR;
- выполнить `ssh deploy@vps prod <tag>`.

## 6. Деплой на VPS

### 6.1 Схема пайплайна

Диаграмма проверена рендером через mermaid-cli (`mmdc`).

```mermaid
flowchart TD
  LOCAL["Локально: pre-commit (ruff, mypy, eslint, gitleaks) и make check"] --> PRQ{"PR в main готов? (не draft)"}
  PRQ -->|"нет: draft или только docs/**"| SKIP["CI не запускается: 0 минут"]
  PRQ -->|"да"| CHECK

  subgraph CI["ci.yml: pull_request в main, workflow_dispatch (cancel-in-progress)"]
    direction TB
    CHECK["одна джоба check: paths-filter, Postgres 18 и Valkey, кэш uv и pnpm"]
    CHECK --> BE["backend: ruff, mypy, pytest, alembic upgrade и check"]
    CHECK --> FE["web: eslint, tsc, next build"]
  end

  BE --> MERGE["squash-merge в main (на push CI не запускается)"]
  FE --> MERGE
  MERGE -->|"git tag vX.Y.Z"| REL

  subgraph RELEASE["release.yml: push тега v*"]
    direction TB
    REL["check повторно (ci.yml через workflow_call)"]
    REL --> IMG{"образ sha-коммита уже есть в GHCR?"}
    IMG -->|"да"| RETAG["imagetools: перетегировать, без сборки"]
    IMG -->|"нет"| BUILD["buildx: кэш слоёв в GHCR (type=registry)"]
    BUILD --> SCAN["trivy image: CRITICAL/HIGH → fail"]
    SCAN --> PUSH["push vX.Y.Z и sha-… в GHCR"]
    RETAG --> STG
    PUSH --> STG["deploy staging по SSH (forced command)"]
    STG --> SMOKE["smoke: /healthz, /readyz, тестовый скроббл"]
  end

  SMOKE --> DISPATCH["deploy-prod.yml: ручной workflow_dispatch с тегом"]

  subgraph PROD["deploy.sh на prod VPS"]
    direction TB
    BK["бэкап перед миграцией"] --> PULL["docker compose pull"]
    PULL --> MIG["alembic upgrade head (одноразовый контейнер)"]
    MIG --> UP["docker compose up -d --wait"]
    UP --> HC{"health check OK?"}
    HC -->|"да"| DONE["записать тег как текущий"]
    HC -->|"нет"| RB["откат на предыдущий тег, алерт"]
  end

  DISPATCH --> BK
  BUDGET["минуты кончаются (>75% от 2000)"] -.-> FALLBACK["vars.RUNNER → self-hosted дома или CI GitVerse на зеркале"]
```

### 6.2 Образы и реестр

- Два образа: `scrobb-backend` (один образ, роль задаётся `ROLE=api|worker|bot|migrate`, architecture.md §7.6) и `scrobb-web`. Теги: неизменяемый `sha-<7 символов>` и релизный `vX.Y.Z`. `latest` в prod не используем.
- **GHCR — основной реестр:** push из Actions через `GITHUB_TOKEN`. Для pull на VPS нужен **classic PAT** с `read:packages` — fine-grained токены GitHub Packages не поддерживает [18]. Classic PAT видит все пакеты аккаунта, поэтому лучше отдельный machine-user.
- **Если pull из GHCR на VPS в РФ сбоит** [13][14]: ретраи в `deploy.sh`; компактные образы (multi-stage, slim-база); запасной вариант без реестра — `docker save | ssh vps docker load` или свой реестр (CNCF Distribution / Zot) на VPS.
- **Откат не зависит от реестра:** на VPS храним локально 3–5 предыдущих тегов, чистим только более старые.
- Docker Hub с VPS: `docker login` и/или `registry-mirrors` (например, зеркало GitVerse `dh-mirror.gitverse.ru` [25]) — из-за лимитов pull [21].
- **Том PostgreSQL 18 в compose** монтируем в `/var/lib/postgresql`, а не в `/var/lib/postgresql/data`, как до 18-й версии: в образе `postgres:18` `PGDATA` = `/var/lib/postgresql/18/docker`, а `VOLUME` перенесён на уровень выше [38].

### 6.3 SSH с forced command

- CI подключается как `deploy@vps` и может выполнить **только** `/opt/scrobb/bin/deploy.sh`. В `authorized_keys`: `command="/opt/scrobb/bin/deploy.sh",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty`. Аргументы скрипт берёт из `SSH_ORIGINAL_COMMAND` и проверяет по белому списку.
- Разные ключи для staging и prod (`STAGING_SSH_KEY`, `PROD_SSH_KEY`) плюс `SSH_KNOWN_HOSTS` в repository secrets. `StrictHostKeyChecking` не отключаем.
- `deploy` в группе `docker` — это фактически root, отсюда forced command. Вход только по ключу, `fail2ban`. Диапазоны IP GitHub-hosted раннеров огромные, поэтому порт SSH открыт для мира.

### 6.4 `deploy.sh`: health check и откат

```bash
#!/usr/bin/env bash
set -euo pipefail
read -r ENV TAG <<<"${SSH_ORIGINAL_COMMAND:-}"
[[ "$ENV" =~ ^(staging|prod)$ ]] || { echo "bad env"; exit 2; }
if [[ "$ENV" == prod ]]; then [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 2
else [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] || exit 2; fi

cd "/opt/scrobb/$ENV"
exec 9>.deploy.lock; flock -n 9 || { echo "deploy in progress"; exit 3; }
PREV=$(cat .current_tag 2>/dev/null || echo "")
export IMAGE_TAG="$TAG"                      # compose.yaml: image: ghcr.io/…/scrobb-backend:${IMAGE_TAG}

for i in 1 2 3; do docker compose pull && break || sleep 15; done   # ретраи на случай сбоев связности
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

- `docker compose up --wait` ждёт, пока сервисы станут `running` или `healthy` [33]. У каждого сервиса есть `healthcheck`: `api` — `GET /healthz`, `web` — свой health-роут, `worker`/`bot` — heartbeat-ключ в Valkey или файл с меткой времени (отдельного `scheduler` нет: периодические задачи ставят воркеры, architecture.md §6.3).
- `smoke.sh` проверяет снаружи через Caddy: `/readyz` (БД и Valkey отвечают, ревизия Alembic = head), главную страницу, тестовый скроббл (только на staging).
- Простой при `up -d` на одном VPS — секунды (**не измерено**). Blue/green за Caddy — позже, если понадобится.
- Откат образа не откатывает БД — поэтому миграции по схеме expand/contract (§6.6).

### 6.5 Секреты

На приватном Free нет environment secrets [7], а любой участник с правом записи может прочитать все repository secrets через workflow [29]. Поэтому:
- **В GitHub — только секреты доставки:** `STAGING_SSH_KEY`, `PROD_SSH_KEY`, `SSH_KNOWN_HOSTS`, токен зеркала (§7.4). Для GHCR — `GITHUB_TOKEN` с `packages: write` только в джобе `release`.
- **Секреты приложения живут на VPS** (`DATABASE_URL`, ЮKassa, токен Telegram-бота, S3, Last.fm API key, `SECRET_KEY`):
  - вариант 1: `/opt/scrobb/<env>/.env`, права `0600`;
  - вариант 2 (лучше): **sops + age** — зашифрованный файл в репозитории, приватный age-ключ **только на VPS**, `deploy.sh` расшифровывает перед `up`.
- `permissions: contents: read` по умолчанию. Не печатать секреты в лог и не складывать JSON или YAML в один секрет: это ломает маскирование [29].
- В CI и на staging — только синтетические данные, дампы prod туда не попадают (152-ФЗ).

### 6.6 Миграции Alembic

1. Отдельный шаг деплоя, одноразовый контейнер `migrate` из **нового** образа до `up` (architecture.md §7.6).
2. **Expand → migrate → contract:** миграция совместима с предыдущей версией кода, удаление старого — в следующем релизе. Тогда откат образа безопасен без `downgrade`.
3. `downgrade` в prod автоматически не запускаем. Восстановление — из бэкапа перед миграцией или PITR.
4. В `env.py`: `SET lock_timeout = '5s'` и `statement_timeout`. Индексы на больших таблицах — `CREATE INDEX CONCURRENTLY` в `autocommit_block()`. Секции `listens` создаёт периодическая задача `partitions.ensure` (architecture.md §2.3), а не Alembic; начальный набор секций — в миграции. Таблицы очереди — SQL-миграции procrastinate, которые применяются тем же шагом `migrate`.
5. Бэкфилл данных — фоновая задача в очереди, а не миграция.
6. В CI (`check`): `alembic upgrade head` на пустой БД и `alembic check`. Локально в `make check` — то же.

### 6.7 Окружения staging и prod

- **staging:** на том же VPS отдельный compose-проект `scrobb-staging` со своими Postgres и Valkey, `mem_limit`/`cpus`, поддомен за basic auth, только синтетические данные. Деплоится на каждый тег `v*`, в том числе `vX.Y.Z-rc.N`. Если ресурсов VPS мало — поднимаем только на время проверки релиза.
- **prod:** только финальный тег `vX.Y.Z`, только вручную (`deploy-prod.yml`), с бэкапом перед миграцией.
- Штатных environments на приватном Free нет [7] — разделение через отдельные workflow, разные SSH-ключи и проверки в `deploy.sh`.

## 7. Процесс и безопасность

### 7.1 Ветки и релизы

- **Trunk-based:** `main` всегда готов к релизу, короткие ветки `feat/*`, `fix/*`, merge через PR в режиме squash. Релизы — git-теги: `vX.Y.Z-rc.N` → только staging, `vX.Y.Z` на том же коммите → staging (без пересборки, §3 п. 10) → prod вручную.
- Branch protection на приватном Free нет [5][6]. Компенсация: договорённость «в `main` только через PR с зелёным `check`», повторный `check` в `release.yml`, проверки тега в `deploy-prod.yml` и белый список в `deploy.sh`. Это мягкие ворота: участник с правом записи может изменить workflow. Жёсткая граница — forced command и проверки на VPS.

### 7.2 Безопасность Actions

- **Пиним actions по полному SHA.** 19–20.03.2026 злоумышленники перезаписали 76 из 77 тегов `aquasecurity/trivy-action` на вредонос, крадущий секреты [31][32]. Только SHA делает ссылку неизменяемой [29]. Trivy ставим бинарником с проверкой checksum или через закреплённый по SHA action.
- `permissions:` минимальные в каждом workflow. Без `pull_request_target` с checkout кода из PR [29].
- Сканирование без CodeQL (на приватном Free его нет [8]): правила `S` в ruff, `gitleaks` в pre-commit, `osv-scanner`/`pip-audit` локально и в `release.yml`, `trivy image` при сборке. Полный `trivy` текущего prod-образа — вручную раз в месяц или в `release.yml`; отдельного расписания нет — экономим минуты.

### 7.3 Self-hosted раннер (для плана Б)

GitHub предупреждает: self-hosted раннер не гарантирует чистую эфемерную среду, может быть скомпрометирован навсегда и «should almost never be used for public repositories» [29][30]. Репозиторий приватный, поэтому раннер допустим, но:
- **не на prod VPS:** `pip install`/`pnpm install` выполняют код зависимостей, Docker socket — это root на хосте с персональными данными;
- отдельный пользователь без sudo; режим `--ephemeral` или JIT (одна джоба — и раннер снимается с регистрации [29]) в контейнере или VM;
- на машине раннера нет SSH-ключей от prod и личных данных; деплой-ключи остаются в GitHub Secrets и ограничены forced command;
- нужен `docker login` или зеркало Docker Hub: на self-hosted лимиты pull действуют [16][21];
- если репозиторий когда-нибудь станет публичным — self-hosted раннер сразу отключаем.

### 7.4 Зеркало

- `mirror.yml` раз в неделю и вручную: `git push --mirror` в приватный репозиторий GitVerse. Токен — repository secret с правом записи только в зеркало. После важного релиза — ручной запуск.
- Зачем: при санкционном ограничении приватный репозиторий на GitHub недоступен и не скачивается [12]. Зеркало же — готовая площадка для плана Б (§8).

### 7.5 Dependabot

Бесплатен и в приватном репозитории [9]: экосистемы `pip` (или `uv`), `npm`, `docker`, `github-actions`. Расписание `monthly`, обновления сгруппированы, `open-pull-requests-limit: 3`. Security updates приходят вне расписания. Поддержку `uv` и `docker-compose` проверить при настройке (**не проверено**). Renovate — если понадобится гибче [34].

## 8. План Б: если минуты кончаются

| Вариант | Стоимость | Что даёт | Минусы |
|---|---|---|---|
| **1. Self-hosted раннер на домашней машине** | ₽0 (электричество) | Минуты GitHub не тратятся: self-hosted бесплатны и без лимита [1]. Переключение без правки workflow: переменная репозитория `RUNNER` = `["self-hosted","linux","scrobb"]` (вернуть — удалить переменную). Белый IP не нужен: раннер только исходящий | Машина должна быть включена; связность GitHub из РФ деградирует [13]; меры безопасности §7.3; риск будущей платы за self-hosted [3][22]. Работа `vars` в `runs-on` — **проверить при внедрении** |
| **2. CI GitVerse на зеркале** | ₽0 | Облачные раннеры: **500 мин/мес** для приватных репозиториев пользователя (1 000 у верифицированных организаций), облачная джоба ≤ 30 мин, артефакты 500 МБ на все репозитории, хранятся 30 дней [26]. Там же свои раннеры (act_runner) без лимитов [27]. Площадка в РФ | Отдельные workflow: синтаксис в стиле GitHub Actions, совместимость с нашими файлами **не проверена**. Статусы не видны в PR на GitHub. В 09.2026 GitVerse отключил старый endpoint раннеров `gitverse.ru/sc` — платформа активно меняется [27] |
| **3. Отдельный VPS под раннер (позже)** | **Selectel VDS 4-8-80 — 1 100 ₽/мес**: 4 vCPU, 8 ГБ RAM, 80 ГБ, KVM, Xeon Gold 6240R 2,4 ГГц, 3 ТБ исходящего трафика в месяц, только IPv4. Цена проверена 04.10.2026 [28] | Стабильный раннер 24/7 без домашней машины | По FAQ Selectel эта линейка VDS не соответствует 152-ФЗ — для CI без персональных данных допустимо. Почасовая оплата для VDS **не проверена**. Только фиксированные конфигурации, уменьшить нельзя [28] |
| Другие | — | SourceCraft (Яндекс): 1 000 мин/мес CI и для приватных репозиториев, но свой формат YAML [36]. Forgejo + Forgejo Actions на своём сервере — без лимитов, но «not designed to be compatible» с GitHub Actions и требует обслуживания [37] | Переписывать workflow |

**Порядок действий:** при 75% квоты включаем вариант 1 до конца месяца. Если домашняя машина недоступна — вручную запускаем `ci.yml` только перед релизом. Если такое повторяется каждый месяц — берём вариант 3.

## 9. План внедрения

1. `.pre-commit-config.yaml` и `make check` — до первого кода в CI.
2. `ci.yml` (одна джоба, draft-skip, paths-ignore, concurrency, кэши), `dependabot.yml` (monthly, groups).
3. `compose.yaml` с healthcheck, `deploy.sh` с forced command, staging.
4. `release.yml` (GHCR, кэш слоёв в реестре, перетегирование, trivy), `deploy-prod.yml`.
5. `mirror.yml` → GitVerse.
6. Через 2–4 недели — фактический расход минут (§4) и решение, нужен ли план Б.

## Источники

GitHub — тарифы и лимиты:
- [1] GitHub Docs — GitHub Actions billing (self-hosted бесплатны, квоты Free, цены, блокировка без способа оплаты, уведомления на 90% и 100%, сброс минут в начале месяца): https://docs.github.com/en/billing/concepts/product-billing/github-actions
- [2] GitHub Docs — GitHub's plans: https://docs.github.com/en/get-started/learning-about-github/githubs-plans
- [3] GitHub Changelog — «Update to GitHub Actions pricing» (16.12.2025, обновлено: перенос платы за self-hosted): https://github.blog/changelog/2025-12-16-coming-soon-simpler-pricing-and-a-better-experience-for-github-actions/
- [4] GitHub — «Pricing changes for GitHub Actions»: https://github.com/resources/insights/2026-pricing-changes-for-github-actions ; roadmap issue #1197: https://github.com/github/roadmap/issues/1197
- [5] GitHub Docs — About protected branches: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches ; доступность: https://github.com/github/docs/blob/main/data/reusables/gated-features/protected-branches.md
- [6] GitHub Docs — About rulesets: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets ; доступность: https://github.com/github/docs/blob/main/data/reusables/gated-features/repo-rules.md
- [7] GitHub Docs — Deployments and environments: https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments ; доступность: https://github.com/github/docs/blob/main/data/reusables/gated-features/environments.md
- [8] Доступность code scanning, secret scanning, dependency review, push protection: https://github.com/github/docs/tree/main/data/reusables/gated-features
- [9] Доступность Dependabot: https://github.com/github/docs/blob/main/data/reusables/gated-features/dependabot-version-updates.md (а также `dependabot-alerts.md`, `dependabot-security-updates.md`)
- [11] GitHub Pricing (Team — $4 за пользователя в месяц, на 04.10.2026): https://github.com/pricing
- [15] GitHub Docs — GitHub Packages billing (Container registry «currently free»): https://docs.github.com/en/billing/concepts/product-billing/github-packages
- [16] GitHub Docs — Actions limits: https://docs.github.com/en/actions/reference/limits ; Actions runner pricing (округление до минуты): https://docs.github.com/en/billing/reference/actions-runner-pricing
- [17] GitHub Docs — Dependency caching (10 ГБ, вытеснение через 7 дней): https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- [18] GitHub Docs — Working with the Container registry (только classic PAT, `GITHUB_TOKEN`): https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry
- [19] GitHub Docs — Skipping workflow runs (`[skip ci]`, Pending при пропуске по путям): https://docs.github.com/en/actions/how-tos/manage-workflow-runs/skip-workflow-runs
- [20] GitHub Docs — Viewing your usage of metered products: https://docs.github.com/en/billing/how-tos/products/view-productlicense-use ; REST API Billing usage: https://docs.github.com/en/rest/billing/usage
- [22] The Register — «GitHub walks back plan to charge for self-hosted runners» (17.12.2025): https://www.theregister.com/software/2025/12/17/github-walks-back-plan-to-charge-for-self-hosted-runners/2064954
- [23] cicdpipelinecost.com — GitHub Actions Pricing 2026 (сторонний трекер, статус на 23.09.2026): https://cicdpipelinecost.com/github-actions-pricing

Безопасность:
- [29] GitHub Docs — Secure use reference: https://docs.github.com/en/actions/reference/security/secure-use
- [30] GitHub Docs — Adding self-hosted runners: https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners
- [31] Aqua Security — GHSA-69fq-xp46-6x23: https://github.com/aquasecurity/trivy/security/advisories/GHSA-69fq-xp46-6x23 ; NVD CVE-2026-33634: https://nvd.nist.gov/vuln/detail/CVE-2026-33634
- [32] Microsoft Security Blog — Trivy supply chain compromise (24.03.2026): https://www.microsoft.com/en-us/security/blog/2026/03/24/detecting-investigating-defending-against-trivy-supply-chain-compromise/

Риски РФ:
- [10] Visa — «Visa Suspends All Russia Operations» (05.03.2022): https://usa.visa.com/about-visa/newsroom/press-releases.releaseId.18871.html
- [12] GitHub Docs — GitHub and Trade Controls: https://docs.github.com/en/site-policy/other-site-policies/github-and-trade-controls
- [13] Meduza — «GitHub access deteriorates in Russia» (08.05.2026, данные OONI): https://meduza.io/en/news/2026/05/08/github-access-deteriorates-in-russia-as-internet-regulator-denies-blocking
- [14] Хабр — «GitHub и PyPI сбоят в России» (15.06.2026): https://habr.com/ru/articles/1047444/
- [21] Docker Docs — Docker Hub usage and limits: https://docs.docker.com/docker-hub/usage/
- [24] BleepingComputer — GitHub suspends accounts of Russian devs at sanctioned companies (2022): https://www.bleepingcomputer.com/news/security/github-suspends-accounts-of-russian-devs-at-sanctioned-companies/

План Б и инструменты:
- [25] GitVerse — зеркало Docker Hub: https://gitverse.ru/docs/artifactory/registry-mirrors/dh-mirror/
- [26] GitVerse — Лимиты CI/CD: https://gitverse.ru/docs/cicd/docs/limits
- [27] GitVerse — Локальные раннеры: https://gitverse.ru/docs/cicd/docs/runners/self-hosted/
- [28] Selectel — Аренда VPS/VDS (VDS 4-8-80 — 1 100 ₽/мес; FAQ: 152-ФЗ, IPv4, трафик; проверено 04.10.2026): https://selectel.ru/services/cloud/vps-vds/
- [33] Docker Docs — `docker compose up` (`--wait`, `--wait-timeout`): https://docs.docker.com/reference/cli/docker/compose/up/
- [34] Renovate Docs: https://docs.renovatebot.com/
- [35] pre-commit: https://pre-commit.com/ ; ruff-pre-commit: https://github.com/astral-sh/ruff-pre-commit
- [36] SourceCraft — Правила тарификации (с 01.10.2026): https://sourcecraft.dev/portal/docs/ru/sourcecraft/pricing
- [37] Forgejo — Actions user guide: https://forgejo.org/docs/latest/user/actions/overview/
- [38] Docker Hub — образ `postgres` (PGDATA и VOLUME в 18+): https://hub.docker.com/_/postgres
- Внутренние документы: [architecture.md](architecture.md) (§6, §7), [stack-rationale.md](stack-rationale.md)
