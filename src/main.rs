use axum::{
    response::{sse::{Event, Sse}, Html},
    routing::get,
    Router,
};
use lazy_static::lazy_static;
use prometheus::{Encoder, Gauge, TextEncoder, Registry};
use std::{collections::HashMap, net::SocketAddr, sync::Mutex, time::Duration};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use redis::Connection;
use mellow::HTML;

const URI: &str = "redis://127.0.0.1/";

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref GAUGES: Mutex<HashMap<String, Gauge>> = Mutex::new(HashMap::new());
}

fn setup_redis_client(uri: &str) -> Connection {
    let con: Connection = redis::Client::open(uri)
        .expect("Failed to create Redis client")
        .get_connection()
        .expect("Failed to connect to Redis");
    con
}

fn get_redis_info(con: &mut Connection) -> String {
    let info: String = redis::cmd("INFO").query(con)
        .expect("Failed to get Redis info");
    info
}

fn parse_redis_info(info: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for line in info.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn get_json_from_info(map: &HashMap<String, String>) -> String {
    let val = serde_json::to_value(map)
        .expect("Failed to convert Redis info to JSON");
    let json = serde_json::to_string(&val)
        .expect("Failed to convert Redis info JSON to String");
    json
}

fn update_gauges_from_info(map: &HashMap<String, String>) {
    let mut gauges = GAUGES.lock()
        .expect("Failed to lock GAUGES");
    for (key, value) in map {
        if let Ok(val) = value.parse::<f64>() {
            if let Some(gauge) = gauges.get(key) {
                gauge.set(val);
            } else {
                let gauge = Gauge::new(key, "Redis metric")
                    .expect("Failed to create gauge");
                gauge.set(val);
                REGISTRY.register(Box::new(gauge.clone()))
                    .expect("Failed to register gauge");
                gauges.insert(key.clone(), gauge);
            }
        }
    }
}

async fn sse_handler() -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(2)))
        .map(|_| {
            let mut con: Connection = setup_redis_client(URI);
            let info: String = get_redis_info(&mut con);
            let info_map: HashMap<String, String> = parse_redis_info(&info);
            let info_json: String = get_json_from_info(&info_map);
            Ok(Event::default().data(info_json))
        });
    Sse::new(stream)
}

async fn metrics_handler() -> String {
    let mut con: Connection = setup_redis_client(URI);
    let info: String = get_redis_info(&mut con);
    let info_map: HashMap<String, String> = parse_redis_info(&info);
    update_gauges_from_info(&info_map);
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&REGISTRY.gather(), &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

async fn index_handler() -> Html<&'static str> {
    Html(HTML)
}

#[tokio::main]
async fn main() {
    let app: Router = Router::new()
        .route("/", get(index_handler))
        .route("/events", get(sse_handler))
        .route("/metrics", get(metrics_handler));

    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Listening on http://{}", addr);
    let listener: TcpListener = TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");
    axum::serve(listener, app.into_make_service())
        .await
        .expect("Failed to start server");
}
