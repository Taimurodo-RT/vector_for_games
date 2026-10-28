//! Photoshop files as backdrops: the layers of a PSD (or PSB) read and put back together with
//! some of them turned off or on, so a layered sketch can be traced one part at a time.
//!
//! Only what a reference picture needs is read: pixel layers with their opacity, blend mode,
//! clipping and layer mask, and groups. Effects, adjustment layers and text are drawn as Photoshop
//! last rasterised them, or not at all.

use tiny_skia::{BlendMode, Mask, MaskType, Pixmap, PixmapPaint, Transform};

/// A layered picture: its size and its layers, bottom to top as Photoshop keeps them.
pub struct Layered {
    pub width: u32,
    pub height: u32,
    pub nodes: Vec<Node>,
    /// The flattened picture saved along with the layers: what is shown when there are none.
    merged: Option<Pixmap>,
    /// Layers whose pixels could not be read, drawn as empty.
    pub unreadable: Vec<String>,
}

pub enum Node {
    Layer(Layer),
    Group(Group),
}

/// What layers and groups have in common.
pub struct Props {
    pub name: String,
    /// Visible in the file: the eye in Photoshop's layer list.
    pub visible: bool,
    /// Opacity times fill, 0–1.
    pub opacity: f32,
    pub blend: [u8; 4],
    /// Clipped to the layer below it (Alt-click between layers in Photoshop).
    pub clipped: bool,
}

pub struct Layer {
    pub props: Props,
    pub left: i32,
    pub top: i32,
    /// Premultiplied pixels with the layer mask applied; `None` for an empty layer.
    pub pixmap: Option<Pixmap>,
}

pub struct Group {
    pub props: Props,
    /// Bottom to top.
    pub children: Vec<Node>,
}

impl Node {
    pub fn props(&self) -> &Props {
        match self {
            Node::Layer(l) => &l.props,
            Node::Group(g) => &g.props,
        }
    }
}

/// One line of the layer list, top to bottom as Photoshop shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub depth: usize,
    pub group: bool,
    pub visible: bool,
}

/// Whether a file is read as a layered picture.
pub fn is_layered(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb"))
}

/// Whether a layer or group is drawn: as in the file, unless its name is in `show` or `hide`.
/// Hiding wins over showing.
pub fn shown(p: &Props, hide: &[String], show: &[String]) -> bool {
    !hide.contains(&p.name) && (p.visible || show.contains(&p.name))
}

