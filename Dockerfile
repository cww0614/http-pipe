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

COPY --from=build \
     /app/target/release/http-pipe-server \
     /app/

CMD ["/app/http-pipe-server", "0.0.0.0:8080"]
