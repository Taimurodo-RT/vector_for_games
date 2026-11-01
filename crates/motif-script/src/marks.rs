//! The artist's marks on a morphing shape: `match "0>1 0:0=0:0 0:0.5=0:0.5"`.
//!
//! Each string is one step between neighbouring key contours, `K>K+1` (0 is the shape itself,
//! then each morph target or form), followed by marks `FROM=TO`:
//! - `2=1`: piece 2 of contour K becomes piece 1 of contour K+1;
//! - `0:0.25=1:0.5`: the place a quarter along piece 0 meets the place half along piece 1;
//! - `3:0.5=*`: piece 3 has no partner and shrinks into its middle; `*=2:0` grows from its start.
//!
//! Pieces count from 0 in drawing order (each `M`); a place is a share of the piece's length
//! from its start, 0 to 1.

use motif_core::{MatchEnd, MorphLink};

/// The marks in one `match` string.
pub fn parse(text: &str) -> Result<Vec<MorphLink>, String> {
    let mut words = text.split_whitespace();
    let Some(step) = words.next() else {
        return Err("пустые метки: match \"0>1 0:0=0:0\" — шаг между контурами, затем метки".into());
    };
    let key = match step.split_once('>').map(|(a, b)| (a.parse::<usize>(), b.parse::<usize>())) {
        Some((Ok(a), Ok(b))) if b == a + 1 => a,
        Some((Ok(a), Ok(b))) => return Err(format!("метки ставятся между соседними контурами: {a}>{} вместо {a}>{b}", a + 1)),
        _ => return Err(format!("«{step}»: сначала шаг между контурами, например 0>1")),
    };
    let mut out = Vec::new();
    for w in words {
        let Some((a, b)) = w.split_once('=') else {
            return Err(format!("«{w}»: метка пишется так: 0:0.25=0:0.5 (кусок:место=кусок:место), 2=1 или 3:0.5=*"));
        };
        let (from, to) = (end(a, w)?, end(b, w)?);
        match (from, to) {
            (None, None) => return Err(format!("«{w}»: с обеих сторон «*» — нечего сопоставлять")),
            (Some(x), Some(y)) if x.at.is_some() != y.at.is_some() => {
                return Err(format!("«{w}»: место нужно с обеих сторон, например {}:0.5={}:0.5", x.piece, y.piece));
            }
            _ => {}
        }
        out.push(MorphLink { key, from, to });
    }
    if out.is_empty() {
        return Err(format!("после {step} нужны метки, например 0:0=0:0"));
    }
    Ok(out)
}

fn end(side: &str, word: &str) -> Result<Option<MatchEnd>, String> {
    if side == "*" {
        return Ok(None);
    }
    let (piece, at) = match side.split_once(':') {
        Some((p, f)) => (p, Some(f)),
        None => (side, None),
    };
    let piece = piece.parse::<usize>().map_err(|_| format!("«{word}»: кусок — номер с нуля, например 0:0.5"))?;
    let at = match at.map(|f| f.parse::<f64>()) {
        None => None,
        Some(Ok(f)) if (0.0..=1.0).contains(&f) => Some(f),
        Some(_) => return Err(format!("«{word}»: место на куске — доля его длины от 0 до 1")),
    };
    Ok(Some(MatchEnd { piece, at }))
}

/// The `match` strings that read back as `links`, one for each step, in the order of steps.
pub fn format(links: &[MorphLink]) -> Vec<String> {
    let mut keys: Vec<usize> = links.iter().map(|l| l.key).collect();
    keys.sort_unstable();
    keys.dedup();
    let side = |e: Option<MatchEnd>| match e {
        None => "*".to_string(),
        Some(MatchEnd { piece, at: None }) => piece.to_string(),
        Some(MatchEnd { piece, at: Some(f) }) => format!("{piece}:{}", crate::fmt_num_with(f, 4)),
    };
    keys.iter()
        .map(|&k| {
            let marks: Vec<String> = links.iter().filter(|l| l.key == k).map(|l| format!("{}={}", side(l.from), side(l.to))).collect();
            format!("{k}>{} {}", k + 1, marks.join(" "))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_read_and_write_back() {
        let links = parse("0>1 0:0=0:0 0:0.5=0:0.5 2=1 3:0.25=* *=2").unwrap();
        assert_eq!(links.len(), 5);
        assert_eq!(links[2], MorphLink { key: 0, from: Some(MatchEnd { piece: 2, at: None }), to: Some(MatchEnd { piece: 1, at: None }) });
        assert_eq!(links[3].to, None);
        assert_eq!(format(&links), vec!["0>1 0:0=0:0 0:0.5=0:0.5 2=1 3:0.25=* *=2".to_string()]);
        for (bad, says) in [
            ("0>2 0=0", "соседними"),
            ("0:0=0:0", "шаг"),
            ("0>1", "нужны метки"),
            ("0>1 0:1.5=0:0", "от 0 до 1"),
            ("0>1 0:0.5=0", "с обеих сторон"),
            ("0>1 *=*", "нечего"),
            ("0>1 a=0", "номер"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
