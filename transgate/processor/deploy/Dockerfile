# Build from the repository root:
#   docker build -f transgate/processor/deploy/Dockerfile -t transgate-processor .
FROM rust:1.99.1-bookworm AS build
WORKDIR /build
# Cargo loads every workspace member even when building only this package;
# Dockerfile.dockerignore limits the context to member manifests, Rust source and migrations.
COPY . ./
RUN cargo build --locked --release --package transgate-processor

FROM debian:bookworm-slim AS runtime
# FFmpeg and ffprobe are this service's runtime; the main Transgate image has neither.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/transgate-processor /usr/local/bin/transgate-processor
USER 65532:65532
ENV PORT=8080
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent --show-error --max-time 2 "http://127.0.0.1:${PORT}/health" || exit 1
ENTRYPOINT ["/usr/local/bin/transgate-processor"]
