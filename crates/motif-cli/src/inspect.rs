//! Read-only, versioned scene diagnostics for tools and regression investigations.
use super::Args;
use motif_core::kurbo::{Affine, BezPath, PathEl, Rect, Shape};
use motif_core::{Document, Gradient, Node, Prop, Shadow, Track, Value};
use motif_script::{Scene, Span};
use serde_json::{Value as Json, json};
use std::io::Write;

pub(super) fn run(args: &Args, src: &str, scenes: &[Scene]) -> Result<(), String> {
    let report = report(args, src, scenes)?;
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| format!("JSON: {e}"))?;
    if let Some(path) = &args.out {
        if path.canonicalize().ok().zip(args.scene.canonicalize().ok()).is_some_and(|(a, b)| a == b) {
            return Err("inspect: файл отчёта не должен заменять исходную сцену".into());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(&bytes).and_then(|_| file.write_all(b"\n")).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes).and_then(|_| stdout.write_all(b"\n")).map_err(|e| format!("stdout: {e}"))
    }
}

fn report(args: &Args, src: &str, scenes: &[Scene]) -> Result<Json, String> {
    if !args.time.is_finite() || args.time < 0.0 {
        return Err("inspect: --time должен быть конечным числом не меньше нуля".into());
    }
    let pick = args
        .board
        .as_deref()
        .map(|pick| {
            let index = pick
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=scenes.len()).contains(n))
                .map(|n| n - 1)
                .or_else(|| scenes.iter().position(|s| s.doc.artboard.name == pick));
            index.ok_or_else(|| format!("нет холста «{pick}»"))
        })
        .transpose()?;
    let (head, ranges) = motif_script::boards(src);
    let mut boards = Vec::new();
    for (index, scene) in scenes.iter().enumerate() {
        if pick.is_some_and(|p| p != index) {
            continue;
        }
        let face = args.side.as_deref().map(|side| motif_pixel::side(&scene.doc, side)).transpose()?;
        let animation = args.anim.as_deref().and_then(|name| match (face, &scene.doc.directions) {
            (Some(k), Some(directions)) => directions.anim_for(&scene.doc, name, k),
            _ => scene.doc.animation(name),
        });
        if let Some(anim) = &args.anim
            && animation.is_none()
        {
            if pick.is_some() {
                return Err(format!("нет анимации «{anim}» на холсте «{}»", scene.doc.artboard.name));
            }
            continue;
        }
        if let Some(id) = &args.node
            && scene.doc.node(id).is_none()
        {
            if pick.is_some() {
                return Err(format!("нет объекта «{id}» на холсте «{}»", scene.doc.artboard.name));
            }
            continue;
        }
        let range = &ranges[index];
        let board_src = format!("{}{}", &src[head.clone()], &src[range.clone()]);
        let mut posed = match animation {
            Some(anim) => anim.pose(&scene.doc, args.time),
            None => {
                let mut d = scene.doc.clone();
                d.apply_parameters();
                d.rig();
                d
            }
        };
        if let Some(side) = face {
            posed.face(side);
            posed.rig();
        }
        boards.push(json!({
            "index":index+1,"name":scene.doc.artboard.name,
            "pose_selection":{"requested_clip":args.anim,"resolved_clip":animation.map(|a|a.name.as_str()),"side":args.side,"side_index":face,"requested_time":args.time,"local_time":posed.time},
            "artboard":{"name":scene.doc.artboard.name,"width":scene.doc.artboard.width,"height":scene.doc.artboard.height,"background":color(Some(scene.doc.artboard.background)),"purpose":format!("{:?}",scene.doc.purpose)},
            "source":{"text":board_src,"span_unit":"utf8_bytes","original_segments":[
                {"local_start":0,"file_start":head.start,"length":head.len()},
                {"local_start":head.len(),"file_start":range.start,"length":range.len()}]},
            "base":document(&scene.doc,scene,&board_src,args.node.as_deref()),
            "posed":document(&posed,scene,&board_src,args.node.as_deref()),
            "drivers":scene.doc.drivers.iter().map(|d|json!({"parameter":d.parameter,"tracks":d.tracks.iter().map(track).collect::<Vec<_>>(),
                "source":scene.driver_sites.get(&d.parameter).map(|sites|sites.iter().map(track_source).collect::<Vec<_>>())})).collect::<Vec<_>>(),
            "clips":scene.doc.animations.iter().map(|a|json!({"name":a.name,"duration":a.duration,"looping":a.looping,
                "tracks":a.tracks.iter().map(track).collect::<Vec<_>>(),"events":a.events.iter().map(|e|json!({"name":e.name,"time":e.time})).collect::<Vec<_>>(),
                "source":scene.anims.get(&a.name).map(|s|json!({"duration":span(s.duration),"loop_word":s.loop_word.map(span),"close":s.close,"tracks":s.tracks.iter().map(track_source).collect::<Vec<_>>()}))})).collect::<Vec<_>>(),
            "machines":scene.doc.machines.iter().map(machine).collect::<Vec<_>>(),
            "metadata":{"characters":motif_script::character::read(&board_src).ok().flatten(),
                "palette":scene.palette.iter().map(|s|json!({"name":s.name,"color":color(Some(s.color)),"literal":s.literal,"span":span(s.span)})).collect::<Vec<_>>(),
                "board_cell":scene.board_cell,
                "directions":scene.doc.directions.as_ref().map(|d|json!({"count":d.count,"turn":d.turn,"mirror":d.mirror}))}
        }));
    }
    if boards.is_empty() {
        return Err(format!("inspect: нет холста для выбранных --anim {:?} и --node {:?}", args.anim, args.node));
    }
    Ok(json!({"schema":1,"source":{"path":args.scene,"encoding":"UTF-8"},
        "selection":{"board":args.board,"anim":args.anim,"requested_time":args.time,"node":args.node,"side":args.side},
        "units":{"length":"artboard_units","angle":"degrees_clockwise","time":"seconds","matrix":"[a,b,c,d,e,f]: x'=a*x+c*y+e; y'=b*x+d*y+f","bounds":"[x0,y0,x1,y1]; geometric, excluding paint effects and clipping"},
        "boards":boards}))
}

