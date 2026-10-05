# Плейбук YouTube Push

## Выбор способа обновления

По умолчанию Dock Panel опрашивает публичные RSS-ленты YouTube каждые 15 минут.
Для этого сервер и домен не нужны. Синхронизация списка подписок Google —
отдельная функция и не заменяется WebSub.

Этот каталог содержит необязательный сервер WebSub для самостоятельного
развёртывания. Он принимает уведомления YouTube и отдаёт события клиенту.
В настройках YouTube выберите «Собственный сервер уведомлений», укажите
HTTPS-адрес и PUSH_TOKEN, затем сохраните. Токен хранится в Windows Credential
Manager. До подтверждения подписок и при сбое сервера сохраняется опрос раз
в 15 минут; при подключённом сервере — события и контрольный опрос раз в час.

Один сервер обслуживает один общий список каналов. Клиенты с разными списками
перезаписывают его: для независимых пользователей нужны отдельные экземпляры.

## Размещение

Каталог на вашем сервере: `/opt/dock-panel-push`.
Публичный адрес задаётся вами в `.env` через `PUSH_PUBLIC_URL`.
Текущее развёртывание использует Docker CLI и `deploy.sh`, без Docker Compose.
`compose.yaml` — альтернативная конфигурация; не запускайте её поверх текущего
развёртывания: она создаст другие контейнеры и тома.
При установке через Compose заполните оба секрета в `.env` заранее
(например, каждый результатом `openssl rand -hex 32`), затем выполните
`docker compose up -d --build` в каталоге сервиса. `deploy.sh` генерирует
секреты только для установки через Docker CLI.

Контейнеры:

- `dock-panel-push-relay`: Python, SQLite, приём WebSub и выдача событий клиенту.
- `dock-panel-push-https`: Caddy, HTTPS и автоматическое продление сертификата.

Каждый ограничен 96 МБ RAM, 0.35 CPU и 64 процессами. Relay не публикует порт
на хосте; Caddy занимает TCP 80 и 443. Политика перезапуска — `unless-stopped`.
Контейнеры Amnezia не входят в это развёртывание.

## Первая установка

Требуются Docker, Python 3 на хосте, свободные TCP 80/443 и A-запись домена
на этот сервер. Скопируйте `service.py`, `Dockerfile`, `Caddyfile`, `deploy.sh`
в `/opt/dock-panel-push`, затем выполните на сервере:

```sh
sh /opt/dock-panel-push/deploy.sh
```

Перед запуском создайте `.env` по примеру `.env.example`: укажите собственный
домен в `PUSH_DOMAIN` и HTTPS-адрес в `PUSH_PUBLIC_URL`. Скрипт отказывается
перезаписывать существующие контейнеры и генерирует отсутствующие секреты
с правами 600: `PUSH_TOKEN` для клиента и `PUSH_SECRET` для подписей callback. Не публикуйте этот файл и не выводите его в общие логи.

## Проверка

```sh
docker ps --filter name=dock-panel-push
docker inspect --format '{{.State.Health.Status}}' dock-panel-push-relay
docker stats --no-stream dock-panel-push-relay dock-panel-push-https
curl -fsS https://push.example.com/health
docker logs --tail 50 dock-panel-push-relay
docker logs --tail 50 dock-panel-push-https
```

Ожидается `healthy` и ответ `{"ok": true}`. Это проверяет доступность сервиса,
но не подтверждает подписку на каналы и доставку нового видео. Для этого
клиент должен передать каналы через авторизованный `POST /channels`, а
`GET /status` должен показать действующие leases. `GET /events?after=N`
возвращает события через длинный запрос. Приватные маршруты требуют Bearer-токен.

## Обновление обработчика и откат

Скопируйте новую версию исходников в рабочий каталог. Команды ниже выполнять
на сервере; они пересоздают оба контейнера этого сервиса. Перед изменением сохраните резервную
копию данных по инструкции ниже.

```sh
cd /opt/dock-panel-push
docker image tag dock-panel-push:local dock-panel-push:rollback
docker build -t dock-panel-push:local .
docker stop dock-panel-push-relay dock-panel-push-https
docker rm dock-panel-push-relay dock-panel-push-https
sh deploy.sh
```

При пересоздании есть короткий перерыв в доступности. Именованные тома и `.env`
сохраняются. Скрипт также повторно получает образ Caddy. Если новая версия
relay не работает, верните прежний образ, не выполняя повторную сборку:

```sh
docker stop dock-panel-push-relay
docker rm dock-panel-push-relay
docker image tag dock-panel-push:rollback dock-panel-push:local
docker run -d --name dock-panel-push-relay --restart unless-stopped \
  --network dock-panel-push --network-alias relay \
  --env-file /opt/dock-panel-push/.env -v dock-panel-push-data:/data \
  --read-only --tmpfs /tmp:size=8m --memory 96m --cpus .35 --pids-limit 64 \
  --security-opt no-new-privileges:true --cap-drop ALL \
  --log-opt max-size=5m --log-opt max-file=2 \
  --health-cmd "python -c \"import urllib.request; urllib.request.urlopen('http://127.0.0.1:8791/health', timeout=3).read()\"" \
  --health-interval 30s --health-timeout 5s --health-retries 3 dock-panel-push:local
```

Этот откат возвращает код обработчика; изменение схемы SQLite требует отдельно
проверенной миграции или восстановления резервной копии.

## Резервная копия

Остановите relay, чтобы копия SQLite вместе с WAL была согласованной.
Резервная копия содержит секреты: храните её с ограниченным доступом.

```sh
cd /opt/dock-panel-push
umask 077
mkdir -p backups
docker stop dock-panel-push-relay
docker run --rm --network none --user 0 --entrypoint sh \
  -v dock-panel-push-data:/data:ro -v "$PWD/backups:/backup" \
  dock-panel-push:local -c 'tar czf /backup/relay-data.tar.gz -C /data .'
cp .env backups/relay.env
docker start dock-panel-push-relay
```

Если копирование завершилось ошибкой, всё равно запустите relay последней
командой. Сохраните также Caddyfile и исходники нужной версии. Сертификаты
хранятся в томе `dock-panel-push-caddy-data`; Caddy может получить их заново.

## Остановка

```sh
docker stop dock-panel-push-https dock-panel-push-relay
```

Возобновление: `docker start dock-panel-push-relay dock-panel-push-https`.
Не удаляйте тома: `dock-panel-push-data` содержит подписки и события,
`dock-panel-push-caddy-data` и `dock-panel-push-caddy-config` — данные Caddy.
