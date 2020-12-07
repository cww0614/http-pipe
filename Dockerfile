FROM rust:1.44 AS build

WORKDIR /
RUN USER=root cargo new --bin app

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release || :

RUN rm -rf src
COPY . .
RUN touch **/*

RUN cargo build --release

# --- Executable stage ---

FROM debian:buster-slim

WORKDIR /app

RUN apt-get update \
    && apt-get install -y ca-certificates tini libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build \
     /app/target/release/http-pipe \
     /app/

ENTRYPOINT ["/usr/bin/tini", "--", "/app/http-pipe"]

CMD ["--server", "0.0.0.0:8080"]
