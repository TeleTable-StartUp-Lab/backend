# Hosting Documentation

## Architecture Overview

The stack runs on a Debian server. Public traffic enters via a Cloudflare Tunnel, which forwards to Nginx on the host. Nginx terminates the internal connection (self-signed cert) and proxies to the application containers. All core application services are managed via Docker Compose. Monitoring tooling runs in a separate Compose stack.

```
Internet
  │
  ▼
Cloudflare (TLS termination, CDN)
  │  (HTTPS)
  ▼
cloudflared (host daemon) ──► Nginx (host, self-signed cert internally)
                                │
                    ┌───────────┴───────────┐
                    │                       │
               Axum backend            (static frontend
               container               built & served
                    │                  via Nginx)
               Redis container
                    │
             Postgres container
                    │
          postgres-backup-local
               container
```

**Access:**

- Public: via Cloudflare Tunnel only — no ports exposed directly to the internet
- Admin: SSH with public key authentication
- Internal (by IP): Nginx serves with a self-signed cert over HTTPS

---

## Directory Layout

```
/home/lukas/srv/backend
├── docker-compose.yml        # Core stack: backend, redis, postgres, backup
├── .env                      # Secrets and config for core stack
├── backups/                  # Postgres backup volume (mounted by backup container)
└── logs/

/home/lukas/srv/infrastructure
├── docker-compose.yml    # Monitoring stack: Dozzle, Uptime Kuma
└── .env                  # Config for monitoring stack (if any)
```

---

## Docker Setup

### Core Stack — `backend/docker-compose.yml`

Services:

- `backend` — Rust/Axum HTTP API, image built locally by the GitHub Actions self-hosted runner
- `postgres` — PostgreSQL 16
- `redis` — Cache / session store
- `backup` — `prodrigestivill/postgres-backup-local:16`, runs daily backups

Key points:

- Secrets and environment variables are loaded from `.env` in the same directory
- No ports should be exposed to `0.0.0.0` — backend should only listen on a named Docker network, with Nginx proxying to it

### Monitoring Stack — `infrastructure/docker-compose.yml`

Services:

- `dozzle` — Real-time log viewer for all Docker containers
- `uptime-kuma` — Uptime monitoring for internal and external endpoints

This stack is managed independently. Bring it up/down without affecting the core stack.

## Nginx & Reverse Proxy

Nginx is installed on the host (not containerised). It handles two concerns:

1. **Proxying from cloudflared** — forwards decrypted traffic from the Cloudflare Tunnel to the appropriate backend or frontend
2. **Direct IP access** — serves with a self-signed certificate for internal HTTPS access by IP

TLS termination for public traffic is handled entirely by Cloudflare. Nginx only needs to handle the internal leg.

## Cloudflare Tunnel

`cloudflared` runs as a host daemon (systemd service). It creates an outbound-only persistent tunnel to Cloudflare's edge - no inbound ports need to be opened on the server firewall.

Traffic flow: `Cloudflare edge → cloudflared (host) → Nginx (host) → containers`

### Key Files

```
/etc/cloudflared/config.yml    # Tunnel config (ingress rules, credentials path)
~/.cloudflared/                # May contain credentials if set up per-user
```

### Ingress Config Pattern (`config.yml`)

```yaml
tunnel: <tunnel-id>
credentials-file: /etc/cloudflared/<tunnel-id>.json

ingress:
  - hostname: example.com
    service: https://localhost:443
    originRequest:
      noTLSVerify: true # Required because Nginx uses a self-signed cert internally
  - service: http_status:404
```

> `noTLSVerify: true` is intentional — the self-signed cert on Nginx is for the private tunnel leg only. Public TLS is handled by Cloudflare.

## Backups

Postgres backups are handled by the `prodrigestivill/postgres-backup-local:16` container, which runs on a daily cron schedule internal to the container.

### Storage

Backups are stored in `/home/lukas/srv/backups/` on the host, bind-mounted into the backup container. Files are gzip-compressed SQL dumps.

> **Note:** Backups are currently local only (same server as the database). If the server is lost, backups are lost too. Consider periodic offsite copy (rclone to S3/B2, rsync to another host, etc.).

### Retention

Daily backups. Review the `BACKUP_KEEP_DAYS`, `BACKUP_KEEP_WEEKS`, and `BACKUP_KEEP_MONTHS` env vars in `.env` to confirm the current retention policy and adjust as needed.

## Monitoring & Logging

### Dozzle

Web UI for streaming Docker container logs in real time. Accessible internally.

- Logs are ephemeral — they are cleared when a container restarts
- For persistent logs look in the log directory

### Uptime Kuma

Monitors both internal services and external public endpoints. Accessible internally via its own port.

- Public domain HTTP(S) checks, internal container ports (backend, redis, postgres)
- Configured notification channels (email, Telegram, etc.) for alerting

## Deployment Process

Deployments are triggered by GitHub Actions using a **self-hosted runner** on the server itself. There is no external registry — images are built directly on the server.

### What Happens on a Push/Merge

1. GitHub Actions picks up the workflow on the self-hosted runner
2. Runner builds the Docker image for the Rust/Axum backend on the server
3. Runner builds the frontend (static assets)
4. Frontend build output is placed in the directory Nginx serves from
5. Runner runs `docker compose up -d --build` (or equivalent) to restart the backend with the new image

## Runbooks

### Restore Postgres from Backup

1. Identify the backup file to restore:

   ```bash
   ls /home/lukas/srv/backups/
   ```

2. Stop the backend to prevent writes during restore:

   ```bash
   cd /home/lukas/srv/backend/
   docker compose stop teletable-backend
   ```

3. Drop and recreate the database (connect via psql):

   ```bash
   docker compose exec postgres psql -U <db_user> -c "DROP DATABASE <db_name>;"
   docker compose exec postgres psql -U <db_user> -c "CREATE DATABASE <db_name>;"
   ```

4. Restore from the dump:

   ```bash
   gunzip -c /home/lukas/srv/backups/<backup_file>.sql.gz \
     | docker compose exec -T postgres psql -U <db_user> <db_name>
   ```

5. Restart the backend:

   ```bash
   docker compose start backend
   ```

6. Verify the application is healthy

### Restart a Single Service

```bash
cd /home/lukas/srv
docker compose restart <service>   # backend | redis | postgres | backup
```

---

### Full Stack Restart

```bash
cd /home/lukas/srv
docker compose down && docker compose up -d
```

---

### SSH Access

Authentication is public key only - password auth is disabled.

```bash
ssh lukas@<server-ip>
```

To add a new authorized key:

```bash
echo "<public-key>" >> ~/.ssh/authorized_keys
chmod 600 ~/.ssh/authorized_keys
```

To revoke access, remove the relevant line from `~/.ssh/authorized_keys`.
