#!/bin/sh
# Замер скорости MotifSprite в настоящем Godot: N персонажей на экране, работа кадра.
# run.sh ПУТЬ_К_GODOT ПУТЬ_К_БИБЛИОТЕКЕ [N ...]
# Библиотеку берите из release-сборки: cargo build --release -p motif-godot.
# Для каждой сцены и каждого N печатает среднее и p95 работы кадра, её долю в кадре
# 60 fps и память процесса.
set -e
godot="$1"
lib="$2"
shift 2
counts="${*:-1 20}"
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../../.."
p=$(mktemp -d)
cp -r "$here/../../addons" "$p/"
mkdir -p "$p/addons/motif/bin"
# Под тем именем, которое ждёт motif.gdextension, как бы ни назывался файл.
case "$lib" in
  *.dll) name=motif_godot.dll ;;
  *.dylib) name=libmotif_godot.dylib ;;
  *) name=libmotif_godot.so ;;
esac
cp "$lib" "$p/addons/motif/bin/$name"
for s in directions character-platformer character-anime; do cp "$root/examples/$s.msc" "$p/"; done
cp "$here/bench.gd" "$p/"
printf 'config_version=5\n\n[application]\n\nconfig/name="motif-bench"\n' > "$p/project.godot"
"$godot" --headless --path "$p" --import >/dev/null 2>&1 || true
run() { "$godot" --headless --path "$p" -s bench.gd -- "$@" 2>&1 | grep '^bench'; }
run 0 res://directions.msc | sed 's/^bench /пустая сцена: /'
for s in directions character-platformer character-anime; do
  for n in $counts; do
    run "$n" "res://$s.msc" | sed "s/^bench /$s: /"
  done
done
rm -rf "$p"