impl Layered {
    /// The layer list, top to bottom, groups before what is in them.
    pub fn rows(&self) -> Vec<Row> {
        fn walk(nodes: &[Node], depth: usize, out: &mut Vec<Row>) {
            for n in nodes.iter().rev() {
                let p = n.props();
                out.push(Row { name: p.name.clone(), depth, group: matches!(n, Node::Group(_)), visible: p.visible });
                if let Node::Group(g) = n {
                    walk(&g.children, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.nodes, 0, &mut out);
        out
    }

    /// The picture with the layers named in `hide` turned off and those in `show` turned on.
    pub fn compose(&self, hide: &[String], show: &[String]) -> Option<Pixmap> {
        if self.nodes.is_empty() {
            return self.merged.clone();
        }
        let mut canvas = Pixmap::new(self.width, self.height)?;
        self.draw(&mut canvas, &self.nodes, 1.0, &|p| shown(p, hide, show));
        Some(canvas)
    }

    /// Draws `nodes`, bottom to top, onto `canvas`, with `opacity` from pass-through groups
    /// around them.
    fn draw(&self, canvas: &mut Pixmap, nodes: &[Node], opacity: f32, shown: &dyn Fn(&Props) -> bool) {
        // The alpha of the last unclipped node: what the clipped ones above it are cut by. `None`
        // while that node is hidden, and then so are they.
        let mut base: Option<Mask> = None;
        for (i, node) in nodes.iter().enumerate() {
            let p = node.props();
            let visible = shown(p);
            let clipped = p.clipped && i > 0;
            if !clipped {
                base = None;
            }
            if !visible || (clipped && base.is_none()) {
                continue;
            }
            let is_base = !clipped && nodes.get(i + 1).is_some_and(|n| n.props().clipped);
            let paint = PixmapPaint { opacity: (p.opacity * opacity).clamp(0.0, 1.0), blend_mode: blend(p.blend), quality: tiny_skia::FilterQuality::Nearest };
            match node {
                // Straight onto the canvas, the usual case.
                Node::Layer(l) if !clipped && !is_base => {
                    if let Some(px) = &l.pixmap {
                        canvas.draw_pixmap(l.left, l.top, px.as_ref(), &paint, Transform::identity(), None);
                    }
                }
                Node::Group(g) if !clipped && !is_base && &p.blend == b"pass" => self.draw(canvas, &g.children, paint.opacity, shown),
                // On a picture of its own first: a group blended as a whole, a clipping base, a
                // clipped layer.
                _ => {
                    let Some(mut own) = Pixmap::new(self.width, self.height) else { continue };
                    match node {
                        Node::Layer(l) => {
                            if let Some(px) = &l.pixmap {
                                own.draw_pixmap(l.left, l.top, px.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                            }
                        }
                        Node::Group(g) => self.draw(&mut own, &g.children, 1.0, shown),
                    }
                    canvas.draw_pixmap(0, 0, own.as_ref(), &paint, Transform::identity(), if clipped { base.as_ref() } else { None });
                    if is_base {
                        base = Some(Mask::from_pixmap(own.as_ref(), MaskType::Alpha));
                    }
                }
            }
        }
    }
}

/// Photoshop's blend mode as tiny-skia has it; the ones it lacks are drawn as normal.
fn blend(key: [u8; 4]) -> BlendMode {
    match &key {
        b"mul " => BlendMode::Multiply,
        b"scrn" => BlendMode::Screen,
        b"over" => BlendMode::Overlay,
        b"dark" => BlendMode::Darken,
        b"lite" => BlendMode::Lighten,
        b"div " => BlendMode::ColorDodge,
        b"idiv" => BlendMode::ColorBurn,
        b"hLit" => BlendMode::HardLight,
        b"sLit" => BlendMode::SoftLight,
        b"diff" => BlendMode::Difference,
        b"smud" => BlendMode::Exclusion,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"colr" => BlendMode::Color,
        b"lum " => BlendMode::Luminosity,
        b"lddg" => BlendMode::Plus,
        _ => BlendMode::SourceOver,
    }
}

/// Reads a PSD or PSB file.
pub fn read(bytes: &[u8]) -> Result<Layered, String> {
    let mut c = Cur { b: bytes, p: 0 };
    if c.take(4)? != b"8BPS" {
        return Err("это не файл Photoshop".into());
    }
    let big = match c.u16()? {
        1 => false,
        2 => true,
        v => return Err(format!("неизвестная версия PSD: {v}")),
    };
    c.take(6)?;
    let channels = c.u16()? as usize;
    let height = c.u32()?;
    let width = c.u32()?;
    let depth = c.u16()?;
    let mode = c.u16()?;
    if depth != 8 && depth != 16 {
        return Err(format!("{depth} бит на канал не читаются — сохраните копию в 8 или 16 бит"));
    }
    let gray = match mode {
        1 => true,
        3 => false,
        4 => return Err("PSD в CMYK — сохраните копию в RGB".into()),
        _ => return Err("PSD не в RGB и не в оттенках серого — сохраните копию в RGB".into()),
    };
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err(format!("размер {width}×{height} не подходит для подложки"));
    }
    let f = Format { big, depth, gray };
    let colour_data = c.u32()? as usize;
    c.take(colour_data)?;
    let resources = c.u32()? as usize;
    c.take(resources)?;
    let section = c.len(big)?;
    let mut nodes = Vec::new();
    let mut unreadable = Vec::new();
    {
        let mut s = Cur { b: c.take(section)?, p: 0 };
        if section > 0 {
            let info = s.len(big)?;
            if info > 0 {
                nodes = layer_info(s.take(info)?, f, &mut unreadable)?;
            }
            // Global layer mask, then more blocks: 16- and 32-bit files keep their layers in one.
            let global_mask = s.u32()? as usize;
            s.take(global_mask)?;
            while nodes.is_empty() && s.left() >= 12 {
                if s.peek(4) == Some(&[0; 4][..]) {
                    s.take(1)?;
                    continue;
                }
                let (key, data) = block(&mut s, big)?;
                if matches!(&key, b"Lr16" | b"Lr32" | b"Layr") {
                    nodes = layer_info(data, f, &mut unreadable)?;
                }
            }
        }
    }
    let merged = if nodes.is_empty() { merged_image(&mut c, f, channels, width, height).ok() } else { None };
    if nodes.is_empty() && merged.is_none() {
        return Err("в файле нет ни слоёв, ни сведённой картинки".into());
    }
    Ok(Layered { width, height, nodes, merged, unreadable })
}

#[derive(Clone, Copy)]
struct Format {
    /// PSB: some lengths take 8 bytes, row counts 4.
    big: bool,
    depth: u16,
    gray: bool,
}

/// The fields of one layer record that matter here.
struct Record {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    channels: Vec<(i16, usize)>,
    blend: [u8; 4],
    opacity: u8,
    fill: u8,
    clipped: bool,
    visible: bool,
    mask: Option<MaskRect>,
    name: String,
    /// `lsct`: 1 or 2 opens a group (its top), 3 is the hidden line under the group's layers.
    divider: u32,
}

struct MaskRect {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    /// The mask's value outside its rectangle.
    default: u8,
    disabled: bool,
}

/// The layer records and their pixels, built into the tree of groups.
fn layer_info(data: &[u8], f: Format, unreadable: &mut Vec<String>) -> Result<Vec<Node>, String> {
    let mut c = Cur { b: data, p: 0 };
    let count = c.i16()?.unsigned_abs() as usize;
    let records = (0..count).map(|_| record(&mut c, f)).collect::<Result<Vec<_>, _>>()?;
    // Bottom to top: the hidden line under a group's layers opens a level, the group's own
    // record above them closes it.
    let mut stack: Vec<Vec<Node>> = vec![Vec::new()];
    for r in records {
        let mut channels = std::collections::HashMap::new();
        let (w, h) = ((r.right - r.left).max(0) as usize, (r.bottom - r.top).max(0) as usize);
        let mut broken = false;
        for &(id, len) in &r.channels {
            let raw = c.take(len)?;
            let size = match (id, &r.mask) {
                (-2, Some(m)) => ((m.right - m.left).max(0) as usize, (m.bottom - m.top).max(0) as usize),
                (-1..=2, _) => (w, h),
                _ => continue,
            };
            // A channel packed in a way not read here spoils only its own layer.
            match channel(raw, size, f) {
                Ok(v) => _ = channels.insert(id, v),
                Err(_) => broken = true,
            }
        }
        if broken && r.divider == 0 {
            unreadable.push(r.name.clone());
        }
        let props =
            Props { name: r.name.clone(), visible: r.visible, opacity: r.opacity as f32 / 255.0 * r.fill as f32 / 255.0, blend: r.blend, clipped: r.clipped };
        match r.divider {
            3 => stack.push(Vec::new()),
            1 | 2 => {
                let children = if stack.len() > 1 { stack.pop().unwrap_or_default() } else { Vec::new() };
                stack.last_mut().expect("the outermost level stays").push(Node::Group(Group { props, children }));
            }
            _ => {
                let pixmap = pixels(&r, &channels, (w, h), f.gray);
                stack.last_mut().expect("the outermost level stays").push(Node::Layer(Layer { props, left: r.left, top: r.top, pixmap }));
            }
        }
    }
    // A group left open by a broken file: its layers go to the level around it.
    while stack.len() > 1 {
        let inner = stack.pop().unwrap_or_default();
        stack.last_mut().expect("the outermost level stays").extend(inner);
    }
    Ok(stack.pop().unwrap_or_default())
}

fn record(c: &mut Cur, f: Format) -> Result<Record, String> {
    let (top, left, bottom, right) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
    let n = c.u16()?;
    let mut channels = Vec::new();
    for _ in 0..n {
        let id = c.i16()?;
        channels.push((id, c.len(f.big)?));
    }
    c.take(4)?;
    let blend: [u8; 4] = c.take(4)?.try_into().map_err(|_| "PSD оборван")?;
    let opacity = c.u8()?;
    let clipped = c.u8()? != 0;
    let flags = c.u8()?;
    c.u8()?;
    let extra = c.u32()? as usize;
    let mut e = Cur { b: c.take(extra)?, p: 0 };
    let mask_len = e.u32()? as usize;
    let mut m = Cur { b: e.take(mask_len)?, p: 0 };
    let mask = if mask_len >= 18 {
        let (top, left, bottom, right) = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
        let default = m.u8()?;
        let flags = m.u8()?;
        Some(MaskRect { top, left, bottom, right, default, disabled: flags & 2 != 0 })
    } else {
        None
    };
    let ranges = e.u32()? as usize;
    e.take(ranges)?;
    let name_len = e.u8()? as usize;
    // The short name is in the system code page; `luni` below has it in Unicode.
    let mut name: String = e.take(name_len)?.iter().map(|&b| if b.is_ascii() { b as char } else { '?' }).collect();
    e.take((4 - (name_len + 1) % 4) % 4)?;
    let mut rec =
        Record { top, left, bottom, right, channels, blend, opacity, fill: 255, clipped, visible: flags & 2 == 0, mask, name: String::new(), divider: 0 };
    while e.left() >= 12 {
        let (key, data) = block(&mut e, f.big)?;
        let mut d = Cur { b: data, p: 0 };
        match &key {
            b"luni" => {
                let n = d.u32()? as usize;
                let units: Vec<u16> = (0..n).map_while(|_| d.u16().ok()).collect();
                name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
            }
            b"lsct" | b"lsdk" => {
                rec.divider = d.u32()?;
                // A group keeps its real blend mode here, mostly pass-through.
                if d.left() >= 8 {
                    d.take(4)?;
                    rec.blend = d.take(4)?.try_into().map_err(|_| "PSD оборван")?;
                }
            }
            b"iOpa" => rec.fill = d.u8()?,
            _ => {}
        }
    }
    rec.name = name;
    Ok(rec)
}

/// One block of additional information: its key and its data.
fn block<'a>(c: &mut Cur<'a>, big: bool) -> Result<([u8; 4], &'a [u8]), String> {
    let sig = c.take(4)?;
    if sig != b"8BIM" && sig != b"8B64" {
        return Err("PSD: не разобран блок слоя".into());
    }
    let key: [u8; 4] = c.take(4)?.try_into().map_err(|_| "PSD оборван")?;
    // In a PSB these keys take an 8-byte length.
    let long =
        big && matches!(&key, b"LMsk" | b"Lr16" | b"Lr32" | b"Layr" | b"Mt16" | b"Mt32" | b"Mtrn" | b"Alph" | b"FMsk" | b"lnk2" | b"FEid" | b"FXid" | b"PxSD");
    let len = c.len(long)?;
    let data = c.take(len.min(c.left()))?;
    Ok((key, data))
}

/// The pixels of a layer from its channels: premultiplied, with its mask applied.
fn pixels(r: &Record, ch: &std::collections::HashMap<i16, Vec<u8>>, (w, h): (usize, usize), gray: bool) -> Option<Pixmap> {
    let mut px = Pixmap::new(w as u32, h as u32)?;
    let red = ch.get(&0)?;
    let (green, blue) = if gray { (red, red) } else { (ch.get(&1)?, ch.get(&2)?) };
    let alpha = ch.get(&-1);
    let mask = r.mask.as_ref().filter(|m| !m.disabled).zip(ch.get(&-2));
    let data = px.data_mut();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut a = alpha.map_or(255, |a| a[i]) as u32;
            if let Some((m, values)) = mask {
                let (mx, my) = (r.left + x as i32 - m.left, r.top + y as i32 - m.top);
                let mw = (m.right - m.left) as usize;
                let inside = mx >= 0 && my >= 0 && mx < m.right - m.left && my < m.bottom - m.top;
                let v = if inside { values[my as usize * mw + mx as usize] } else { m.default } as u32;
                a = a * v / 255;
            }
            let pre = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            data[i * 4..i * 4 + 4].copy_from_slice(&[pre(red[i]), pre(green[i]), pre(blue[i]), a as u8]);
        }
    }
    Some(px)
}

