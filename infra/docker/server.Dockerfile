# syntax=docker/dockerfile:1
# The game server image: the built client (client/dist) and the release server binary that serves it.
# Build context: the repository root (infra/docker-compose.yml sets it).

# ---- client: proto code, art check, type check, bundle
FROM node:22-bookworm-slim AS client
WORKDIR /src/client
COPY client/package.json client/package-lock.json ./
RUN npm ci
COPY proto/ /src/proto/
COPY client/ ./
RUN npm run build

# ---- server: release build (protoc is vendored, nothing else to install)
FROM rust:1.90-bookworm AS server
WORKDIR /src/server
COPY proto/ /src/proto/
COPY server/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cargo build --release --locked \
    && cp target/release/sudden-front-server /usr/local/bin/sudden-front-server

# ---- runtime
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 game
COPY --from=server /usr/local/bin/sudden-front-server /usr/local/bin/sudden-front-server
COPY --from=client /src/client/dist /app/dist
ENV CLIENT_DIST=/app/dist \
    PORT=8800
EXPOSE 8800
USER game
CMD ["sudden-front-server"]
