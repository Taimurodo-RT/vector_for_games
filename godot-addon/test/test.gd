# Проверка MotifSprite в настоящем Godot (запускает run.sh): машина, события, входы, мышь, картинка.
extends SceneTree

var failed := 0


func check(ok: bool, what: String) -> void:
	print(("ok   " if ok else "FAIL ") + what)
	if not ok:
		failed += 1


func _initialize() -> void:
	var hero = ClassDB.instantiate("MotifSprite")
	hero.scene = "res://hero.msc"
	hero.position = Vector2(320, 200)
	var events := []
	var states := []
	hero.event.connect(func(n): events.append(n))
	var data := {}
	hero.event_data.connect(func(n, d): data[n] = d)
	hero.state_changed.connect(func(s): states.append(s))
	root.add_child(hero)
	check(hero.get_inputs() == PackedStringArray(["speed", "jump", "hurt"]), "входы машины: %s" % hero.get_inputs())
	check(hero.get_state() == "Idle", "начальное состояние Idle")
	check(hero.get_size() == Vector2(480, 320), "размер холста")
	hero.set_input("speed", 1)
	await create_timer(0.6).timeout
	check(hero.get_state() == "Run", "speed=1 → Run")
	hero.fire("jump")
	await create_timer(0.1).timeout
	check(hero.get_state() == "Jump", "jump → Jump")
	await create_timer(0.8).timeout
	check(states.slice(0, 2) == ["Run", "Jump"], "сигнал state_changed: %s" % [states])
	check(events.has("step") and events.has("push"), "сигнал event: %s" % [events])
	check(data.get("push") == {"force": 1.5} and data.get("step") == {}, "сигнал event_data: %s" % [data])
	hero.jump_to("Hit")
	check(hero.get_state() == "Hit", "jump_to")
	hero.restart()
	check(hero.get_state() == "Idle" and hero.get_input("speed") == 0.0, "restart")
	var img: Image = hero.get_image(0.5)
	check(img.get_size() == Vector2i(240, 160), "get_image(0.5): %s" % img.get_size())
	check(img.get_pixel(120, 75).a > 0.9, "капля нарисована")
	# Сцену сохранили в Motif ещё раз, пока игра идёт: состояние и входы остаются.
	hero.set_input("speed", 1)
	await create_timer(0.6).timeout
	var text := FileAccess.get_file_as_string("res://hero.msc")
	var f := FileAccess.open("res://hero.msc", FileAccess.WRITE)
	f.store_string(text.replace("artboard", "# правка\nartboard"))
	f.close()
	check(hero.hot_reload() and hero.get_state() == "Run" and hero.get_input("speed") == 1.0, "hot_reload: играет дальше из Run")
	f = FileAccess.open("res://hero.msc", FileAccess.WRITE)
	f.store_string(text + "\nmachine Hero {\n")
	f.close()
	check(not hero.hot_reload() and hero.get_state() == "Run", "hot_reload с ошибкой: старая сцена играет дальше")
	f = FileAccess.open("res://hero.msc", FileAccess.WRITE)
	f.store_string(text)
	f.close()

	var button = ClassDB.instantiate("MotifSprite")
	button.scene = "res://ui/modern-button.msc"
	root.add_child(button)
	check(button.get_state() == "Normal", "кнопка из другого файла, шрифт рядом с ней")
	var bad = ClassDB.instantiate("MotifSprite")
	bad.scene = "res://hero.msc"
	bad.animation = "Fly"
	root.add_child(bad)
	check(bad.get_state() == "", "нет анимации — ошибка, а не падение")
	bad.animation = "Run"
	check(bad.get_state() == "Run", "одна анимация без машины")
	sides()
	print("FAILED %d" % failed if failed else "ALL OK")
	quit(1 if failed else 0)


# Стороны: тот же сценарий sides.txt, что в тесте ядра, и тот же ожидаемый журнал sides.expected.
func sides() -> void:
	var w = ClassDB.instantiate("MotifSprite")
	w.scene = "res://walker.msc"
	w.machine = "Walker"
	w.playing = false
	root.add_child(w)
	var heard := []
	var turns := []
	w.event.connect(func(n): heard.append(n))
	w.side_changed.connect(func(s): turns.append(s))
	check(w.get_sides().size() == 8 and w.side == "down", "стороны сцены: %s, сейчас %s" % [w.get_sides(), w.side])
	var log := ""
	for raw in FileAccess.get_file_as_string("res://sides.txt").split("\n"):
		var line := raw.strip_edges()
		if line == "" or line.begins_with("#"):
			continue
		var words := line.split(" ", false)
		heard.clear()
		match words[0]:
			"advance": w.advance(float(words[1]))
			"side": w.side = words[1]
			"facing": w.facing = Vector2(float(words[1]), float(words[2]))
			"set": w.set_input(words[1], float(words[2]))
		var flip := " flipped" if w.is_flipped() else ""
		log += "%s | %s %s %s%s t=%.3f events=[%s]\n" % [line, w.get_state(), w.get_current_animation(), w.side, flip, w.get_time(), ", ".join(heard)]
	var expected := FileAccess.get_file_as_string("res://sides.expected")
	check(log == expected, "журнал сторон совпадает с ядром")
	if log != expected:
		print(log)
	check(turns == ["right", "up", "left", "up_left", "down", "left"], "сигнал side_changed: %s" % [turns])
	check(w.facing.is_equal_approx(Vector2(-1, 0)), "facing следует за side: %s" % w.facing)
	var img: Image = w.get_image(0.5)
	w.side = "right"
	var right: Image = w.get_image(0.5)
	var mirrored := 0
	var differ := 0
	for y in range(0, 120, 3):
		for x in range(120):
			mirrored += int(img.get_pixel(x, y) != right.get_pixel(119 - x, y))
			differ += int(img.get_pixel(x, y) != right.get_pixel(x, y))
	check(mirrored == 0 and differ > 0, "левая сторона — правая, отражённая в рисунке (%d, %d)" % [mirrored, differ])
