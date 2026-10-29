//! Exercise the public executable: diagnostics must stay machine-readable and read-only.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Lab {
    root: PathBuf,
    scene: PathBuf,
    source: String,
}
impl Lab {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!("motif-inspect-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let scene = root.join("лицо.msc");
        std::fs::write(&scene, source).unwrap();
        Self { root, scene, source: source.into() }
    }
    fn inspect(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_motif")).arg("inspect").arg(&self.scene).args(args).output().unwrap()
    }
    fn read(&self, args: &[&str]) -> Value {
        let r = self.inspect(args);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert!(r.stderr.is_empty());
        serde_json::from_slice(&r.stdout).expect("stdout contains only JSON")
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        assert_eq!(std::fs::read_to_string(&self.scene).unwrap(), self.source, "inspect changed the scene");
        assert!(self.root.is_absolute() && self.root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
const SCENE: &str = "artboard \"Test\" 200x200\ngroup head at 100 100 deform 120 120 {\npath face d \"M -30 -30 L 30 -30 L 0 40 Z\" forms \"M -35 -30 L 35 -30 L 0 45 Z\" fill #ffeedd\n}\nrect other size 10\nanim A 1s {\nhead.warp-x4 0: 0 linear | 1: 40\nface.contour 0: 0 linear | 1: 1\nother.x 0: 0 linear | 1: 50\n}\n";
fn node<'a>(report: &'a Value, pose: &str, id: &str) -> &'a Value {
    report["boards"][0][pose]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == id).unwrap()
}
#[test]
fn stdout_reports_source_and_actual_posed_geometry_without_changing_source() {
    let lab = Lab::new(SCENE);
    let report = lab.read(&["--anim", "A", "--time", "0.5"]);
    assert_eq!(report["schema"], 1);
    assert_eq!(node(&report, "base", "head")["properties"]["warp-x4"], 0.0);
    assert_eq!(node(&report, "posed", "head")["properties"]["warp-x4"], 20.0);
    assert_eq!(node(&report, "posed", "other")["properties"]["x"], 25.0);
    let base = node(&report, "base", "face");
    let posed = node(&report, "posed", "face");
    assert_eq!(base["geometry"]["source"], posed["geometry"]["source"]);
    assert_ne!(base["geometry"]["authorable"], posed["geometry"]["authorable"]);
    assert_ne!(posed["geometry"]["rendered"], posed["geometry"]["authorable"]);
    assert_eq!(posed["parent"], "head");
    assert_eq!(posed["world_transform"], json!([1.0, 0.0, 0.0, 1.0, 100.0, 100.0]));
    assert_eq!(report["boards"][0]["clips"][0]["tracks"][0]["keys"][0]["ease"], "linear");
}
#[test]
fn node_filter_keeps_descendants_and_context_and_output_file_has_no_stdout() {
    let lab = Lab::new(SCENE);
    let output = lab.root.join("reports").join("nested").join("inspect.json");
    let r = lab.inspect(&["--node", "head", "-o", output.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(r.stdout.is_empty());
    assert!(r.stderr.is_empty());
    let report: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    let ids: Vec<_> = report["boards"][0]["base"]["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["head", "face"]);
    assert_eq!(report["boards"][0]["clips"][0]["tracks"][2]["node"], "other");
}
#[test]
fn board_local_utf8_spans_map_back_to_the_original_multiboard_file() {
    let lab = Lab::new("let цвет = #ffccaa\nartboard \"Первый\" 100x100\nrect a fill цвет\nartboard \"Второй\" 100x100\nrect b \"Лицо\" at 42 24 fill цвет\n");
    let report = lab.read(&["--board", "2", "--node", "b"]);
    let b = &report["boards"][0];
    assert_eq!(b["name"], "Второй");
    let src = b["source"]["text"].as_str().unwrap();
    let prop = &node(&report, "base", "b")["source"]["properties"].as_array().unwrap().iter().find(|p| p["property"] == "x").unwrap();
    let start = prop["span"]["start"].as_u64().unwrap() as usize;
    let end = prop["span"]["end"].as_u64().unwrap() as usize;
    assert_eq!(&src[start..end], "42");
    let segment = b["source"]["original_segments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            let a = s["local_start"].as_u64().unwrap() as usize;
            let n = s["length"].as_u64().unwrap() as usize;
            (a..a + n).contains(&start)
        })
        .unwrap();
    let file_start = segment["file_start"].as_u64().unwrap() as usize + start - segment["local_start"].as_u64().unwrap() as usize;
    assert_eq!(&lab.source[file_start..file_start + end - start], "42");
}
#[test]
fn invalid_selections_fail_with_empty_stdout_and_cannot_overwrite_the_scene() {
    let lab = Lab::new(SCENE);
    for args in [
        vec!["--node", "missing"],
        vec!["--board", "missing"],
        vec!["--anim", "missing"],
        vec!["--time", "NaN"],
        vec!["--time", "-1"],
        vec!["-o", lab.scene.to_str().unwrap()],
    ] {
        let r = lab.inspect(&args);
        assert!(!r.status.success(), "{args:?}");
        assert!(r.stdout.is_empty());
        assert!(!r.stderr.is_empty());
    }
}

#[test]
fn directional_inspection_resolves_side_specific_clips_before_posing() {
    let lab = Lab::new(
        "artboard 200x200\ndirections 4 turn body\nellipse body size 50\nrect marker size 10\nanim Walk 1s {\nmarker.x 0: 10\n}\nanim Walk_up 1s {\nmarker.x 0: 80\n}\n",
    );
    let up = lab.read(&["--anim", "Walk", "--side", "up", "--time", "0.5"]);
    assert_eq!(up["boards"][0]["pose_selection"]["resolved_clip"], "Walk_up");
    assert_eq!(node(&up, "posed", "marker")["properties"]["x"], 80.0);
    let down = lab.read(&["--anim", "Walk", "--side", "down"]);
    assert_eq!(down["boards"][0]["pose_selection"]["resolved_clip"], "Walk");
    assert_eq!(node(&down, "posed", "marker")["properties"]["x"], 10.0);
    assert_eq!(node(&up, "base", "marker")["properties"]["x"], 0.0);
}