fn document(doc: &Document, scene: &Scene, src: &str, root: Option<&str>) -> Json {
    let root = root.and_then(|id| doc.index_of(id));
    json!({"time":doc.time,"light":doc.light,
        "nodes":doc.nodes.iter().enumerate().filter(|(i,_)|root.is_none_or(|r|*i==r||doc.is_inside(*i,r))).map(|(i,_)|node(doc,i,scene,src)).collect::<Vec<_>>(),
        "parameters":doc.parameters.iter().map(|p|json!({"id":p.id,"name":p.name,"min":p.min,"max":p.max,"default":p.default,"value":p.value,"source_value":scene.parameter_values.get(&p.id).copied().map(span)})).collect::<Vec<_>>(),
        "world":{"gravity":[doc.world.gravity.x,doc.world.gravity.y],"wind":[doc.world.wind.x,doc.world.wind.y],"gust":doc.world.gust,"time":doc.world.time}})
}

fn node(doc: &Document, i: usize, scene: &Scene, src: &str) -> Json {
    let n = &doc.nodes[i];
    let properties: serde_json::Map<String, Json> = Prop::ALL.into_iter().map(|p| (p.path_name().into(), value(n.get(p)))).collect();
    let mut sources: Vec<_> = scene.sources.iter().filter(|((id, _), _)| id == &n.id).collect();
    sources.sort_by_key(|((_, p), _)| p.path_name());
    let mut styles: Vec<_> = scene.prop_spans.iter().filter(|((id, _), _)| id == &n.id).collect();
    styles.sort_by_key(|((_, key), _)| key.as_str());
    let authorable = if n.kind == motif_core::ShapeKind::Path { Some(n.editable_path()) } else { n.base_path() };
    let rendered = n.path();
    let world = doc.world_transform(i);
    json!({"index":i,"id":n.id,"name":n.name,"kind":n.kind.keyword(),"parent":n.parent.map(|p|doc.nodes[p].id.as_str()),
        "children":doc.nodes.iter().filter(|c|c.parent==Some(i)).map(|c|c.id.as_str()).collect::<Vec<_>>(),"mask":if n.clip {doc.nodes.iter().find(|c|c.parent==Some(i)).map(|c|c.id.as_str())}else{None},
        "visible":n.visible,"effective_visible":doc.is_visible(i),"locked":n.locked,"effective_locked":doc.is_locked(i),
        "properties":properties,"local_transform":matrix(n.local_transform()),"world_transform":matrix(world),
        "world_bounds":doc.world_bounds(i).map(bounds),"local_bounds":rendered.as_ref().map(|p|bounds(p.bounding_box())),
        "geometry":{"source":path(&n.geometry),"authorable":authorable.as_ref().map(path),"rendered":rendered.as_ref().map(path),"world_rendered":rendered.map(|p|path(&(world*p))),"deformed":n.deformed.as_ref().map(path),"body_faces":n.body_faces(doc.light).map(|faces|faces.iter().map(|f|json!({"path":path(&f.path),"color":color(Some(f.color)),"even_odd":f.even_odd,"edges":f.edges.iter().map(|line|line.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),"top_edges":f.top_edges.iter().map(|line|line.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>())},
        "lattice":n.lattice.as_ref().map(|l|json!({"width":l.width,"height":l.height,"offsets":l.offsets.map(|v|[v.x,v.y]),"controls":(0..9).map(|i|{let p=l.control(i);[p.x,p.y]}).collect::<Vec<_>>(),"neutral":l.neutral()})),
        "forms":n.forms.as_ref().map(|f|json!({"amount":f.amount,"shapes":f.shapes.iter().map(path).collect::<Vec<_>>(),"source_form_index":0})),
        "skin":n.skin.as_ref().map(|s|json!({"bones":s.bones,"sharp":s.sharp,"bind_world":matrix(s.bind_world),
            "bind":s.bind.iter().map(|b|json!({"bone":b.bone,"to_bone":matrix(b.to_bone),"a":[b.a.x,b.a.y],"b":[b.b.x,b.b.y]})).collect::<Vec<_>>(),
            "paint":s.paint.iter().map(|p|json!({"bone":p.bone,"radius":p.radius,"amount":p.amount,"points":p.points.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()})).collect::<Vec<_>>()})),
        "ik":n.ik.as_ref().map(|k|json!({"target":k.target,"chain":k.chain,"flip":k.flip})),
        "morph":n.morph.as_ref().map(|m|json!({"targets":m.targets,"amount":m.amount})),
        "body":n.body.as_ref().map(|b|json!({"depth":b.depth,"taper":b.taper})),
        "orbit":{"on":n.on,"wrap":n.wrap,"behind":n.behind.map(|(i,clip)|json!({"node":doc.nodes[i].id,"clip":clip}))},
        "paint":paint(n),"layout":json!({"stack":n.stack.map(|s|json!({"direction":s.dir.keyword(),"align":s.align.keyword(),"gap":s.gap,"pad":s.pad})),"fit":n.fit,"anchor":n.anchor,"offset":[n.offset.x,n.offset.y],"frame":n.frame.map(bounds)}),
        "ui":{"enabled":n.ui,"role":n.ui_role.keyword(),"part":n.part.map(|p|p.keyword()),"slice":n.slice,"radio":n.radio,"dial":{"drag":n.dial.drag.keyword(),"arc":n.dial.arc,"value":n.dial.value}},
        "text":n.text.as_ref().map(|t|json!({"content":t.content,"font":t.font.as_ref().map(|f|f.name.as_str()),"style":{"size":n.text_style.size,"spacing":n.text_style.spacing,"line_height":n.text_style.line_height,"align":n.text_style.align.keyword(),"reveal":n.text_style.reveal,"wave":n.text_style.wave,"phase":n.text_style.phase}})),
        "physics":physics(n),
        "source":{"site":scene.sites.get(&n.id).map(|s|json!({"span":span(s.span),"line":s.line,"generated":s.generated,"insert_at":s.insert_at,"id_span":span(s.id_span),"name_span":s.name_span.map(span)})),
            "properties":sources.into_iter().map(|((_,p),s)|json!({"property":p.path_name(),"span":span(s.span),"text":src.get(s.span.start..s.span.end),"editable":s.editable,"paired":s.paired.map(|p|p.path_name()),"from_override":s.from_override})).collect::<Vec<_>>(),
            "styles":styles.into_iter().map(|((_,key),s)|json!({"key":key,"span":span(*s),"text":src.get(s.start..s.end)})).collect::<Vec<_>>(),
            "path_span":scene.path_sources.get(&n.id).copied().map(span)}})
}

fn path(p: &BezPath) -> Json {
    json!({"svg":p.to_svg(),"bounds":if p.elements().is_empty(){Json::Null}else{bounds(p.bounding_box())},"elements":p.elements().iter().map(|e|match e {
        PathEl::MoveTo(p)=>json!({"op":"M","points":[[p.x,p.y]]}),PathEl::LineTo(p)=>json!({"op":"L","points":[[p.x,p.y]]}),
        PathEl::QuadTo(a,p)=>json!({"op":"Q","points":[[a.x,a.y],[p.x,p.y]]}),PathEl::CurveTo(a,b,p)=>json!({"op":"C","points":[[a.x,a.y],[b.x,b.y],[p.x,p.y]]}),
        PathEl::ClosePath=>json!({"op":"Z","points":[]})}).collect::<Vec<_>>()})
}
fn bounds(r: Rect) -> Json {
    json!([r.x0, r.y0, r.x1, r.y1])
}
fn matrix(a: Affine) -> Json {
    json!(a.as_coeffs())
}
fn span(s: Span) -> Json {
    json!({"start":s.start,"end":s.end})
}
fn value(v: Value) -> Json {
    match v {
        Value::Num(v) => json!(v),
        Value::Color(c) => json!(c.to_hex()),
        Value::None => Json::Null,
    }
}
fn color(c: Option<motif_core::Color>) -> Json {
    c.map_or(Json::Null, |c| json!(c.to_hex()))
}
fn gradient(g: &Gradient) -> Json {
    json!({"radial":g.radial,"angle":g.angle,"colors":g.colors.iter().map(|c|c.to_hex()).collect::<Vec<_>>()})
}
fn shadow(s: Shadow) -> Json {
    json!({"dx":s.dx,"dy":s.dy,"blur":s.blur,"color":color(Some(s.color))})
}
fn track(t: &Track) -> Json {
    json!({"node":t.node,"property":t.prop.path_name(),"keys":t.keys.iter().map(|k|json!({"time":k.time,"value":value(k.value),"ease":k.ease.script()})).collect::<Vec<_>>()})
}
fn track_source(t: &motif_script::TrackSite) -> Json {
    json!({"node":t.node,"property":t.prop.path_name(),"span":span(t.span),"keys":t.keys.iter().map(|k|json!({"time":k.time,"span":span(k.span),"value_span":span(k.value),"time_span":span(k.time_span),"ease_span":k.ease.map(span)})).collect::<Vec<_>>()})
}

fn paint(n: &Node) -> Json {
    json!({"fill":color(n.fill),"stroke":color(n.stroke),"stroke_width":n.stroke_width,"even_odd":n.even_odd,"clip":n.clip,
        "pivot":[n.pivot.x,n.pivot.y],"radii":n.radii,"dash":n.dash,"cap":n.cap.keyword(),"join":n.join.keyword(),"blend":format!("{:?}",n.blend),
        "gradient":n.gradient.as_ref().map(gradient),"stroke_gradient":n.stroke_gradient.as_ref().map(gradient),
        "shadow":n.shadow.map(shadow),"inset":n.inset.map(shadow),"glow":n.glow.map(|g|json!({"size":g.size,"spread":g.spread,"color":color(Some(g.color))})),"noise":n.noise.map(|n|json!({"amount":n.amount,"size":n.size})),
        "outlines":n.outlines.iter().map(|(c,w)|json!({"color":color(Some(*c)),"width":w})).collect::<Vec<_>>(),
        "brush":{"rough":n.brush.rough,"wave":n.brush.wave,"boil":n.brush.boil,"seed":n.brush.seed,"bands":n.brush.bands,
            "hatch":n.brush.hatch.map(|h|json!({"angle":h.angle,"gap":h.gap,"width":h.width,"color":color(h.color),"cross":h.cross})),
            "grain":n.brush.grain.map(|g|json!({"density":g.density,"size":g.size,"color":color(g.color)}))}})
}
fn physics(n: &Node) -> Json {
    json!({"solid":n.solid,"weight":n.weight,"sail":n.sail,"warp":n.warp.map(matrix),"swing":n.swing,
        "spring":n.spring.map(|s|json!({"freq":s.freq,"damp":s.damp})),"jiggle":n.jiggle.map(|s|json!({"freq":s.freq,"damp":s.damp})),
        "soft":n.soft.map(|s|json!({"stiff":s.stiff,"pressure":s.pressure,"friction":s.friction,"damp":s.damp,"gravity":s.gravity,"drive":s.drive})),
        "squash":n.squash.map(|s|json!({"amount":s.amount,"spring":{"freq":s.spring.freq,"damp":s.spring.damp}})),
        "roll":n.roll.map(|s|json!({"friction":s.friction,"damp":s.damp,"gravity":s.gravity})),
        "fluid":n.fluid.map(|s|json!({"visc":s.visc,"flow":s.flow,"gravity":s.gravity})),
        "emitter":n.emit.map(|s|json!({"rate":s.rate,"burst":s.burst,"life":s.life,"speed":s.speed,"angle":s.angle,"spread":s.spread,"gravity":s.gravity,"drag":s.drag,"fade":s.fade,"grow":s.grow,"spin":s.spin})),
        "particles":n.particles.as_ref().map(|p|p.iter().map(|(m,a)|json!({"transform":matrix(*m),"opacity":a})).collect::<Vec<_>>()),
        "rope":n.rope.as_ref().map(|r|json!({"cloth":r.cloth,"stiff":r.stiff,"damp":r.damp,"gravity":r.gravity,"wind":r.wind,"pins":r.pins.iter().map(|p|p.word()).collect::<Vec<_>>()})),"tie":n.tie,"hang":n.hang,"links":n.links,
        "pull":n.pull.map(|s|json!({"strength":s.strength,"range":s.range,"cone":s.cone})),"fan":n.fan.map(|s|json!({"strength":s.strength,"range":s.range,"cone":s.cone}))})
}
fn machine(m: &motif_core::Machine) -> Json {
    json!({"name":m.name,"any_pos":m.any_pos,"inputs":m.inputs.iter().map(|i|json!({"name":i.name,"kind":format!("{:?}",i.kind),"default":i.default})).collect::<Vec<_>>(),
        "listeners":m.listeners.iter().map(|l|json!({"node":l.node,"event":l.event.keyword(),"input":l.input})).collect::<Vec<_>>(),
        "states":m.states.iter().map(|s|json!({"name":s.name,"anim":s.anim,"pos":s.pos})).collect::<Vec<_>>(),
        "transitions":m.transitions.iter().map(|t|json!({"from":t.from.map(|i|m.states[i].name.as_str()),"to":m.states[t.to].name,"at_end":t.at_end,"blend":t.blend,
            "conditions":t.conds.iter().map(|c|json!({"input":c.input,"op":format!("{:?}",c.op),"value":c.value})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
