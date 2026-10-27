#!/bin/sh
# Пересобирает интерфейс приложений из их .msc: каждое app/app.msc → app/ui/*.tscn и тема.
# Motif берётся из $MOTIF, иначе из PATH, иначе собирается cargo из этого репозитория.
set -e
cd "$(dirname "$0")"
if [ -n "$MOTIF" ]; then run="$MOTIF"
elif command -v motif >/dev/null 2>&1; then run=motif
else run="cargo run -q --release -p motif-cli --"
fi
for app in calculator focus; do
  rm -rf "$app/ui"
  $run screen "$app/$app.msc" -o "$app/ui"
done
