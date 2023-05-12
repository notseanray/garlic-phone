use futures::{FutureExt, StreamExt};
use rand::{rngs::ThreadRng, seq::SliceRandom};
use std::convert::Infallible;
use std::env;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, sync::Arc};
use std::{fs::create_dir_all, path::PathBuf};
use tokio::sync::RwLock;
use tokio::{fs::remove_file, sync::Mutex};
use tokio::{process::Command, sync::mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;
use warp::Filter;
use warp::{
    ws::{Message, WebSocket},
    Rejection, Reply,
};

#[derive(Clone)]
struct User {
    id: String,
    admin: bool,
    username: String,
    prompt: String,
}

const CANVAS_HEIGHT: usize = 480;
const CANVAS_WIDTH: usize = 600;

static ROUND: AtomicUsize = AtomicUsize::new(0);

macro_rules! round {
    () => {
        ROUND.load(std::sync::atomic::Ordering::Relaxed)
    };
}

macro_rules! next {
    () => {
        let r = ROUND.load(std::sync::atomic::Ordering::Relaxed) + 1;
        ROUND.swap(r, std::sync::atomic::Ordering::Relaxed);
        r
    };
}

struct Canvas {
    colors: [String; 4],
    data: [u8; CANVAS_WIDTH * CANVAS_HEIGHT],
}

impl Default for Canvas {
    fn default() -> Self {
        let colors = [
            "#fff".to_string(),
            "#000".to_string(),
            "#ff0000".to_string(),
            "#ffa500".to_string(),
        ];
        Self {
            colors,
            data: [0; CANVAS_HEIGHT * CANVAS_WIDTH],
        }
    }
}

#[derive(Default)]
struct Game {
    // ids
    players: Vec<User>,
    data: Vec<Round>,
}

impl Game {}

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
    warp::serve(routes).run(([127, 0, 0, 1], 8069)).await;
}

pub(crate) struct WsClient {
    pub sender: Option<mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>>,
    pub authed: bool,
    // if initial canvas send is sent
    pub init: bool,
    pub id: String,
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

pub(crate) async fn client_connection(
    ws: WebSocket,
    clients: Clients,
    game_data: Arc<RwLock<Game>>,
) {
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
        id: uuid.clone(),
    };
    clients.lock().await.insert(uuid.clone(), new_client);
    if let Some(v) = clients.lock().await.get_mut(&uuid) {
        if !v.init {
            println!("init");
            v.send(
                game_data.read().await.data[0].canvases[0]
                    .data
                    .to_vec()
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>()
                    .join(" "),
            )
            .await;
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

async fn client_msg(
    client_id: &str,
    msg: Message,
    clients: &Clients,
    game_data: Arc<RwLock<Game>>,
) {
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
        // we move locking after the response once authed, looks messy but should be better, I hope
        if let Some(response) = handle_response(msg, game_data, v).await {
            if let Some(v) = locked.get(client_id) {
                if let Some(sender) = &v.sender {
                    let _ = sender.send(Ok(Message::text(response)));
                }
            }
        }
    }
}

pub(crate) async fn ws_handler(
    ws: warp::ws::Ws,
    game_data: Arc<RwLock<Game>>,
    clients: Clients,
) -> Result<impl Reply> {
    Ok(ws.on_upgrade(move |socket| client_connection(socket, clients, game_data)))
}

#[inline]
fn distance_between(point1_x: i32, point2_x: i32, point1_y: i32, point2_y: i32) -> usize {
    ((((point2_x - point1_x).pow(2) + (point2_y - point1_y).pow(2)) as f32).sqrt() + 0.5) as usize
}

#[inline]
fn angle_between(point1_x: i32, point2_x: i32, point1_y: i32, point2_y: i32) -> f32 {
    ((point2_x - point1_x) as f32).atan2((point2_y - point1_y) as f32)
}

async fn handle_response(
    message: &str,
    game_data: Arc<RwLock<Game>>,
    user: &mut WsClient,
) -> Option<String> {
    let v: Vec<&str> = message.split_whitespace().collect();
    if v.is_empty() {
        return None;
    }
    if let Some(v) = v.first() {
        match *v {
            "clear" => {
                // game_data.write().await.canvases
                // clear canvas
            }
            "next" => {
                // check if admin
            }
            "end" => {
                // yeet
            }
            "color" => {
                // set colors for this instance
            }
            _ => {}
        }
    }
    let mut v: Vec<i32> = v.iter().map(|x| x.parse().unwrap_or_default()).collect();
    if v.len() & 1 == 0 {
        return None;
    }
    if v.len() < 2 {
        return None;
        //go > rust
    }
    let r = v.pop().unwrap_or(3) / 2;
    let (Some(mut prev_y), Some(mut prev_x)) = (v.pop(), v.pop()) else {
        return None;
    };
    let round = game_data.read().await.data.len() - 1;
    let Some(player_index) = game_data.read().await.players.iter().filter(|x| x.id == user.id).collect::<Vec<_>>().first() else {
        return None;
    };
    if !(prev_y >= CANVAS_HEIGHT as i32
        || prev_y < 0
        || prev_x >= CANVAS_WIDTH as i32
        || prev_x < 0)
    {
        game_data.write().await.data[round].canvases[player_index].data[(prev_y*CANVAS_WIDTH as i32 + prev_x) as usize] = 1;
    }
    while !v.is_empty() {
        let (y, x) = (v.pop(), v.pop());
        if let (Some(x), Some(y)) = (x, y) {
            let dist = distance_between(prev_x, x, prev_y, y);
            let angle = angle_between(prev_x, x, prev_y, y);
            for i in 0..dist {
                let new_x = (prev_x as f32 + angle.sin() * i as f32 + 0.5) as i32;
                let new_y = (prev_y as f32 + angle.cos() * i as f32 + 0.5) as i32;
                for x_c in -r..r {
                    for y_c in -r..r {
                        let d = ((x_c * x_c + y_c * y_c) as f32).sqrt();
                        let new_y = new_y + y_c;
                        let new_x = new_x + x_c;
                        if d > r as f32
                            || new_y >= CANVAS_HEIGHT as i32
                            || new_y < 0
                            || new_x >= CANVAS_WIDTH as i32
                            || new_x < 0
                        {
                            continue;
                        }
                        game_data.write().await.data[round].canvases[player_index].data[(prev_y*CANVAS_WIDTH as i32 + prev_x) as usize] = 1;
                    }
                }
            }
            // game_data.write().await.canvases[0].data[y*CANVAS_WIDTH + x] = 1;
            prev_x = x;
            prev_y = y;
        }
    }
    Some("test".to_string())
}

impl Game {
    pub fn add_player(&mut self, client: &WsClient, username: String) {
        self.players.push(User {
            id: client.id.clone(),
            admin: false,
            username,
            prompt: String::default(),
        });
    }
    // game ends and starts next
    pub fn reset(&mut self) {
        self.data.clear();
    }
    pub fn start(&mut self) {
        let mut rng = ThreadRng::default();
        self.players.shuffle(&mut rng);
        self.data.push(Round {
            canvases: Vec::default(),
        });
    }
    fn setup() {
        // uuid and player data as seperate keys, need to find encryption protocol that works in
        // browser and rust
        // player join, send uuid over ws, stored in browser ls, encrypt using uuid as key
        // player join handle, insert Canvas and player data
        // rand assign people
        // push to round
    }
    // maybe no object pooling? consume self
    fn generate_gifs() {}
}

pub struct Round {
    canvases: Vec<Canvas>,
}

// move to impl
impl Round {
    pub fn new(player_count: usize) -> Self {
        Self { canvases: vec![] }
    }
}
