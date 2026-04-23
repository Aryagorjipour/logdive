# logdive examples

Sample JSON log files to try logdive against. Both files use realistic formats from the backend and web-server worlds.

## Files

| File | What it is | Entries |
|---|---|---|
| `app.log` | Structured application logs from a mock e-commerce backend (three services: `payments`, `orders`, `auth`) | 60 |
| `nginx.log` | Structured nginx access logs in JSON format | 25 |

Both files cover the same time window (early morning on 2026-04-15) so queries can span them cleanly.

## Quick start

These examples use a throwaway database at `/tmp/logdive-examples.db` so they don't touch your default `~/.logdive/index.db`. Adjust the path if you prefer.

### 1. Ingest both files

```bash
logdive --db /tmp/logdive-examples.db ingest --file examples/app.log
logdive --db /tmp/logdive-examples.db ingest --file examples/nginx.log
```

### 2. Inspect what you've got

```bash
logdive --db /tmp/logdive-examples.db stats
```

Expected: 85 entries, time range spanning roughly 09:00 to 12:51 UTC, four distinct tags (`auth`, `nginx`, `orders`, `payments`, plus `(untagged)`).

### 3. Try some queries

Find every error:

```bash
logdive --db /tmp/logdive-examples.db query 'level=error'
```

Find payment failures specifically:

```bash
logdive --db /tmp/logdive-examples.db query 'service=payments AND level=error'
```

Find any message mentioning timeouts:

```bash
logdive --db /tmp/logdive-examples.db query 'message contains "timeout"'
```

Find 5xx responses from the nginx logs:

```bash
logdive --db /tmp/logdive-examples.db query 'tag=nginx AND status > 499'
```

Find slow requests (over 1 second):

```bash
logdive --db /tmp/logdive-examples.db query 'request_time > 1.0'
```

Find everything from the last hour of the fixture window:

```bash
# Depending on when you read this, you may need to adjust the `since` datetime.
logdive --db /tmp/logdive-examples.db query 'since 2026-04-15T11:00:00Z'
```

Pipe structured output into `jq` for further manipulation:

```bash
logdive --db /tmp/logdive-examples.db query 'level=error' --format json | jq '{when: .timestamp, who: .service, what: .message}'
```

### 4. Spin up the HTTP API

```bash
logdive-api --db /tmp/logdive-examples.db --port 4000 &

curl -s 'http://127.0.0.1:4000/stats' | jq
curl -s 'http://127.0.0.1:4000/query?q=level%3Derror&limit=5' | jq -s .

# When done:
kill %1
```

## Piping from Docker

One of logdive's intended use cases: pipe a running container's logs straight in.

```bash
docker logs -f my-container | logdive --db /tmp/my-app.db ingest --tag my-container
```

Works because Docker logs structured applications emit one JSON object per line on stdout. The `--tag` flag attaches a source label so you can later query `tag=my-container AND level=error`.

Same pattern works with `kubectl logs`, `journalctl --output=json`, and any other tool that streams NDJSON to stdout.

## Cleanup

```bash
rm /tmp/logdive-examples.db
```

## Now try your own logs

```bash
logdive --db ~/my-index.db ingest --file /path/to/your-app.log
logdive --db ~/my-index.db stats
logdive --db ~/my-index.db query 'your query here'
```

See the main [README](../README.md) for the full query language reference.