/// The flattened picture at the end of the file.
fn merged_image(c: &mut Cur, f: Format, channels: usize, width: u32, height: u32) -> Result<Pixmap, String> {
    let (w, h) = (width as usize, height as usize);
    let compression = c.u16()?;
    let bps = f.depth as usize / 8;
    let wanted = if f.gray { 1 } else { 3 }.min(channels);
    let has_alpha = channels > wanted;
    let take = wanted + has_alpha as usize;
    let mut planes = Vec::new();
    match compression {
        0 => {
            for _ in 0..take {
                planes.push(narrow(c.take(w * h * bps)?, f.depth));
            }
        }
        1 => {
            let count = |c: &mut Cur| if f.big { c.u32().map(|v| v as usize) } else { c.u16().map(|v| v as usize) };
            let counts = (0..channels * h).map(|_| count(c)).collect::<Result<Vec<_>, _>>()?;
            for k in 0..take {
                let mut plane = Vec::with_capacity(w * h * bps);
                for row in &counts[k * h..(k + 1) * h] {
                    plane.extend(unpack(c.take(*row)?, w * bps)?);
                }
                planes.push(narrow(&plane, f.depth));
            }
        }
        _ => return Err("сведённая картинка сжата ZIP".into()),
    }
    let blank = vec![255; w * h];
    let (r, g, b) = if f.gray { (&planes[0], &planes[0], &planes[0]) } else { (&planes[0], &planes[1], &planes[2]) };
    let a = if has_alpha { &planes[wanted] } else { &blank };
    let mut px = Pixmap::new(width, height).ok_or("пустая картинка")?;
    for (i, out) in px.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let alpha = a[i] as u32;
        let pre = |c: u8| ((c as u32 * alpha + 127) / 255) as u8;
        out.copy_from_slice(&[pre(r[i]), pre(g[i]), pre(b[i]), a[i]]);
    }
    Ok(px)
}

