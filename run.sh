#!/usr/bin/env bash
# Builds the client and the server, then starts the server, which serves the built client.
# Usage: ./run.sh                 (port 8800, or the next free one the server picks and prints)
#        PORT=8810 ./run.sh       (that port only; the server stops if it is taken)
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"

for tool in npm cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "!! $tool 이(가) 없습니다. Node.js 22.12 이상과 Rust 1.89 이상이 필요합니다." >&2
    exit 1
  fi
done

echo "==> 클라이언트 빌드 (client/)"
cd "$root/client"
# Packages are installed from the lock file on a fresh checkout, and again when the lock file is
# newer than the last install (npm keeps its own copy in node_modules/.package-lock.json).
if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
  npm ci
fi
npm run build

echo "==> 서버 빌드 (server/)"
cd "$root/server"
cargo build --release

# A server left running on the port keeps serving the old build there.
port="${PORT:-8800}"
if command -v lsof >/dev/null 2>&1; then
  running="$(lsof -ti "tcp:$port" -sTCP:LISTEN 2>/dev/null | tr '\n' ' ' || true)"
  if [ -n "$running" ]; then
    echo "!! 포트 $port 를 이미 다른 프로세스(PID $running)가 쓰고 있습니다. 이전 서버라면 종료하세요: kill $running" >&2
  fi
fi

# The address other players on the same network open, with the port the server prints below.
ip="$( (ipconfig getifaddr en0 || ipconfig getifaddr en1 || hostname -I | awk '{print $1}') 2>/dev/null || true)"
if [ -n "$ip" ]; then
  echo "==> 같은 네트워크의 다른 플레이어는 http://$ip:<포트> 로 접속합니다 (포트는 아래 서버 출력 참고)"
fi

echo "==> 서버 시작 (종료: Ctrl+C)"
exec "$root/server/target/release/sudden-front-server"
