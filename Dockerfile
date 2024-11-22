# Этап 1: Сборка приложения
FROM rust:latest AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./

COPY . .
RUN cargo build --release

# Этап 2: Создание образа для выполнения
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/email_auto_reg /app/email_auto_reg
COPY proxy.txt /app/
RUN mkdir -p temp_uploads Logs
CMD ["./email_auto_reg"]