/// One channel's data, 8 bits a sample, `w × h`: raw, PackBits, or ZIP with or without prediction.
fn channel(raw: &[u8], (w, h): (usize, usize), f: Format) -> Result<Vec<u8>, String> {
    let mut c = Cur { b: raw, p: 0 };
    let compression = c.u16()?;
    let bps = f.depth as usize / 8;
    let row = w * bps;
    let data = &raw[2..];
    let bytes = match compression {
        0 => data.get(..row * h).ok_or("слой оборван")?.to_vec(),
        1 => {
            let counts = (0..h).map(|_| if f.big { c.u32().map(|v| v as usize) } else { c.u16().map(|v| v as usize) }).collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::with_capacity(row * h);
            for n in counts {
                out.extend(unpack(c.take(n)?, row)?);
            }
            out
        }
        2 | 3 => {
            use std::io::Read;
            let mut out = Vec::with_capacity(row * h);
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).map_err(|e| format!("слой ZIP: {e}"))?;
            out.resize(row * h, 0);
            if compression == 3 {
                // Each sample stored as the difference from the one before it in the row.
                for line in out.chunks_exact_mut(row.max(1)) {
                    if bps == 2 {
                        let mut prev = 0u16;
                        for s in line.as_chunks_mut::<2>().0 {
                            prev = prev.wrapping_add(u16::from_be_bytes([s[0], s[1]]));
                            s.copy_from_slice(&prev.to_be_bytes());
                        }
                    } else {
                        for i in 1..line.len() {
                            line[i] = line[i].wrapping_add(line[i - 1]);
                        }
                    }
                }
            }
            out
        }
        v => return Err(format!("неизвестное сжатие слоя: {v}")),
    };
    Ok(narrow(&bytes, f.depth))
}

