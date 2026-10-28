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
	hero.jump_to("Hit")
	check(hero.get_state() == "Hit", "jump_to")
	hero.restart()
	check(hero.get_state() == "Idle" and hero.get_input("speed") == 0.0, "restart")
	var img: Image = hero.get_image(0.5)
	check(img.get_size() == Vector2i(240, 160), "get_image(0.5): %s" % img.get_size())
	check(img.get_pixel(120, 75).a > 0.9, "капля нарисована")

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
	print("FAILED %d" % failed if failed else "ALL OK")
	quit(1 if failed else 0)
