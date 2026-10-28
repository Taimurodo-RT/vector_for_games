# Управление каплей: входы машины состояний из hero.msc меняются с клавиатуры, а её события
# (шаг, толчок, приземление) и смена состояний пишутся в углу — так игра узнаёт, когда играть звук.
extends Node2D

@onready var hero: Node2D = $Hero
@onready var camera: Camera2D = $Camera
@onready var log_label: Label = $UI/Log
var lines: Array[String] = []


func _ready() -> void:
	hero.event.connect(func(name: String): say("событие " + name))
	hero.state_changed.connect(func(state: String): say("→ " + state))


func _process(_delta: float) -> void:
	var dir := Input.get_axis("ui_left", "ui_right")
	hero.set_input("speed", absf(dir))
	if dir != 0.0:
		hero.scale.x = signf(dir) * absf(hero.scale.x)


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("ui_accept"):
		hero.fire("jump")
	elif event is InputEventKey and event.pressed and not event.echo and event.keycode == KEY_H:
		hero.fire("hurt")
	elif event is InputEventMouseButton and event.pressed:
		var zoom := camera.zoom.x
		if event.button_index == MOUSE_BUTTON_WHEEL_UP:
			zoom = minf(zoom * 1.15, 8.0)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			zoom = maxf(zoom / 1.15, 0.3)
		camera.zoom = Vector2(zoom, zoom)


func say(line: String) -> void:
	lines.append(line)
	if lines.size() > 6:
		lines.pop_front()
	log_label.text = "\n".join(lines)
