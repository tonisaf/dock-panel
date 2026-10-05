#!/bin/sh
set -eu
cd /opt/dock-panel-push
if docker container inspect dock-panel-push-relay >/dev/null 2>&1 || docker container inspect dock-panel-push-https >/dev/null 2>&1; then
    echo 'Existing relay containers found; refusing to overwrite.' >&2
    exit 1
fi
if [ ! -f .env ]; then
    echo 'Create .env from .env.example with your domain and public HTTPS URL.' >&2
    exit 1
fi
python3 - <<'PY'
import secrets
from pathlib import Path
p = Path('.env')
lines = p.read_text().splitlines()
values = dict(line.split('=', 1) for line in lines if '=' in line and not line.startswith('#'))
domain = values.get('PUSH_DOMAIN', '')
if not domain or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-' for c in domain) or domain == 'push.example.com':
    raise SystemExit('Set PUSH_DOMAIN to your own hostname')
if values.get('PUSH_PUBLIC_URL') != 'https://' + domain:
    raise SystemExit('PUSH_PUBLIC_URL must equal https://PUSH_DOMAIN')
for key in ('PUSH_TOKEN', 'PUSH_SECRET'):
    if not values.get(key):
        lines = [line for line in lines if not line.startswith(key + '=')]
        lines.append(key + '=' + secrets.token_hex(32))
p.chmod(0o600)
p.write_text('\n'.join(lines) + '\n')
PY
chmod 600 .env
docker network inspect dock-panel-push >/dev/null 2>&1 || docker network create dock-panel-push
for volume in dock-panel-push-data dock-panel-push-caddy-data dock-panel-push-caddy-config; do
    docker volume inspect "$volume" >/dev/null 2>&1 || docker volume create "$volume"
done
docker build -t dock-panel-push:local .
docker pull caddy:2-alpine
docker run -d --name dock-panel-push-relay --restart unless-stopped \
    --network dock-panel-push --network-alias relay \
    --env-file .env --volume dock-panel-push-data:/data \
    --read-only --tmpfs /tmp:size=8m --memory 96m --cpus .35 --pids-limit 64 \
    --security-opt no-new-privileges:true --cap-drop ALL \
    --log-opt max-size=5m --log-opt max-file=2 \
    --health-cmd "python -c \"import urllib.request; urllib.request.urlopen('http://127.0.0.1:8791/health', timeout=3).read()\"" \
    --health-interval 30s --health-timeout 5s --health-retries 3 \
    dock-panel-push:local
docker run -d --name dock-panel-push-https --restart unless-stopped \
    --network dock-panel-push --env-file .env -p 80:80 -p 443:443 \
    --volume /opt/dock-panel-push/Caddyfile:/etc/caddy/Caddyfile:ro \
    --volume dock-panel-push-caddy-data:/data --volume dock-panel-push-caddy-config:/config \
    --memory 96m --cpus .35 --pids-limit 64 --security-opt no-new-privileges:true \
    --log-opt max-size=5m --log-opt max-file=2 caddy:2-alpine
