#!/bin/sh
# 远程管理接入 entrypoint：将环境变量注入 frpc.toml 模板后启动 frpc。
set -e

TEMPLATE=/template.toml
TARGET=/etc/frp/frpc.toml

mkdir -p /etc/frp

# 用 sed 做变量替换（不依赖 envsubst）
sed \
    -e "s#\${TUN_SERVER}#${TUN_SERVER:-frp.alius.tech}#g" \
    -e "s#\${TUN_PORT}#${TUN_PORT:-7000}#g" \
    -e "s#\${TUN_TOKEN}#${TUN_TOKEN:-}#g" \
    -e "s#\${TUN_LOCAL_ADDR}#${TUN_LOCAL_ADDR:-tokenhub}#g" \
    -e "s#\${TUN_LOCAL_PORT}#${TUN_LOCAL_PORT:-8081}#g" \
    -e "s#\${TUN_REMOTE_PORT}#${TUN_REMOTE_PORT:-28081}#g" \
    -e "s#\${TUN_SUBDOMAIN}#${TUN_SUBDOMAIN}#g" \
    "$TEMPLATE" > "$TARGET"

echo "=== generated frpc.toml ==="
cat "$TARGET"
echo "============================"

exec frpc -c "$TARGET"
