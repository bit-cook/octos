---
name: web-tunnel
version: 1.0.0
author: octos
description: Deploy websites to public internet via Cloudflare Tunnel with a management dashboard
always: false
---

# Web Tunnel Skill

You can deploy websites to the public internet via Cloudflare Tunnel and manage them through a Dashboard API. All services run in Docker containers.

## Prerequisites

The tunnel system must be running. Start it with:

```bash
cd $OCTOS_HOME
docker compose up -d
docker compose --profile tunnel up -d cloudflared
```

Dashboard API is available at `http://localhost:9080`.

## Available Operations

### List all sites

```bash
curl -s http://localhost:9080/api/sites
```

Returns JSON array of all sites with their status, ports, and public URLs.

### Create a new site

```bash
curl -s -X POST http://localhost:9080/api/sites \
  -H "Content-Type: application/json" \
  -d '{"name":"my-site","subdomain":"my-site","title":"My Site"}'
```

Parameters:
- `name` (required): Container name, lowercase, no spaces (use hyphens)
- `subdomain` (required): Subdomain under octos-cloud.org (site will be at `https://{subdomain}.octos-cloud.org`)
- `title` (optional): Display title for the generated page
- `color` (optional): CSS gradient color for the generated page (e.g. `#6366f1`)

This automatically:
1. Creates an nginx container with a default page
2. Registers DNS CNAME at Cloudflare
3. Updates cloudflared tunnel config
4. Restarts cloudflared to apply changes

The site becomes accessible at `https://{subdomain}.octos-cloud.org` within seconds.

### Stop a site

```bash
curl -s -X POST http://localhost:9080/api/sites/{site_id}/stop
```

Stops the container. The site goes offline but is not deleted.

### Start a site

```bash
curl -s -X POST http://localhost:9080/api/sites/{site_id}/start
```

Restarts a previously stopped site.

### Delete a site

```bash
curl -s -X DELETE http://localhost:9080/api/sites/{site_id}
```

Removes the container, site files, and updates tunnel config.

### Reload Cloudflared

```bash
curl -s -X POST http://localhost:9080/api/cloudflared/reload
```

Regenerates tunnel config from current site registry and restarts cloudflared.

## Custom Website Content

After creating a site, you can replace the default page by writing HTML to the site's directory:

```bash
# Write custom HTML for a site named "my-site"
cat > ~/.octos/www/my-site/index.html << 'EOF'
<!DOCTYPE html>
<html>
<head><title>My Custom Site</title></head>
<body><h1>Hello World</h1></body>
</html>
EOF
```

The nginx container serves from this directory with live reload (no restart needed).

## Workflow Example

When user asks "help me create a bakery website at bakery.octos-cloud.org":

1. Create the site: `curl -X POST http://localhost:9080/api/sites -H "Content-Type: application/json" -d '{"name":"bakery","subdomain":"bakery","title":"Bakery"}'`
2. Generate beautiful HTML for the bakery website
3. Write it to `~/.octos/www/bakery/index.html`
4. The site is immediately live at `https://bakery.octos-cloud.org`

## Dashboard

The web Dashboard at `http://localhost:9080` provides a visual interface to:
- View all sites and their status (auto-refreshes every 10 seconds)
- Start/Stop sites with one click
- Add new sites via form
- Delete sites
- Reload cloudflared config

## Domain

All sites are mapped under `octos-cloud.org`: `https://{subdomain}.octos-cloud.org`

## Notes

- Avoid pure numeric names for sites (use letter prefix like `site-123`)
- New DNS records may take a few minutes to propagate; if a subdomain was previously queried before creation, local DNS cache may need flushing
- All data persists in Docker volumes; Cloudflare auth survives container restarts
