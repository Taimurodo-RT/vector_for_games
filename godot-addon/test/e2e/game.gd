# Автотест экспортированной игры (e2e.sh): герой из walker.tscn, который человек правил в
# Godot, со сценой из Motif. Выходит с кодом 0, если всё на месте. С --shot ПУТЬ ещё сохраняет
# кадр с экрана.
extends Node2D

var failed := 0
var shot_ok := false


func check(ok: bool, what: String) -> void:
	print(("ok   " if ok else "FAIL ") + what)
	if not ok:
		failed += 1


func _ready() -> void:
	RenderingServer.set_default_clear_color(Color(0.1, 0.1, 0.12))
	var w = load("res://chars/walker.tscn").instantiate()
	add_child(w)
	check(w.get_class() == "MotifSprite", "MotifSprite из аддона в экспорте")
	check(w.position == Vector2(160, 120), "позиция из правки .tscn")
	check(w.get_node_or_null("Shadow") != null, "узел, добавленный человеком")
	check(w.side == "up", "сторона из правки .tscn: %s" % w.side)
	check(w.get_inputs() == PackedStringArray(["speed", "run"]), "входы новой сцены: %s" % w.get_inputs())
	var heard := []
	w.event.connect(func(n): heard.append(n))
	w.set_input("speed", 1)
	# Как в sides.expected: машина переходит в Walk на первом шаге, шаги звучат дальше.
	w.advance(0.2)
	w.advance(0.4)
	check(w.get_state() == "Walk", "speed=1 → Walk")
	check(w.get_current_animation() == "Walk_up", "идёт вверх: %s" % w.get_current_animation())
	check(heard.has("step"), "события шагов: %s" % [heard])
	var img: Image = w.get_image(1.0)
	var drawn := 0
	for y in range(0, img.get_height(), 4):
		for x in range(0, img.get_width(), 4):
			drawn += int(img.get_pixel(x, y).a > 0.5)
	check(drawn > 20, "герой нарисован (%d точек)" % drawn)
	var args := OS.get_cmdline_user_args()
	var at := args.find("--shot")
	if at >= 0 and at + 1 < args.size():
		await shot(args[at + 1], w)
	var verdict := "FAILED %d" % failed if failed else "E2E OK"
	if shot_ok:
		verdict += " SHOT OK"
	print(verdict)
	# И в файл рядом с игрой: игра для Windows пишет не в консоль.
	var f := FileAccess.open(OS.get_executable_path().get_base_dir().path_join("e2e.txt"), FileAccess.WRITE)
	if f:
		f.store_line(verdict)
		f.close()
	get_tree().quit(1 if failed else 0)


func shot(path: String, w: Node2D) -> void:
	for i in 5:
		await get_tree().process_frame
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	img.save_png(path)
	var size: Vector2 = w.get_size()
	var lit := 0
	var bg := img.get_pixel(2, 2)
	for y in range(int(w.position.y - size.y / 2), int(w.position.y + size.y / 2), 4):
		for x in range(int(w.position.x - size.x / 2), int(w.position.x + size.x / 2), 4):
			if x >= 0 and y >= 0 and x < img.get_width() and y < img.get_height():
				lit += int(not img.get_pixel(x, y).is_equal_approx(bg))
	check(lit > 20, "герой на экране (%d точек)" % lit)
	if lit > 20:
		print("SHOT OK")
		shot_ok = true
