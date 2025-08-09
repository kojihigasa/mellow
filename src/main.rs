use axum::{
    extract::{Path, State},
    response::{sse::{Event, Sse}, Html},
    routing::get,
    Router,
};
use core::panic;
use lazy_static::lazy_static;
use prometheus::{Encoder, Gauge, TextEncoder, Registry};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration
};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use redis::{Client, Connection};
use mellow::{ROOT_HTML, CLUSTER_HTML};

const CONFIG_PATH: &str = "mellow-config.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RedisInstance {
    host: String,
    port: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RedisCluster {
    name: String,
    instances: Vec<RedisInstance>,
    password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RedisConfig {
    clusters: Vec<RedisCluster>,
}

type AppState = Arc<RedisConfig>;

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref GAUGES: Mutex<HashMap<String, Gauge>> = Mutex::new(HashMap::new());
}

fn setup_redis_client(cluster: &RedisCluster) -> Connection {
    for instance in &cluster.instances {
        let uri: String = if cluster.password.is_empty() {
            format!("redis://{}:{}", instance.host, instance.port)
        } else {
            format!("redis://default:{}@{}:{}", cluster.password, instance.host, instance.port)
        };
        if let Ok(client) = Client::open(uri) {
            if let Ok(con) = client.get_connection() {
                return con;
            }
        }
    }
    panic!("Failed to connect to any Redis instance in the cluster {}", cluster.name);
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

async fn sse_handler(
    Path(name): Path<String>,
    State(config): State<AppState>
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let cluster = config.clusters.iter()
        .find(|c| c.name == name)
        .expect(&format!("Cluster {} not found", name))
        .clone();
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(1)))
        .map(move |_| {
            let mut con: Connection = setup_redis_client(&cluster);
            let info: String = get_redis_info(&mut con);
            let info_map: HashMap<String, String> = parse_redis_info(&info);
            let info_json: String = get_json_from_info(&info_map);
            Ok(Event::default().data(info_json))
        });
    Sse::new(stream)
}

async fn metrics_handler(
    Path(name): Path<String>,
    State(config): State<AppState>
) -> String {
    let cluster = config.clusters.iter()
        .find(|c| c.name == name)
        .expect(&format!("Cluster {} not found", name));
    let mut con: Connection = setup_redis_client(cluster);
    let info: String = get_redis_info(&mut con);
    let info_map: HashMap<String, String> = parse_redis_info(&info);
    update_gauges_from_info(&info_map);
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&REGISTRY.gather(), &mut buffer)
        .expect("Failed to encode metrics");
    String::from_utf8(buffer)
        .expect("Failed to convert metrics to String")
}

async fn index_handler() -> Html<&'static str> {
    Html(ROOT_HTML)
}

async fn named_index_handler(
    Path(name): Path<String>
) -> Html<String> {
    let html = format!(
        r#"<script>window.CLUSTER_NAME = "{}";</script>{}"#,
        name, CLUSTER_HTML
    );
    Html(html)
}

#[tokio::main]
async fn main() {
    let config_data: String = std::fs::read_to_string(CONFIG_PATH)
        .expect("Failed to read config file");
    let redis_config: RedisConfig = serde_json::from_str(&config_data)
        .expect("Failed to parse config file");
    let shared_config: Arc<RedisConfig> = Arc::new(redis_config);

    let app: Router = Router::new()
        .route("/", get(index_handler))
        .route("/:name", get(named_index_handler))
        .route("/:name/events", get(sse_handler))
        .route("/:name/metrics", get(metrics_handler))
        .with_state(shared_config);

    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Listening on http://{}", addr);
    let listener: TcpListener = TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");
    axum::serve(listener, app.into_make_service())
        .await
        .expect("Failed to start server");
}
