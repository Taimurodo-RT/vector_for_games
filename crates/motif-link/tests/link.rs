//! The link as a game sees it: a real socket on 127.0.0.1, messages both ways.

use motif_link::{Client, FromMotif, Server, ToMotif, changes};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;

/// Waits for the server's messages, as the editor does every frame.
fn wait(server: &Server) -> Vec<Result<ToMotif, String>> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let got = server.poll();
        if !got.is_empty() || Instant::now() > until {
            return got;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_game_and_motif_talk_both_ways() {
    let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = woken.clone();
    let server = Server::start(0, "test", move || _ = w.fetch_add(1, std::sync::atomic::Ordering::Relaxed)).unwrap();
    let mut game = Client::connect(server.port()).unwrap();
    assert_eq!(game.hello, FromMotif::Hello { app: "motif".into(), version: "test".into(), protocol: motif_link::PROTOCOL });

    // As a game in any language writes it.
    let raw = r#"{"type":"open","file":"hero.msc","state":"Run"}"#;
    game.send(&serde_json::from_str(raw).unwrap()).unwrap();
    let got = wait(&server);
    assert_eq!(got, [Ok(ToMotif::Open { file: "hero.msc".into(), board: None, animation: None, state: Some("Run".into()) })]);
    assert!(woken.load(std::sync::atomic::Ordering::Relaxed) >= 1, "the window is woken for it");
    game.send(&ToMotif::GameValues { machine: None, values: [("speed".to_string(), 0.8)].into() }).unwrap();
    assert!(matches!(&wait(&server)[..], [Ok(ToMotif::GameValues { values, .. })] if values["speed"] == 0.8));

    server.send(&FromMotif::Focus { file: "hero.msc".into(), object: Some("body".into()) });
    assert_eq!(game.recv(Duration::from_secs(5)).unwrap(), Some(FromMotif::Focus { file: "hero.msc".into(), object: Some("body".into()) }));
    let json = FromMotif::Focus { file: "hero.msc".into(), object: None }.to_json();
    assert_eq!(json, r#"{"type":"focus","file":"hero.msc"}"#);

    // Two games at once both hear Motif.
    let mut second = Client::connect(server.port()).unwrap();
    assert_eq!(server.clients(), 2);
    server.send(&FromMotif::Error { message: "x".into() });
    for g in [&mut game, &mut second] {
        assert_eq!(g.recv(Duration::from_secs(5)).unwrap(), Some(FromMotif::Error { message: "x".into() }));
    }
}

#[test]
fn a_message_that_is_not_one_is_named_and_a_web_page_is_refused() {
    let server = Server::start(0, "test", || {}).unwrap();
    let port = server.port();
    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let (mut ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), stream).unwrap();
    ws.send(tungstenite::Message::text(r#"{"type":"fly"}"#)).unwrap();
    let got = wait(&server);
    assert!(matches!(&got[..], [Err(e)] if e.contains("fly")), "{got:?}");

    // A browser always says where the page came from: such a connection never opens.
    let mut req = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
    req.headers_mut().insert("Origin", "https://example.com".parse().unwrap());
    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let refused = tungstenite::client(req, stream).unwrap_err();
    assert!(refused.to_string().contains("403"), "{refused}");
}

#[test]
fn a_contract_change_is_told_by_id() {
    let before = "artboard \"Герой\" 100x80\nrect r size 10\nanim Run 1s id=a001 {\n  r.x 0: 0 | 1: 10\n  event step 0.5 id=e001\n  event dust 0.6 id=e002\n}\n\
        machine Hero {\n  input speed 0 id=i001\n  input jump trigger id=i002\n  state Run id=s001\n}\n";
    let after = before
        .replace("input speed 0", "input скорость 0")
        .replace("  event dust 0.6 id=e002\n", "")
        .replace("  state Run id=s001\n", "  state Run id=s001\n  state Jump anim Run id=s002\n");
    let contract = |src: &str| motif_script::contract::contract([&motif_script::parse(src).unwrap().doc]);
    let got: Vec<(String, Option<String>, Option<String>)> =
        changes(&contract(before), &contract(&after)).into_iter().map(|c| (c.kind, c.was, c.now)).collect();
    let s = |v: &str| Some(v.to_string());
    assert_eq!(got, [("event".to_string(), s("dust"), None), ("input".to_string(), s("speed"), s("скорость")), ("state".to_string(), None, s("Jump"))]);
    assert!(changes(&contract(before), &contract(before)).is_empty());
}
