//! A stand-in for a game, to try the live link: start Motif with `--link`, then
//! `cargo run -p motif-link --example game -- сцена.msc Машина Состояние вход=значение…`.
//! It opens the state, tells Motif its names, drives the inputs for a few seconds and prints
//! everything Motif says.

use motif_link::{Client, DEFAULT_PORT, ToMotif};
use std::time::{Duration, Instant};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, machine, state, values @ ..] = args.as_slice() else {
        return Err("нужно: сцена.msc Машина Состояние [вход=значение…]".into());
    };
    let file = std::path::absolute(file).map_err(|e| e.to_string())?.display().to_string();
    let values: std::collections::BTreeMap<String, f64> =
        values.iter().filter_map(|v| v.split_once('=')).filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?))).collect();
    let port = std::env::var("MOTIF_LINK").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let mut motif = Client::connect(port)?;
    println!("{:?}", motif.hello);
    motif.send(&ToMotif::Open { file, board: None, animation: None, state: Some(state.clone()) })?;
    let names = values.keys().cloned().collect();
    motif.send(&ToMotif::Profile { values: names, actions: vec!["attack".into()], events: vec!["land".into(), "hit".into()] })?;
    let until = Instant::now() + Duration::from_secs(4);
    while Instant::now() < until {
        motif.send(&ToMotif::GameValues { machine: Some(machine.clone()), values: values.clone() })?;
        while let Some(message) = motif.recv(Duration::from_millis(100))? {
            println!("{message:?}");
        }
    }
    Ok(())
}
