FROM rust:1.97-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY mobile/src-tauri ./mobile/src-tauri
RUN cargo build --locked --release -p lumo-api

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --create-home lumo \
    && mkdir -p /data /certs \
    && chown -R lumo:lumo /data /certs

COPY --from=builder /src/target/release/lumo-api /usr/local/bin/lumo-api

USER lumo
EXPOSE 8443
VOLUME ["/data", "/certs"]
ENTRYPOINT ["/usr/local/bin/lumo-api"]
