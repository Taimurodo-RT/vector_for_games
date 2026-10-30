extends SceneTree
# -- N res://сцена.msc: N узлов MotifSprite играют сцену, их машина идёт (speed = 1).
# Меряется работа кадра: от начала обновления (process_frame) до конца рисования
# узлов — отложенного вызова после всех _process, он идёт за кадрами узлов (draw).
# Сон между кадрами, которым Godot без экрана держит 144 кадра в секунду, в замер
# не входит. Первые 100 кадров — разгон, затем 300 кадров замера.

const WARM := 100
const FRAMES := 300
var times := []
var start := 0

# Обновляется последним и ставит конец замера в очередь за рисованием узлов.
class Tail extends Node:
	var done: Callable
	func _process(_delta):
		done.call_deferred()

func _initialize():
	var args := OS.get_cmdline_user_args()
	var n := int(args[0])
	for i in n:
		var w = ClassDB.instantiate("MotifSprite")
		if w == null:
			print("bench: нет узла MotifSprite — библиотека аддона не загрузилась")
			quit(1)
			return
		w.scene = args[1]
		w.position = Vector2(100 + (i % 5) * 200, 100 + (i / 5) * 200)
		root.add_child(w)
		if w.get_inputs().has("speed"):
			w.set_input("speed", 1)
	process_frame.connect(func(): start = Time.get_ticks_usec())
	var tail := Tail.new()
	tail.done = _drawn
	tail.process_priority = 1 << 20
	root.add_child.call_deferred(tail)

func _drawn():
	if start == 0:
		return
	times.append(Time.get_ticks_usec() - start)
	if times.size() == WARM + FRAMES:
		times = times.slice(WARM)
		times.sort()
		var sum := 0
		for t in times:
			sum += t
		var mean := sum / float(FRAMES) / 1000.0
		var p95: float = times[int(FRAMES * 0.95)] / 1000.0
		print("bench n=%s mean=%.2fms p95=%.2fms доля кадра 60fps=%d%% память=%s" % [OS.get_cmdline_user_args()[0], mean, p95, round(mean / 16.667 * 100), memory()])
		quit()

# Память всего процесса (Godot и библиотеки Motif), где система её сообщает.
func memory() -> String:
	var f := FileAccess.open("/proc/self/status", FileAccess.READ)
	while f and not f.eof_reached():
		var line := f.get_line()
		if line.begins_with("VmRSS:"):
			return "%d МБ" % (int(line.split(":")[1].strip_edges().split(" ")[0]) / 1024)
	return "%d МБ (только Godot)" % (OS.get_static_memory_usage() / 1048576)
