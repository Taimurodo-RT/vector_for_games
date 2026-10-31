//! The two ends of the link: Motif's [`Server`] and the game's [`Client`].

use crate::{FromMotif, PROTOCOL, ToMotif};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket};

/// How long a connection waits for a message before it looks at what it has to send.
const TICK: Duration = Duration::from_millis(20);

/// Motif's end: listens on `127.0.0.1` only, for any number of games at once.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    incoming: Receiver<Result<ToMotif, String>>,
    clients: Arc<Mutex<Vec<Sender<String>>>>,
}

impl Server {
    /// Starts listening on `port` (0: any free one). `version` goes to every game in `hello`;
    /// `wake` is called from the link's threads whenever a message arrives, so a window can draw
    /// again and [`poll`](Self::poll) it.
    pub fn start(port: u16, version: &str, wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let clients: Arc<Mutex<Vec<Sender<String>>>> = Arc::default();
        let (tx, incoming) = channel();
        let hello = FromMotif::Hello { app: "motif".into(), version: version.into(), protocol: PROTOCOL }.to_json();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        {
            let (stop, clients) = (stop.clone(), clients.clone());
            std::thread::Builder::new().name("motif-link".into()).spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let (out_tx, out_rx) = channel();
                            out_tx.send(hello.clone()).ok();
                            clients.lock().expect("clients").push(out_tx);
                            let (stop, tx, wake) = (stop.clone(), tx.clone(), wake.clone());
                            std::thread::Builder::new().name("motif-link-client".into()).spawn(move || serve(stream, out_rx, tx, stop, wake)).ok();
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(TICK),
                        Err(_) => std::thread::sleep(TICK),
                    }
                }
            })?;
        }
        Ok(Server { port, stop, incoming, clients })
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The messages that arrived since the last call, in order; a message that is not one of
    /// [`ToMotif`] comes as the reason it could not be read.
    pub fn poll(&self) -> Vec<Result<ToMotif, String>> {
        self.incoming.try_iter().collect()
    }

    /// Sends `message` to every game connected.
    pub fn send(&self, message: &FromMotif) {
        let text = message.to_json();
        self.clients.lock().expect("clients").retain(|c| c.send(text.clone()).is_ok());
    }

    /// How many games are connected.
    pub fn clients(&self) -> usize {
        let mut clients = self.clients.lock().expect("clients");
        // A connection that ended dropped its receiver: an empty string finds it out.
        clients.retain(|c| c.send(String::new()).is_ok());
        clients.len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// One game's connection: refuses a browser (a web page must not drive Motif), then passes
/// messages both ways until either side closes or the server stops.
fn serve(stream: TcpStream, out: Receiver<String>, tx: Sender<Result<ToMotif, String>>, stop: Arc<AtomicBool>, wake: Arc<dyn Fn() + Send + Sync>) {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    // The error type is tungstenite's own.
    #[allow(clippy::result_large_err)]
    let no_browser = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        if req.headers().contains_key("origin") {
            let mut refuse = ErrorResponse::new(Some("a web page cannot connect to Motif".into()));
            *refuse.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
            return Err(refuse);
        }
        Ok(resp)
    };
    let Ok(mut ws) = tungstenite::accept_hdr(stream, no_browser) else { return };
    ws.get_ref().set_read_timeout(Some(TICK)).ok();
    while !stop.load(Ordering::Relaxed) {
        for text in out.try_iter().filter(|t| !t.is_empty()) {
            if ws.send(Message::text(text)).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                tx.send(serde_json::from_str::<ToMotif>(&text).map_err(|e| format!("сообщение не читается ({e}): {text}"))).ok();
                wake();
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
    ws.close(None).ok();
    ws.flush().ok();
}

/// The game's end, for a game in Rust (Forge) and for tests: connects to Motif on this computer.
pub struct Client {
    ws: WebSocket<TcpStream>,
    /// Motif's `hello`.
    pub hello: FromMotif,
}

impl Client {
    pub fn connect(port: u16) -> Result<Client, String> {
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("Motif не слушает порт {port}: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let (ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), stream).map_err(|e| format!("Motif не ответил: {e}"))?;
        let mut client = Client { ws, hello: FromMotif::Error { message: String::new() } };
        client.hello = client.recv(Duration::from_secs(5))?.ok_or("Motif не поздоровался")?;
        Ok(client)
    }

    pub fn send(&mut self, message: &ToMotif) -> Result<(), String> {
        self.ws.send(Message::text(message.to_json())).map_err(|e| e.to_string())
    }

    /// The next message from Motif, waiting at most `wait`; none when nothing came.
    pub fn recv(&mut self, wait: Duration) -> Result<Option<FromMotif>, String> {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.ws.get_ref().set_read_timeout(Some(left.min(TICK))).ok();
            match self.ws.read() {
                Ok(Message::Text(text)) => return serde_json::from_str(&text).map(Some).map_err(|e| format!("{e}: {text}")),
                Ok(Message::Close(_)) => return Err("Motif закрыл связь".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}
