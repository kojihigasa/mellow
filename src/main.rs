use axum::{
    response::sse::{Event, Sse},
    routing::get,
    Router,
};
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use redis::Connection;

const URI: &str = "redis://127.0.0.1/";

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

fn parse_redis_info(info: &str) -> String {
    let mut map: HashMap<String, String> = HashMap::new();
    for line in info.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    let json = serde_json::to_value(map)
        .expect("Failed to convert Redis info to JSON");
    serde_json::to_string(&json)
        .expect("Failed to convert Redis info JSON to String")
}

async fn sse_handler() -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(2)))
        .map(|_| {
            let mut con: Connection = setup_redis_client(URI);
            let info: String = get_redis_info(&mut con);
            let info_json: String = parse_redis_info(&info);
            Ok(Event::default().data(info_json))
        });
    Sse::new(stream)
}

#[tokio::main]
async fn main() {
    let app: Router = Router::new()
        .route("/", get(sse_handler));

    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Listening on http://{}", addr);
    let listener: TcpListener = TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");
    axum::serve(listener, app.into_make_service())
        .await
        .expect("Failed to start server");
}