/// 16-bit samples down to 8.
fn narrow(bytes: &[u8], depth: u16) -> Vec<u8> {
    if depth == 16 { bytes.as_chunks::<2>().0.iter().map(|s| s[0]).collect() } else { bytes.to_vec() }
}

/// PackBits: a row of `len` bytes.
fn unpack(src: &[u8], len: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i < src.len() && out.len() < len {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let run = n as usize + 1;
            out.extend_from_slice(src.get(i..i + run).ok_or("слой оборван")?);
            i += run;
        } else if n != -128 {
            let v = *src.get(i).ok_or("слой оборван")?;
            out.extend(std::iter::repeat_n(v, (1 - n as isize) as usize));
            i += 1;
        }
    }
    out.resize(len, 0);
    Ok(out)
}

/// Reading big-endian numbers off a slice, with an error instead of a panic at its end.
struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let out = self.b.get(self.p..self.p.checked_add(n).ok_or("PSD оборван")?).ok_or("PSD оборван")?;
        self.p += n;
        Ok(out)
    }
    fn peek(&self, n: usize) -> Option<&'a [u8]> {
        self.b.get(self.p..self.p + n)
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap_or_default()))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    /// A length: 4 bytes, or 8 where a PSB has them.
    fn len(&mut self, long: bool) -> Result<usize, String> {
        if long { Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap_or_default()) as usize) } else { Ok(self.u32()? as usize) }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A layer record to write: in file order, bottom to top.
    #[derive(Clone)]
    pub(crate) struct Rec {
        pub name: &'static str,
        /// Left, top, width, height.
        pub rect: (i32, i32, i32, i32),
        pub color: [u8; 4],
        pub hidden: bool,
        pub opacity: u8,
        pub blend: &'static [u8; 4],
        pub clipped: bool,
        /// `lsct`: 1 for a group's own record, 3 for the line under its layers.
        pub divider: Option<u32>,
        /// PackBits instead of raw.
        pub rle: bool,
    }

    impl Rec {
        pub fn layer(name: &'static str, rect: (i32, i32, i32, i32), color: [u8; 4]) -> Rec {
            Rec { name, rect, color, hidden: false, opacity: 255, blend: b"norm", clipped: false, divider: None, rle: false }
        }
        pub fn group(name: &'static str) -> Rec {
            Rec { divider: Some(1), blend: b"pass", ..Rec::layer(name, (0, 0, 0, 0), [0; 4]) }
        }
        pub fn end() -> Rec {
            Rec { divider: Some(3), ..Rec::layer("</Layer group>", (0, 0, 0, 0), [0; 4]) }
        }
    }

    /// A PSD file of `w × h` with these layers, solid colours each, written as Photoshop would.
    pub(crate) fn write(w: u32, h: u32, recs: &[Rec]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(b"8BPS");
        out.extend(1u16.to_be_bytes());
        out.extend([0; 6]);
        out.extend(3u16.to_be_bytes());
        out.extend(h.to_be_bytes());
        out.extend(w.to_be_bytes());
        out.extend(8u16.to_be_bytes());
        out.extend(3u16.to_be_bytes());
        out.extend(0u32.to_be_bytes());
        out.extend(0u32.to_be_bytes());
        let mut info = Vec::new();
        info.extend((recs.len() as i16).to_be_bytes());
        let mut pixels = Vec::new();
        for r in recs {
            let (l, t, rw, rh) = r.rect;
            let mut chans = Vec::new();
            for (id, v) in [(0i16, r.color[0]), (1, r.color[1]), (2, r.color[2]), (-1, r.color[3])] {
                let mut data = Vec::new();
                if r.rle {
                    data.extend(1u16.to_be_bytes());
                    let row: Vec<u8> = (0..rw).step_by(128).flat_map(|x| [(257 - (rw - x).min(128)) as u8, v]).collect();
                    for _ in 0..rh {
                        data.extend((row.len() as u16).to_be_bytes());
                    }
                    for _ in 0..rh {
                        data.extend(&row);
                    }
                } else {
                    data.extend(0u16.to_be_bytes());
                    data.extend(std::iter::repeat_n(v, (rw * rh) as usize));
                }
                chans.push((id, data));
            }
            for v in [t, l, t + rh, l + rw] {
                info.extend(v.to_be_bytes());
            }
            info.extend((chans.len() as u16).to_be_bytes());
            for (id, data) in &chans {
                info.extend(id.to_be_bytes());
                info.extend((data.len() as u32).to_be_bytes());
            }
            info.extend(b"8BIM");
            info.extend(if r.divider.is_some() { b"norm" } else { r.blend });
            info.extend([r.opacity, r.clipped as u8, if r.hidden { 0x0a } else { 0x08 }, 0]);
            let mut extra = Vec::new();
            extra.extend(0u32.to_be_bytes());
            extra.extend(0u32.to_be_bytes());
            let short: Vec<u8> = r.name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect();
            extra.push(short.len() as u8);
            extra.extend(&short);
            extra.extend(std::iter::repeat_n(0, (4 - (short.len() + 1) % 4) % 4));
            let units: Vec<u16> = r.name.encode_utf16().collect();
            extra.extend(b"8BIMluni");
            extra.extend((4 + units.len() as u32 * 2).to_be_bytes());
            extra.extend((units.len() as u32).to_be_bytes());
            units.iter().for_each(|u| extra.extend(u.to_be_bytes()));
            if let Some(d) = r.divider {
                extra.extend(b"8BIMlsct");
                extra.extend(12u32.to_be_bytes());
                extra.extend(d.to_be_bytes());
                extra.extend(b"8BIM");
                extra.extend(r.blend);
            }
            info.extend((extra.len() as u32).to_be_bytes());
            info.extend(extra);
            chans.into_iter().for_each(|(_, d)| pixels.extend(d));
        }
        info.extend(pixels);
        let mut section = Vec::new();
        section.extend((info.len() as u32).to_be_bytes());
        section.extend(info);
        section.extend(0u32.to_be_bytes());
        out.extend((section.len() as u32).to_be_bytes());
        out.extend(section);
        out.extend(0u16.to_be_bytes());
        out.extend(std::iter::repeat_n(255, (w * h * 3) as usize));
        out
    }

    fn rgba(px: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = px.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// Bottom to top: «Фон» red all over, «Эскиз» green on the left (hidden in the file), a group
    /// «Герой» with blue «Тело» in the middle, half transparent.
    fn sample() -> Layered {
        let bytes = write(
            20,
            10,
            &[
                Rec { rle: true, ..Rec::layer("Фон", (0, 0, 20, 10), RED) },
                Rec { hidden: true, ..Rec::layer("Эскиз", (0, 0, 5, 10), GREEN) },
                Rec::end(),
                Rec { opacity: 128, ..Rec::layer("Тело", (8, 2, 4, 4), BLUE) },
                Rec::group("Герой"),
            ],
        );
        read(&bytes).unwrap()
    }

    #[test]
    fn layers_and_groups_are_read() {
        let psd = sample();
        assert_eq!((psd.width, psd.height), (20, 10));
        let row = |name: &str, depth, group, visible| Row { name: name.into(), depth, group, visible };
        assert_eq!(psd.rows(), vec![row("Герой", 0, true, true), row("Тело", 1, false, true), row("Эскиз", 0, false, false), row("Фон", 0, false, true)]);
    }

    #[test]
    fn layers_are_put_together_as_asked() {
        let psd = sample();
        let px = psd.compose(&[], &[]).unwrap();
        assert_eq!(rgba(&px, 1, 1), RED, "the sketch is hidden in the file");
        let mid = rgba(&px, 9, 3);
        assert!(mid[0].abs_diff(127) <= 2 && mid[2].abs_diff(128) <= 2 && mid[3] == 255, "{mid:?}");
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let px = psd.compose(&[], &names(&["Эскиз"])).unwrap();
        assert_eq!(rgba(&px, 1, 1), GREEN);
        // A group hides what is in it; hiding wins over showing.
        let px = psd.compose(&names(&["Герой", "Фон"]), &names(&["Фон"])).unwrap();
        assert_eq!(rgba(&px, 9, 3)[3], 0);
        assert_eq!(rgba(&px, 1, 1)[3], 0);
    }

    #[test]
    fn clipping_and_blending() {
        let bytes = write(
            10,
            10,
            &[
                Rec::layer("Белый", (0, 0, 10, 10), [255, 255, 255, 255]),
                Rec::layer("Основа", (2, 2, 3, 3), GREEN),
                Rec { clipped: true, ..Rec::layer("Тень", (0, 0, 10, 10), BLUE) },
                Rec { blend: b"mul ", ..Rec::layer("Умножение", (6, 0, 4, 10), [255, 0, 255, 255]) },
            ],
        );
        let psd = read(&bytes).unwrap();
        let px = psd.compose(&[], &[]).unwrap();
        assert_eq!(rgba(&px, 3, 3), BLUE, "clipped to its base");
        assert_eq!(rgba(&px, 0, 0), [255, 255, 255, 255], "nothing outside the base");
        assert_eq!(rgba(&px, 7, 7), [255, 0, 255, 255], "multiplied over white");
        // With the base hidden the clipped layer goes too.
        let px = psd.compose(&["Основа".into()], &[]).unwrap();
        assert_eq!(rgba(&px, 3, 3), [255, 255, 255, 255]);
    }

    #[test]
    fn channels_decode() {
        let f8 = Format { big: false, depth: 8, gray: false };
        assert_eq!(unpack(&[2, 1, 2, 3, 0xfe, 9], 6).unwrap(), vec![1, 2, 3, 9, 9, 9]);
        // 16-bit samples, ZIP with prediction.
        let samples: Vec<u16> = vec![0x1000, 0x2000, 0x3000];
        let mut deltas = Vec::new();
        let mut prev = 0u16;
        for s in &samples {
            deltas.extend(s.wrapping_sub(prev).to_be_bytes());
            prev = *s;
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &deltas).unwrap();
        let mut raw = 3u16.to_be_bytes().to_vec();
        raw.extend(z.finish().unwrap());
        assert_eq!(channel(&raw, (3, 1), Format { depth: 16, ..f8 }).unwrap(), vec![0x10, 0x20, 0x30]);
        assert!(read(b"not a psd").is_err());
        let cut = write(4, 4, &[Rec::layer("a", (0, 0, 4, 4), RED)]);
        assert!(read(&cut[..cut.len() - 80]).is_err(), "a cut file is an error, not a panic");
    }
}
