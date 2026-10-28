#!/bin/sh
# Собирает временный проект Godot с аддоном и примерами Motif и прогоняет test.gd.
# run.sh ПУТЬ_К_GODOT ПУТЬ_К_БИБЛИОТЕКЕ (libmotif_godot.so или motif_godot.dll)
set -e
godot="$1"
lib="$2"
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../.."
p=$(mktemp -d)
cp -r "$here/../addons" "$p/"
mkdir -p "$p/addons/motif/bin" "$p/ui/fonts"
cp "$lib" "$p/addons/motif/bin/"
cp "$root/examples/machine.msc" "$p/hero.msc"
cp "$root/examples/modern-button.msc" "$p/ui/"
cp "$root/examples/fonts/Inter-Medium.ttf" "$p/ui/fonts/"
cp "$here/test.gd" "$p/"
printf 'config_version=5\n\n[application]\n\nconfig/name="motif-test"\n' > "$p/project.godot"
"$godot" --headless --path "$p" --import >/dev/null 2>&1 || true
"$godot" --headless --path "$p" -s test.gd
