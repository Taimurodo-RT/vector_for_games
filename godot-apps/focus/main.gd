extends Control
## Таймер фокуса. Интерфейс — сцена ui/focus_screen.tscn из focus.msc (motif screen),
## здесь только логика: узлы находятся по id из скрипта Motif.

@onready var ui: Control = $UI
var work_button: Button
var rest_button: Button
var start_button: Button
var reset_button: Button
var time_label: Label
var phase_label: Label
var progress: ProgressBar
var minutes: HSlider
var minutes_label: Label
var auto_rest: CheckBox

const REST_MINUTES := 5
var resting := false
var running := false
var total := 25.0 * 60.0
var left := total

func _ready() -> void:
	work_button = _find("work")
	rest_button = _find("rest")
	start_button = _find("start")
	reset_button = _find("reset")
	time_label = _find("time")
	phase_label = _find("phase")
	progress = _find("progress")
	minutes = _find("minutes")
	minutes_label = _find("minutes-value")
	auto_rest = _find("auto")
	# Режимы — переключатели одной группы: выбранный рисуется состоянием Press из Motif.
	var modes := ButtonGroup.new()
	for b in [work_button, rest_button]:
		b.toggle_mode = true
		b.button_group = modes
	work_button.button_pressed = true
	work_button.pressed.connect(_set_mode.bind(false))
	rest_button.pressed.connect(_set_mode.bind(true))
	start_button.pressed.connect(_toggle)
	reset_button.pressed.connect(_reset)
	minutes.min_value = 5
	minutes.max_value = 60
	minutes.step = 5
	minutes.value = 25
	minutes.value_changed.connect(func(_v): if not resting: _reset())
	progress.max_value = 1.0
	progress.step = 0.0
	_reset()

func _find(id: String) -> Node:
	var n := ui.find_child(id)
	assert(n != null, "в ui/focus_screen.tscn нет узла " + id)
	return n

func _process(delta: float) -> void:
	if not running:
		return
	left = maxf(0.0, left - delta)
	_show()
	if left <= 0.0:
		_finished()

func _toggle() -> void:
	running = not running
	start_button.text = "Пауза" if running else "Дальше"
	_show()

func _reset() -> void:
	running = false
	total = (REST_MINUTES if resting else minutes.value) * 60.0
	left = total
	start_button.text = "Старт"
	_show()

func _set_mode(rest: bool) -> void:
	resting = rest
	_reset()

func _finished() -> void:
	running = false
	DisplayServer.window_request_attention()
	var was_rest := resting
	resting = not resting
	(rest_button if resting else work_button).button_pressed = true
	_reset()
	# После работы отдых может начаться сам.
	if not was_rest and auto_rest.button_pressed:
		_toggle()

func _show() -> void:
	var s := int(ceil(left))
	time_label.text = "%02d:%02d" % [s / 60, s % 60]
	if running:
		phase_label.text = "до конца отдыха" if resting else "до отдыха"
	else:
		phase_label.text = "отдых на паузе" if resting and left < total else ("пауза" if left < total else ("отдых" if resting else "готов к работе"))
	progress.value = 1.0 - left / total
	minutes_label.text = str(int(minutes.value))
