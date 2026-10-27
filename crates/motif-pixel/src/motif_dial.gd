@tool
extends Range
## A slider drawn in Motif: its Value animation baked into frames, and the value picks the
## frame. Written by Motif («Экспорт → Экран для Godot…», `motif screen` or `motif kit`).
## Listen to `value_changed` as with any slider.

## The frames side by side, the value from 0 to 1; one row per look.
@export var frames: Texture2D:
	set(v):
		frames = v
		queue_redraw()
## How many frames a row has.
@export var columns := 1
## One frame, in pixels.
@export var cell := Vector2.ZERO
## The row each look draws: normal, hover, pressed, disabled.
@export var rows := PackedInt32Array([0, 0, 0, 0])
## Where a frame starts from the control's corner: glows and shadows reach outside it.
@export var offset := Vector2.ZERO
## Where the pointer moves it, from the control's corner. Empty: the whole control.
@export var track := Rect2()
## How the pointer moves it: sideways, up, or around the middle of the track.
@export_enum("Across", "Up", "Turn") var drag := 0
## Turning: the arc from the lowest value to the highest, in degrees.
@export var arc := 270.0
## Off: the disabled look, and the pointer does nothing.
@export var editable := true:
	set(v):
		editable = v
		queue_redraw()

var _hover := false
var _held := false
var _angle := 0.0
var _turned := 0.0


func _ready() -> void:
	mouse_entered.connect(_set_hover.bind(true))
	mouse_exited.connect(_set_hover.bind(false))
	value_changed.connect(func(_v: float) -> void: queue_redraw())
	changed.connect(queue_redraw)


func _set_hover(on: bool) -> void:
	_hover = on
	queue_redraw()


func _draw() -> void:
	if frames == null or columns < 1 or cell == Vector2.ZERO:
		return
	var look := 0
	if not editable:
		look = 3
	elif _held:
		look = 2
	elif _hover:
		look = 1
	var frame := clampi(roundi(ratio * (columns - 1)), 0, columns - 1)
	var at := Vector2(frame * cell.x, rows[look] * cell.y)
	draw_texture_rect_region(frames, Rect2(offset, cell), Rect2(at, cell))


func _area() -> Rect2:
	return track if track.has_area() else Rect2(Vector2.ZERO, size)


## The angle of `p` around the middle of the track, clockwise from the top, in degrees.
func _angle_at(p: Vector2) -> float:
	var v := p - _area().get_center()
	return rad_to_deg(atan2(v.x, -v.y))


func _gui_input(event: InputEvent) -> void:
	if not editable:
		return
	var button := event as InputEventMouseButton
	if button and button.button_index == MOUSE_BUTTON_LEFT:
		_held = button.pressed
		if _held:
			_angle = _angle_at(button.position)
			_turned = ratio
			if drag != 2:
				_follow(button.position)
		queue_redraw()
		accept_event()
		return
	var motion := event as InputEventMouseMotion
	if motion and _held:
		if drag == 2:
			# The shorter way round from the last angle, so a turn across the bottom goes on.
			var a := _angle_at(motion.position)
			_turned = clampf(_turned + wrapf(a - _angle, -180.0, 180.0) / arc, 0.0, 1.0)
			_angle = a
			ratio = _turned
		else:
			_follow(motion.position)
		accept_event()


func _follow(p: Vector2) -> void:
	var r := _area()
	if drag == 1:
		ratio = clampf((r.end.y - p.y) / r.size.y, 0.0, 1.0)
	else:
		ratio = clampf((p.x - r.position.x) / r.size.x, 0.0, 1.0)
