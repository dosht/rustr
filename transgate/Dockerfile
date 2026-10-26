FROM rust:1.99.1-bookworm AS toolchain
RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked wasm-bindgen-cli --version =0.2.128
WORKDIR /build
# Cargo loads every workspace member; the allow-list also supplies the host's
# compile-time checkout documents and the web build's versioned source assets.
COPY . ./

FROM toolchain AS server
RUN cargo build --locked --release --package transgate-server

FROM toolchain AS wasm
RUN cargo build --locked --release --package transgate-wasm --target wasm32-unknown-unknown \
    && wasm-bindgen --target web --out-dir /pkg target/wasm32-unknown-unknown/release/transgate_wasm.wasm

FROM node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205 AS web
WORKDIR /build/transgate/web
COPY transgate/web/package.json transgate/web/package-lock.json ./
RUN npm ci
WORKDIR /build
COPY transgate/web transgate/web
COPY transgate/export/fonts transgate/export/fonts
COPY transgate/export/assets transgate/export/assets
COPY --from=wasm /pkg transgate/web/src/lib/core/pkg
RUN cd transgate/web && npm run build

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/target/release/transgate-server /usr/local/bin/transgate-server
COPY --from=web /build/transgate/web/dist /srv/transgate-web
USER 65532:65532
ENV TRANSGATE_BIND=0.0.0.0:8788
ENV TRANSGATE_WEB_DIR=/srv/transgate-web
EXPOSE 8788
STOPSIGNAL SIGTERM
# Liveness only: no product API, database or provider readiness is asserted.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "--max-time", "2", "http://127.0.0.1:8788/health"]
ENTRYPOINT ["/usr/local/bin/transgate-server"]
