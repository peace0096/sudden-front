#!/usr/bin/env bash
# Stop hook for server/AGENTS.md rule 5. When the server's Rust sources have changed since the last
# clean check, run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` in server/. If fmt had
# to rewrite files or clippy fails, the stop is blocked once and Claude is shown why. A second stop
# in a row goes through with a warning to the user, so a check Claude can't fix never traps it.
set -u

input=$(cat)
root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null)}
cd "$root/server" 2>/dev/null || exit 0
command -v cargo >/dev/null 2>&1 || exit 0

# A fingerprint of every Rust source and manifest under server/, committed or not.
sources_hash() {
    find . -path ./target -prune -o -type f \( -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' \) -print0 |
        LC_ALL=C sort -z | xargs -0 git hash-object | git hash-object --stdin
}

stamp=$(git rev-parse --git-path server-check.ok)
[ "$(cat "$stamp" 2>/dev/null)" = "$(sources_hash)" ] && exit 0

reformatted=$(cargo fmt --check -- --files-with-diff 2>/dev/null)
[ -n "$reformatted" ] && cargo fmt
lint=$(cargo clippy --all-targets --quiet --message-format=short -- -D warnings 2>&1)
lint_status=$?

if [ -z "$reformatted" ] && [ "$lint_status" -eq 0 ]; then
    sources_hash >"$stamp"
    exit 0
fi

if printf '%s' "$input" | grep -q '"stop_hook_active"[[:space:]]*:[[:space:]]*true'; then
    echo '{"systemMessage": "Server check still failing: run cargo fmt and cargo clippy --all-targets -- -D warnings in server/ (server/AGENTS.md rule 5)."}'
    exit 0
fi

{
    echo "Server check (server/AGENTS.md rule 5) did not pass. Fix it, then finish again."
    if [ -n "$reformatted" ]; then
        echo
        echo "cargo fmt reformatted these files; review the change and include it in the commit:"
        echo "$reformatted"
    fi
    if [ "$lint_status" -ne 0 ]; then
        echo
        echo "cargo clippy --all-targets -- -D warnings failed:"
        printf '%s\n' "$lint" | grep -E '^(src/|build\.rs|error)' | head -40
    fi
} >&2
exit 2
