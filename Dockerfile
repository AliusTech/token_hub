# syntax=docker/dockerfile:1
# 多阶段构建。

FROM rust:1.96-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin token_hub

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget curl && \
    rm -rf /var/lib/apt/lists/*

# 安装 frpc（用于 tun 通道）
RUN arch=$(dpkg --print-architecture) && \
    case "$arch" in \
        amd64) frpc_arch="amd64" ;; \
        arm64) frpc_arch="arm64" ;; \
        *) echo "unsupported arch: $arch" && exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/fatedier/frp/releases/download/v0.61.1/frp_0.61.1_linux_${frpc_arch}.tar.gz" \
    -o /tmp/frp.tar.gz && \
    tar -xzf /tmp/frp.tar.gz -C /tmp && \
    cp /tmp/frp_*/frpc /usr/local/bin/frpc && \
    chmod +x /usr/local/bin/frpc && \
    rm -rf /tmp/frp*

WORKDIR /app
RUN useradd -r -u 10001 -m tokenhub && \
    mkdir -p /data /home/tokenhub/.tokenhub && \
    chown -R tokenhub:tokenhub /data /home/tokenhub
COPY --from=builder /app/target/release/token_hub /usr/local/bin/token_hub

USER tokenhub
ENV DATABASE_URL=sqlite:///data/tokenhub.db \
    REDIS_URL=memory \
    CHAT_LISTEN=0.0.0.0:8080 \
    ADMIN_LISTEN=0.0.0.0:8081 \
    FRPC_BINARY=/usr/local/bin/frpc
VOLUME ["/data"]
EXPOSE 8080 8081
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -qO- http://localhost:8080/healthz || exit 1
ENTRYPOINT ["token_hub"]
