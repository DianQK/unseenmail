FROM rust:1.69.0-bullseye as builder
WORKDIR /usr/src/unseenmail
COPY . .
RUN cargo install --path .

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/unseenmail/scripts/entrypoint.sh /entrypoint.sh
COPY --from=builder /usr/local/cargo/bin/unseenmail /usr/local/bin/unseenmail
RUN chmod +x /usr/local/bin/unseenmail
RUN chmod +x /entrypoint.sh
WORKDIR /app
VOLUME /app
CMD ["sh", "/entrypoint.sh"]
