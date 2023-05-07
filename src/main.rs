use warp::Filter;
use std::convert::Infallible;
use std::fs::File;
use std::{collections::HashMap, sync::Arc};
use std::io::Write;
use futures::{FutureExt, StreamExt};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::{fs::remove_file, sync::Mutex};
use tokio::{process::Command, sync::mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;
use warp::{
    ws::{Message, WebSocket},
    Rejection,
    Reply,
};
use std::{fs::create_dir_all, path::PathBuf};


struct User {
    id: u64,
    username: String,
}

const CANVAS_HEIGHT: usize = 480;
const CANVAS_WIDTH: usize = 600;

struct Canvas {
    round: u8,
    colors: [String; 4],
    data: [u8; CANVAS_WIDTH * CANVAS_HEIGHT],
}

impl Default for Canvas {
    fn default() -> Self {
        let colors = ["#fff".to_string(), "#000".to_string(), "#ff0000".to_string(), "#ffa500".to_string()];
        Self { round: 0, colors, data: [0; CANVAS_HEIGHT * CANVAS_WIDTH] }
    }
}

struct Game {
    // ids
    players: Vec<User>,
    canvases: Vec<Canvas>,
}

impl Game {
}

impl Default for Game {
    fn default() -> Self {
        Self {
            players: vec![],
            canvases: vec![Canvas::default()],
        }
    }
}

const HOST: &str = "127.0.0.1";
const PORT: u16 = 8069;

/*
 * users have an id, connected to username
 *
 *
 */

fn with_clients(clients: Clients) -> impl Filter<Extract = (Clients,), Error = Infallible> + Clone {
    warp::any().map(move || clients.clone())
}

#[tokio::main]
async fn main() {
    let clients = Arc::new(Mutex::new(HashMap::new()));
    let game = Arc::new(RwLock::new(Game::default()));
    let game_data = warp::any().map(move || game.clone());
    let ws_route = warp::path("garlic")
        .and(warp::ws())
        .and(game_data)
        .and(with_clients(clients.clone()))
        .and_then(ws_handler);
    let routes = ws_route.with(warp::cors().allow_any_origin());
    warp::serve(routes).run(([127,0,0,1], 8069)).await;
}

pub(crate) struct WsClient {
    pub sender: Option<mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>>,
    pub authed: bool,
    // if initial canvas send is sent
    pub init: bool,
}

impl WsClient {
    pub(crate) async fn send<'a, T: Into<String>>(&self, msg: T) {
        if let Some(v) = &self.sender {
            let _ = v.send(Ok(Message::text(msg)));
        }
    }
}

pub(crate) type Clients = Arc<Mutex<HashMap<String, WsClient>>>;
pub(crate) type Result<T> = std::result::Result<T, Rejection>;

pub(crate) async fn client_connection(ws: WebSocket, clients: Clients, game_data: Arc<RwLock<Game>>) {
    println!("*info: establishing new client connection...");
    let (client_ws_sender, mut client_ws_rcv) = ws.split();
    let (client_sender, client_rcv) = mpsc::unbounded_channel();
    let client_rcv = UnboundedReceiverStream::new(client_rcv);
    tokio::task::spawn(client_rcv.forward(client_ws_sender).map(|result| {
        if let Err(e) = result {
            println!("warn");
            // warn
        }
    }));
    let uuid = Uuid::new_v4().to_simple().to_string();
    let new_client = WsClient {
        sender: Some(client_sender),
        // fix
        authed: true,
        init: false,
    };
    clients.lock().await.insert(uuid.clone(), new_client);
    if let Some(v) = clients.lock().await.get_mut(&uuid) {
        if !v.init {
            println!("init");
            v.send(game_data.read().await.canvases[0].data.to_vec().iter().map(|x| x.to_string()).collect::<Vec<String>>().join(" ")).await;
            v.init = true;
        }
    }
    while let Some(result) = client_ws_rcv.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                //warn
                println!("warn2");
                break;
            }
        };
        client_msg(&uuid, msg, &clients, game_data.clone()).await;
    }
    clients.lock().await.remove(&uuid);
    // info
    println!("dis");
}

const PASS: &str = "test";

async fn client_msg(client_id: &str, msg: Message, clients: &Clients, game_data: Arc<RwLock<Game>>) {
    let msg = match msg.to_str() {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut locked = clients.lock().await;
    if let Some(mut v) = locked.get_mut(client_id) {
        if !v.authed {
            v.authed = {
                if PASS.len() == msg.len() {
                    let mut result = 0;
                    for (x, y) in PASS.chars().zip(msg.chars()) {
                        result |= x as u32 ^ y as u32;
                    }
                    result == 0
                } else {
                    false
                }
            };
            return;
        }
    }
    // we move locking after the response once authed, looks messy but should be better, I hope
    if let Some(response) = handle_response(msg, game_data).await {
        if let Some(v) = locked.get(client_id) {
            if let Some(sender) = &v.sender {
                let _ = sender.send(Ok(Message::text(response)));
            }
        }
    }
}

pub(crate) async fn ws_handler(ws: warp::ws::Ws, game_data: Arc<RwLock<Game>>, clients: Clients) -> Result<impl Reply> {
    Ok(ws.on_upgrade(move |socket| client_connection(socket, clients, game_data)))
}


async fn handle_response(message: &str, game_data: Arc<RwLock<Game>>) -> Option<String> {
    let mut v: Vec<usize> = message.split_whitespace().map(|x| x.parse().unwrap_or_default()).collect();
    if v.len() & 1 == 1 {
        return None;
    }
    while v.len() >= 2 {
        let (y, x) = (v.pop(), v.pop());
        if let (Some(x), Some(y)) = (x, y) {
            game_data.write().await.canvases[0].data[y*CANVAS_WIDTH + x] = 1;
        }
    }
    Some("test".to_string())
}
