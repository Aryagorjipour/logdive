# logdive examples

Sample log files and a query cookbook covering every feature of logdive v0.3.0.

## Files

| File | Format | What it is | Entries |
|---|---|---|---|
| `app.log` | JSON | Backend services: `payments`, `orders`, `auth` | 60 |
| `nginx.log` | JSON | Nginx access logs | 25 |
| `worker.log` | logfmt | Background job worker: email and notification queues | 41 |
| `deploy.log` | plain text | Deployment pipeline output (no structured fields) | 28 |

All four files share the same time window (2026-04-15, 09:00–12:51 UTC).

---

## Setup

These examples use a throwaway database so they don't touch your default index. Run from the repository root.

### Ingest all files

```bash
DB=/tmp/logdive-examples.db

# JSON files — default format
logdive --db $DB ingest --file examples/app.log
logdive --db $DB ingest --file examples/nginx.log

# logfmt file — specify format explicitly
logdive --db $DB ingest --file examples/worker.log --format logfmt

# Plain text — no structured fields; --timestamp-now stamps each line at ingest time
logdive --db $DB ingest --file examples/deploy.log --format plain --timestamp-now
```

### Check what you ingested

```bash
logdive --db $DB stats
```

Expected output:
```
entries:    154
time range: 2026-04-15T09:00:05Z → 2026-04-15T12:51:19Z
tags:       auth (24), nginx (25), orders (19), payments (18), worker (41), (untagged) (3 + 28 deploy lines)
db size:    ~0.3 MB
```

---

## Query cookbook

### Alias for brevity

```bash
alias ld='logdive --db /tmp/logdive-examples.db query'
```

---

### Basic level filtering

```bash
# All errors across every source
ld 'level=error'

# All warnings
ld 'level=warn'

# Case-insensitive level matching — new in v0.3.0
# These three return identical results
ld 'level=error'
ld 'level=ERROR'
ld 'level=Error'

# Exclude info noise — show everything that isn't info
ld 'level!=info'
```

---

### OR queries — new in v0.3.0

```bash
# Errors or warnings from any source
ld 'level=error OR level=warn'

# Worker queue errors or queue connection issues
ld 'queue=email OR queue=notifications'

# Payments or auth service entries
ld 'service=payments OR service=auth'
```

---

### Parenthesised groups — new in v0.3.0

Combine OR branches with AND conditions using parentheses.

```bash
# Errors or warnings, but only from payments
ld '(level=error OR level=warn) AND service=payments'

# Errors from payments or auth
ld '(service=payments OR service=auth) AND level=error'

# Failed jobs from either queue
ld '(queue=email OR queue=notifications) AND level=error'

# Auth problems: rate limiting or invalid tokens
ld '(message contains "rate limit" OR message contains "invalid token") AND tag=auth'

# nginx 4xx and 5xx responses from a specific IP range
ld '(status > 399) AND tag=nginx'

# Critical worker events — errors or permanent failures
ld '(level=error OR message contains "permanently") AND tag=worker'
```

---

### AND chains

```bash
# Slow payments (over 5 seconds)
ld 'service=payments AND duration_ms > 5000'

# Nginx 5xx responses
ld 'tag=nginx AND status > 499'

# Auth errors involving a specific user
ld 'service=auth AND level=error AND user_id=1147'

# Worker job failures with specific queue
ld 'tag=worker AND level=error AND queue=email'
```

---

### CONTAINS — full-text substring match

```bash
# Any mention of timeout
ld 'message contains "timeout"'

# Job failure events
ld 'message contains "failed"'

# Database-related events
ld 'message contains "database"'

# nginx scanner/probe traffic
ld 'tag=nginx AND message contains "access" AND path contains "wp-admin"'

# Payment errors mentioning specific error codes
ld 'message contains "GATEWAY_TIMEOUT" OR message contains "CARD_DECLINED"'
```

---

### Field comparisons

