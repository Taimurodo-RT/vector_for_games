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
# Капля (hero) играет вектором через аддон Motif: аддон и его библиотека, собранная cargo.
case "$(uname)" in
  Darwin) lib=libmotif_godot.dylib ;;
  *) lib=libmotif_godot.so ;;
esac
(cd .. && cargo build -q --release -p motif-godot)
rm -rf hero/addons/motif
mkdir -p hero/addons
cp -r ../godot-addon/addons/motif hero/addons/
mkdir -p hero/addons/motif/bin
cp "../target/release/$lib" hero/addons/motif/bin/
