# transgate-processor

The free-tools processor: a separate deployable that runs the seven public tool
families server-first on FFmpeg (P3 D1). It is the Rust port of
`frontend/services/free-tools-processor`. The browser calls it directly at
`NEXT_PUBLIC_FREE_TOOLS_PROCESSOR_URL`; no Transgate server route proxies it,
and it creates no paid jobs. It has no authentication: sessions are anonymous
server-minted UUIDs.

Product rules (formats, limits, TTLs, presets, progress math, allowance window)
are in `transgate/core/src/tools.rs`. This crate is the HTTP, file and FFmpeg shell.

## Routes

`GET /health` plus 39 tool endpoints:

| Mount | Endpoints |
|---|---|
| `/trim-audio` | POST `upload`, POST `trim`, GET `progress`, GET `download?part=1\|2`, GET `download-zip`, DELETE `invalidate` |
| `/convert-audio` | POST `upload`, POST `convert`, GET `progress`, GET `download`, DELETE `invalidate` |
| `/reduce-audio` | POST `upload`, POST `reduce`, GET `progress`, GET `download`, DELETE `invalidate` |
| `/remove-noise` | POST `upload`, POST `denoise`, GET `progress`, GET `download`, DELETE `invalidate` |
| `/video-to-audio` | POST `upload`, GET `thumbnail`, POST `extract`, GET `progress`, GET `download`, DELETE `invalidate` |
| `/compress-video` | POST `upload`, GET `thumbnail`, POST `compress`, GET `progress`, GET `download`, DELETE `invalidate` |
| `/convert-video` | POST `upload`, GET `thumbnail`, POST `convert`, GET `progress`, GET `download`, DELETE `invalidate` |

Every ten minutes the service purges expired sessions and orphaned files of all
seven families. The TTL is one or two hours, by family.

## Running

```sh
cargo run -p transgate-processor          # needs ffmpeg and ffprobe on PATH
cargo test -p transgate-processor -p transgate-core
docker build -f transgate/processor/deploy/Dockerfile -t transgate-processor .
```

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8080` | Listen port, on all addresses. If set, it must be 1-65535 or the service exits 2 |
| `ALLOWED_ORIGINS` | `https://transgate.ai` | Comma-separated CORS origins |
| `REQUEST_TIMEOUT_MS` | 30 minutes | Longest time a request body may take (408 after) |
| `FREE_TOOLS_TRUSTED_PROXY_COUNT` | `1` | Proxies that append to `X-Forwarded-For`; the allowance key is read that far from the right |
| `FFMPEG_THREADS` | `1` | `-threads` for decode and encode |
| `FFMPEG_PATH`, `FFPROBE_PATH` | from `PATH` | Binaries |
| `TMPDIR` | `/tmp` | Where session files live |

Run exactly one instance. Sessions and processing allowances are held in
memory, so a restart loses them, while their files persist until the periodic
purge removes them. Running two replicas splits sessions between them
(OPS-D06). `deploy/` holds a Caddy TLS edge and a compose file for one host.