```bash
# Slow requests from nginx (over 1 second)
ld 'request_time > 1.0'

# Very fast responses only
ld 'request_time < 0.010'

# High-latency backend calls (over 2 seconds)
ld 'duration_ms > 2000'

# Large orders
ld 'total > 100'

# Specific HTTP status
ld 'status=503'

# Worker jobs that took over 300ms
ld 'tag=worker AND duration_ms > 300'
```

---

### Time-range queries

```bash
# Everything from 11:00 UTC onwards
ld 'since 2026-04-15T11:00:00Z'

# Errors in the morning window only
ld 'level=error AND since 2026-04-15T09:00:00Z'

# Combine with OR groups and time range
ld '(level=error OR level=warn) AND service=payments AND since 2026-04-15T09:20:00Z'

# Note: 'last Nh/Nd' is relative to now — use 'since' for fixture data with fixed timestamps
# For live logs: ld 'level=error last 1h'
```

---

### Tag-based filtering

```bash
# All nginx access log entries
ld 'tag=nginx'

# Errors from any tagged source
ld 'level=error AND tag=payments'

# Worker queue stats only
ld 'tag=worker AND message contains "queue stats"'

# Everything not from nginx
ld 'level=error AND tag!=nginx'
```

---

### Output formats — `--output` flag, new in v0.3.0

`--output` was renamed from `--format` in v0.3.0.

```bash
# Pretty-printed output (default) — human readable with colours
ld 'level=error' --output pretty

# JSON output — one JSON object per line (NDJSON), pipe-friendly
ld 'level=error' --output json

# Pipe to jq — extract specific fields
ld 'level=error' --output json | jq '{ts: .timestamp, svc: .service, msg: .message}'

# Aggregate: count errors per service
ld 'level=error' --output json | jq -r '.service' | sort | uniq -c | sort -rn

# Extract all error codes from payments
ld 'service=payments AND level=error' --output json | jq '.error_code // empty'

# Flatten all fields from worker failures
ld 'tag=worker AND level=error' --output json | jq '.'

# Build a CSV of slow nginx requests
ld 'tag=nginx AND request_time > 0.5' --output json \
  | jq -r '[.timestamp, .path, .status, .request_time] | @csv'
```

---

### Pagination — `--limit` and `--offset`, new in v0.3.0

```bash
# First 5 errors
ld 'level=error' --limit 5

# Next 5 (page 2)
ld 'level=error' --limit 5 --offset 5

# Page 3
ld 'level=error' --limit 5 --offset 10

# Unlimited results (override default limit of 1000)
ld 'tag=worker' --limit 0

# Combine pagination with JSON output for scripted consumption
ld 'level=error' --limit 10 --offset 0 --output json
ld 'level=error' --limit 10 --offset 10 --output json
```

---

### Realistic investigation workflows

**"What went wrong around 09:22?"**

```bash
ld 'since 2026-04-15T09:20:00Z' --limit 20
```

**"Show me the full payment failure chain for order ord_28472"**

```bash
ld 'order_id=ord_28472' --output json | jq '{ts: .timestamp, svc: .service, lvl: .level, msg: .message}'
```

**"Are there any errors or warnings I should care about right now?"**

```bash
ld 'level=error OR level=warn' --output pretty
```

**"Which user triggered rate limiting?"**

```bash
ld 'message contains "rate limit" OR message contains "rate_limit"' --output json | jq '.user_id // empty'
```

**"Show me all nginx responses that weren't 200"**

```bash
ld 'tag=nginx AND status!=200' --output json | jq '{ts: .timestamp, path: .path, status: .status}'
```

**"Did any worker jobs fail permanently?"**

```bash
ld 'tag=worker AND message contains "permanently"'
```

**"Find the worker reconnect after the payment worker panic"**

```bash
ld '(message contains "reconnect" OR message contains "restarted") AND since 2026-04-15T11:27:00Z'
```

---

## HTTP API

Start the API server against the example database:

```bash
logdive-api --db /tmp/logdive-examples.db --port 4000 &
```

### Basic queries

