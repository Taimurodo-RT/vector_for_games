extends Control
## Калькулятор. Весь интерфейс — сцена ui/calculator_screen.tscn, собранная из calculator.msc
## командой motif screen: здесь только логика. Клавиши находятся по id из скрипта Motif
## (k0…k9, kadd, keq…), табло — по id текстов expr и result.

const DIGITS := 12

@onready var ui: Control = $UI
var expr_label: Label
var result_label: Label
var result_size: int

var entry := "0"          # что набирается сейчас
var stored := 0.0         # левый операнд
var pending := ""         # ожидающая операция: + − × ÷
var fresh := true         # следующая цифра начинает новое число
var history := ""

func _ready() -> void:
	expr_label = ui.find_child("expr") as Label
	result_label = ui.find_child("result") as Label
	result_size = result_label.get_theme_font_size("font_size")
	for b in ui.find_children("*", "Button"):
		b.pressed.connect(_on_key.bind(String(b.name)))
		b.focus_mode = Control.FOCUS_NONE
	_show()

func _on_key(id: String) -> void:
	match id:
		"kac": _clear()
		"ksign": _sign()
		"kpct": _percent()
		"kdot": _dot()
		"kadd": _operator("+")
		"ksub": _operator("−")
		"kmul": _operator("×")
		"kdiv": _operator("÷")
		"keq": _equals()
		_:
			if id.begins_with("k") and id.length() == 2:
				_digit(id.substr(1))
	_show()

func _unhandled_key_input(event: InputEvent) -> void:
	var k := event as InputEventKey
	if k == null or not k.pressed:
		return
	var ch := char(k.unicode) if k.unicode > 0 else ""
	var id := ""
	if ch >= "0" and ch <= "9" and ch.length() == 1:
		id = "k" + ch
	else:
		match ch:
			"+": id = "kadd"
			"-": id = "ksub"
			"*", "x": id = "kmul"
			"/": id = "kdiv"
			".", ",": id = "kdot"
			"%": id = "kpct"
			"=": id = "keq"
	match k.keycode:
		KEY_ENTER, KEY_KP_ENTER: id = "keq"
		KEY_ESCAPE, KEY_DELETE: id = "kac"
		KEY_BACKSPACE:
			_backspace()
			_show()
			return
	if id != "":
		_press(id)

## Нажатие с клавиатуры: та же клавиша на экране мигает своим состоянием Press.
func _press(id: String) -> void:
	var b := ui.find_child(id) as Button
	if b:
		b.emit_signal("pressed")
		var t := create_tween()
		b.add_theme_stylebox_override("normal", b.get_theme_stylebox("pressed"))
		t.tween_interval(0.08)
		t.tween_callback(func(): b.remove_theme_stylebox_override("normal"))

func _digit(d: String) -> void:
	if fresh:
		entry = d
		fresh = false
	elif entry.replace(",", "").replace("-", "").length() < DIGITS:
		entry = d if entry == "0" else entry + d

func _dot() -> void:
	if fresh:
		entry = "0,"
		fresh = false
	elif not entry.contains(","):
		entry += ","

func _backspace() -> void:
	if fresh:
		return
	entry = entry.left(entry.length() - 1)
	if entry == "" or entry == "-":
		entry = "0"

func _sign() -> void:
	if entry == "0":
		return
	entry = entry.substr(1) if entry.begins_with("-") else "-" + entry

func _percent() -> void:
	var v := _value()
	# 200 + 10 % — это десять процентов от 200, как на обычном калькуляторе.
	v = stored * v / 100.0 if pending in ["+", "−"] else v / 100.0
	entry = _format(v)
	fresh = true

func _operator(op: String) -> void:
	if pending != "" and not fresh:
		stored = _apply(stored, _value(), pending)
		entry = _format(stored)
	else:
		stored = _value()
	pending = op
	history = "%s %s" % [_format(stored), op]
	fresh = true

func _equals() -> void:
	if pending == "":
		return
	var right := _value()
	history = "%s %s %s =" % [_format(stored), pending, _format(right)]
	stored = _apply(stored, right, pending)
	entry = _format(stored)
	pending = ""
	fresh = true

func _clear() -> void:
	entry = "0"
	stored = 0.0
	pending = ""
	history = ""
	fresh = true

func _apply(a: float, b: float, op: String) -> float:
	match op:
		"+": return a + b
		"−": return a - b
		"×": return a * b
		"÷": return a / b if b != 0.0 else NAN
	return b

func _value() -> float:
	return entry.replace(",", ".").to_float()

func _format(v: float) -> String:
	if is_nan(v) or is_inf(v):
		return "Ошибка"
	var s := String.num(v, 10)
	if s.contains("."):
		s = s.rstrip("0").rstrip(".")
	if s.length() > DIGITS + 2:
		s = "%.6e" % v
	return s.replace(".", ",")

func _show() -> void:
	if entry == "Ошибка":
		stored = 0.0
	expr_label.text = history
	result_label.text = entry
	# Длинное число уменьшается, чтобы влезть в табло.
	var extra := maxi(0, entry.length() - 9)
	result_label.add_theme_font_size_override("font_size", maxi(20, result_size - extra * 4))
