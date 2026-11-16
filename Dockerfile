FROM rust:1.94.1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends build-essential cmake pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
ARG BUILD_JOBS=2
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target CARGO_BUILD_JOBS=$BUILD_JOBS cargo build --locked --release -p sessiondb-server && cp target/release/sessiondb-server /usr/local/bin/sessiondb-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --create-home sessiondb
COPY --from=builder /usr/local/bin/sessiondb-server /usr/local/bin/sessiondb-server
USER sessiondb
EXPOSE 8080
ENTRYPOINT ["sessiondb-server"]