```bash
# Stats
curl -s 'http://127.0.0.1:4000/stats' | jq

# Version
curl -s 'http://127.0.0.1:4000/version' | jq

# All errors
curl -s 'http://127.0.0.1:4000/query?q=level%3Derror' | jq -s .

# Errors from payments (URL-encode spaces and =)
curl -s 'http://127.0.0.1:4000/query?q=service%3Dpayments%20AND%20level%3Derror' | jq -s .
```

### Pagination — `?offset=`, new in v0.3.0

```bash
# Page 1: first 5 errors
curl -s 'http://127.0.0.1:4000/query?q=level%3Derror&limit=5&offset=0' | jq -s .

# Page 2: next 5
curl -s 'http://127.0.0.1:4000/query?q=level%3Derror&limit=5&offset=5' | jq -s .
```

### OR and paren queries over HTTP

```bash
# Errors or warnings (URL-encode OR)
curl -s 'http://127.0.0.1:4000/query?q=level%3Derror%20OR%20level%3Dwarn' | jq -s .

# Parenthesised group
curl -s 'http://127.0.0.1:4000/query?q=%28level%3Derror%20OR%20level%3Dwarn%29%20AND%20service%3Dpayments' | jq -s .
```

### Stop the server

```bash
kill %1
```

---

## Stats and prune

```bash
# Full index statistics
logdive --db /tmp/logdive-examples.db stats

# Dry-run prune: see what would be deleted before cutoff
# (nothing to delete in the example DB — all entries are from 2026-04-15)
logdive --db /tmp/logdive-examples.db prune --before 2026-04-14T00:00:00Z

# Prune entries older than 30 days from a live index
# logdive --db ~/my-index.db prune --older-than 30d

# Prune without confirmation prompt (for automation)
# logdive --db ~/my-index.db prune --older-than 7d --yes
```

---

## Multi-format ingestion in depth

### logfmt

`worker.log` uses logfmt — the format popularized by go-kit and Heroku. Keys `timestamp`, `level`, and `message` map to indexed columns; all other keys go into the queryable `fields` blob.

```bash
logdive --db /tmp/logdive-examples.db ingest --file examples/worker.log --format logfmt

# Query the worker logs just like any JSON log
logdive --db /tmp/logdive-examples.db query 'tag=worker AND level=error'
logdive --db /tmp/logdive-examples.db query 'queue=email AND level!=info'
logdive --db /tmp/logdive-examples.db query 'job_type=send_confirmation'
```

### Plain text

`deploy.log` is raw deployment output — no structured fields at all. Use `--format plain` to ingest it; each line becomes the `message` field. Use `--timestamp-now` to stamp entries at ingest time (otherwise lines without a `timestamp` field are skipped).

```bash
logdive --db /tmp/logdive-examples.db ingest \
  --file examples/deploy.log \
  --format plain \
  --timestamp-now \
  --tag deploy

# Query by content — CONTAINS is the only useful operator here
logdive --db /tmp/logdive-examples.db query 'tag=deploy AND message contains "failed"'
logdive --db /tmp/logdive-examples.db query 'tag=deploy AND message contains "Build step"'
logdive --db /tmp/logdive-examples.db query 'tag=deploy AND message contains "successful"'
```

### Piping live sources

```bash
# Docker container logs
docker logs -f my-container | logdive --db ~/my-index.db ingest --tag my-container

# systemd journal (JSON format)
journalctl --output=json -f | logdive --db ~/my-index.db ingest --tag systemd

# kubectl pod logs
kubectl logs -f my-pod | logdive --db ~/my-index.db ingest --tag k8s-my-pod

# Pipe with --follow for a file that's still being written to
logdive --db ~/my-index.db ingest --file /var/log/app.log --follow
```

---

## Cleanup

```bash
rm /tmp/logdive-examples.db
```

---

## Now try your own logs

```bash
DB=~/my-index.db
logdive --db $DB ingest --file /path/to/your-app.log
logdive --db $DB stats
logdive --db $DB query 'level=error last 1h'
logdive --db $DB query '(level=error OR level=warn) AND service=your-service'
```

See the main [README](../README.md) for the full query language reference and performance numbers.
