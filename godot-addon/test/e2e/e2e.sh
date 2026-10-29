#!/bin/bash
# Сквозная поставка, как у человека: Motif ставит сцену в новый проект Godot, человек правит
# .tscn в Godot, сцена меняется и ставится снова, проект экспортируется шаблонами 4.3, игра
# переносится в чужую папку и запускается там сама: её автотест (game.gd) выходит с кодом 0.
# e2e.sh ПУТЬ_К_GODOT ПУТЬ_К_MOTIF ПАПКА_АДДОНА ШАБЛОНЫ.tpz [СНИМОК.png]
# ПАПКА_АДДОНА — addons/motif с библиотекой этой системы в bin. СНИМОК — запустить игру ещё раз
# с окном (нужен экран, например Xvfb) и сохранить кадр.
set -euo pipefail
godot="$1"
motif="$2"
addon="$3"
tpz="$4"
shot="${5:-}"
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../../.."
fail() { echo "FAIL $*"; exit 1; }
ok() { echo "ok   $*"; }

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) platform="Windows Desktop"; template=windows_release_x86_64.exe; game=game.exe
    templates="$(cygpath -u "$APPDATA")/Godot/export_templates/4.3.stable" ;;
  *) platform="Linux"; template=linux_release.x86_64; game=game.x86_64
    templates="${XDG_DATA_HOME:-$HOME/.local/share}/godot/export_templates/4.3.stable" ;;
esac
if [ ! -f "$templates/$template" ]; then
  mkdir -p "$templates"
  t=$(mktemp -d)
  if command -v unzip >/dev/null; then
    unzip -q -o "$tpz" "templates/$template" "templates/version.txt" -d "$t"
  else
    7z x -y -bd "-o$t" "$tpz" "templates/$template" "templates/version.txt" >/dev/null
  fi
  mv "$t/templates/"* "$templates/"
fi

work=$(mktemp -d)
p="$work/project"
mkdir -p "$p/chars" "$work/art"
# Проект, только что созданный в Godot 4.3, с главной сценой-автотестом.
cat > "$p/project.godot" <<'P'
config_version=5

[application]

config/name="motif-e2e"
run/main_scene="res://main.tscn"
config/features=PackedStringArray("4.3")
P
cat > "$p/export_presets.cfg" <<P
[preset.0]

name="Game"
platform="$platform"
runnable=true
export_filter="all_resources"
include_filter=""
exclude_filter=""
export_path=""

[preset.0.options]

binary_format/embed_pck=false
P
cp "$here/game.gd" "$here/main.tscn" "$p/"

# 1. Первая установка: аддон, сцена, .tscn с узлом и *.msc в фильтре экспорта.
cp "$root/examples/directions.msc" "$work/art/walker.msc"
out=$("$motif" runtime "$work/art/walker.msc" -o "$p/chars" --addon "$addon")
echo "$out"
grep -q "аддон поставлен" <<<"$out" || fail "первая установка ставит аддон"
grep -q 'include_filter=".*\*\.msc' "$p/export_presets.cfg" || fail "*.msc в фильтре экспорта"
[ -f "$p/chars/walker.tscn" ] || fail "walker.tscn создан"
ok "первая установка"

# 2. Человек правит walker.tscn в Godot: поворачивает героя вверх, двигает, добавляет узел.
cat > "$p/chars/walker.tscn" <<'T'
[gd_scene format=3]

[node name="walker" type="MotifSprite"]
position = Vector2(160, 120)
scene = "res://chars/walker.msc"
facing = Vector2(0, -1)

[node name="Shadow" type="Node2D" parent="."]
T
cp "$p/chars/walker.tscn" "$work/edited.tscn"

# 3. Сцена в Motif поменялась (новый вход машины) и ставится снова.
sed -i 's/^  input speed 0/&\n  input run 0/' "$work/art/walker.msc"
grep -q "input run 0" "$work/art/walker.msc" || fail "правка сцены"
out=$("$motif" runtime "$work/art/walker.msc" -o "$p/chars" --addon "$addon")
echo "$out"
grep -q "аддон уже стоит" <<<"$out" || fail "второй раз аддон не меняется"
cmp -s "$p/chars/walker.tscn" "$work/edited.tscn" || fail "правка .tscn пережила переустановку"
grep -q "input run 0" "$p/chars/walker.msc" || fail "новая сцена в проекте"
[ -z "$(find "$p" -name '.*.part' -o -name '.*.old')" ] || fail "после установки не осталось .part и .old"
ok "переустановка: .tscn человека цел, сцена новая"

# 4. Экспорт шаблонами 4.3.
"$godot" --headless --path "$p" --import >/dev/null 2>&1 || true
mkdir -p "$work/build"
"$godot" --headless --path "$p" --export-release "Game" "$work/build/$game" 2>&1 | tail -5
[ -f "$work/build/$game" ] || fail "экспорт: нет $game"
ls "$work/build"
ok "экспорт"

# 5. Игра в чужой папке, без проекта рядом.
far=$(mktemp -d)
mv "$work/build" "$far/Моя игра"
rm -rf "$p"
cd "$far/Моя игра"
set +e
"./$game" --headless > log.txt 2>&1
code=$?
set -e
cat log.txt
[ $code -eq 0 ] || fail "игра вышла с кодом $code"
grep -q "E2E OK" e2e.txt || fail "автотест игры не прошёл"
ok "игра из чужой папки: код 0"

if [ -n "$shot" ]; then
  "./$game" --rendering-driver opengl3 -- --shot "$shot" > shot.txt 2>&1 || { cat shot.txt; fail "игра с окном"; }
  [ -f "$shot" ] || { cat shot.txt; fail "кадр не сохранён"; }
  grep -q "SHOT OK" e2e.txt || { cat shot.txt; fail "на кадре нет героя"; }
  ok "кадр с окна: $shot"
fi
echo "E2E ALL OK"
