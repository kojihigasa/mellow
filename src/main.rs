use axum::{
    extract::{Path, State},
    Json,
    response::{sse::{Event, Sse}, Html},
    routing::get,
    Router,
};
use core::panic;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use redis::{Client, Connection};
use mellow::{ROOT_HTML, CLUSTER_HTML};

const CONFIG_PATH: &str = "mellow-config.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RedisInstance {
    ip: String,
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

fn setup_redis_client(cluster: &RedisCluster) -> Connection {
    for instance in &cluster.instances {
        let uri: String = if cluster.password.is_empty() {
            format!("redis://{}:{}", instance.ip, instance.port)
        } else {
            format!("redis://default:{}@{}:{}", cluster.password, instance.ip, instance.port)
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

fn get_replicas(info_map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut replicas: Vec<(String, String)> = Vec::new();
    for i in 0..5 {
        if let Some(replica) = info_map.get(&format!("slave{}", i)) {
            // slave$i=ip, port, state, offset, lag
            let mut ip = None;
            let mut port = None;
            for item in replica.split(',') {
                if let Some((k, v)) = item.split_once('=') {
                    match k {
                        "ip" => ip = Some(v.to_string()),
                        "port" => port = Some(v.to_string()),
                        _ => {}
                    }
                }
            }
            if let (Some(ip), Some(port)) = (ip, port) {
                replicas.push((ip, port));
            }
        }
    }
    replicas
}

fn get_cluster_masters(con: &mut Connection) -> Vec<(String, String)> {
    let mut masters: Vec<(String, String)> = Vec::new();
    let nodes: String = redis::cmd("CLUSTER")
        .arg("NODES")
        .query(con)
        .expect("Failed to get cluster nodes");
    // id ip:port@cport master - ping-sent pong-recv config-epoch link-state slots
    nodes.lines().filter_map(|line| {
        let items: Vec<&str> = line.split_whitespace().collect();
        if items.len() > 2 && items[2].contains("master") {
            let addr: Vec<&str> = items[1].split('@').collect();
            if addr.len() == 2 {
                let ip_port: Vec<&str> = addr[0].split(':').collect();
                if ip_port.len() == 2 {
                    return Some((ip_port[0].to_string(), ip_port[1].to_string()));
                }
            }
        }
        None
    }).for_each(|(ip, port)| {
        masters.push((ip, port));
    });
    masters
}

fn generics_handler<F, R>(
    name: String,
    config: &RedisConfig,
    mut node_callback: F,
) -> R
where
    F: FnMut(&RedisCluster, &HashMap<String, String>, &str) -> R,
    R: Default + Clone,
{
    let cluster = config.clusters.iter()
        .find(|c| c.name == name)
        .expect(&format!("Cluster {} not found", name))
        .clone();

    let mut con: Connection = setup_redis_client(&cluster);
    let info: String = get_redis_info(&mut con);
    let info_map: HashMap<String, String> = parse_redis_info(&info);

    let cluster_enabled: bool = info_map.get("cluster_enabled")
        .map(|v| v == "1").unwrap_or(false);

    let mut result = R::default();

    if cluster_enabled {
        for (ip, port) in get_cluster_masters(&mut con) {
            let master_cluster = RedisCluster {
                name: cluster.name.clone(),
                instances: vec![RedisInstance { ip: ip.clone(), port: port.clone() }],
                password: cluster.password.clone(),
            };
            if let Ok(mut master_con) = std::panic::catch_unwind(|| setup_redis_client(&master_cluster)) {
                let master_info: String = get_redis_info(&mut master_con);
                let master_info_map: HashMap<String, String> = parse_redis_info(&master_info);
                result = node_callback(&master_cluster, &master_info_map, &ip);
                collect_replica_info_callback(&master_cluster, &master_info_map, &mut node_callback);
            }
        }
    } else {
        let role: &str = info_map.get("role").map(|v| v.as_str()).unwrap();
        if role == "master" {
            let ip: String = cluster.instances[0].ip.clone();
            result = node_callback(&cluster, &info_map, &ip);
            collect_replica_info_callback(&cluster, &info_map, &mut node_callback);
        } else if role == "slave" {
            let master_ip: String = info_map.get("master_host")
                .cloned().unwrap_or_default();
            let master_port: String = info_map.get("master_port")
                .cloned().unwrap_or_default();
            if !master_ip.is_empty() && !master_port.is_empty() {
                let master_cluster = RedisCluster {
                    name: cluster.name.clone(),
                    instances: vec![RedisInstance { ip: master_ip.clone(), port: master_port.clone() }],
                    password: cluster.password.clone(),
                };
                if let Ok(mut master_con) = std::panic::catch_unwind(|| setup_redis_client(&master_cluster)) {
                    let master_info: String = get_redis_info(&mut master_con);
                    let master_info_map: HashMap<String, String> = parse_redis_info(&master_info);
                    result = node_callback(&master_cluster, &master_info_map, &master_ip);
                    collect_replica_info_callback(&master_cluster, &master_info_map, &mut node_callback);
                }
            }
        }
    }
    result
}

fn collect_replica_info_callback<F, R>(
    cluster: &RedisCluster,
    info_map: &HashMap<String, String>,
    node_callback: &mut F,
) where
    F: FnMut(&RedisCluster, &HashMap<String, String>, &str) -> R,
    R: Default + Clone,
{
    if info_map.get("connected_slaves")
        .and_then(|v| v.parse::<u32>().ok()) > Some(0) {
        for (rip, rport) in get_replicas(info_map) {
            let replica_cluster = RedisCluster {
                name: cluster.name.clone(),
                instances: vec![RedisInstance { ip: rip.clone(), port: rport.clone() }],
                password: cluster.password.clone(),
            };
            if let Ok(mut replica_con) = std::panic::catch_unwind(|| setup_redis_client(&replica_cluster)) {
                let replica_info: String = get_redis_info(&mut replica_con);
                let replica_info_map: HashMap<String, String> = parse_redis_info(&replica_info);
                node_callback(&replica_cluster, &replica_info_map, &rip);
            }
        }
    }
}

async fn sse_handler(
    Path(name): Path<String>,
    State(config): State<AppState>
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let config = config.clone();
    let name = name.clone();
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(1)))
        .map(move |_| {
            let mut data: Vec<HashMap<String, String>> = Vec::new();
            generics_handler(
                name.clone(),
                &config,
                |_, info_map, ip| {
                    let mut node_info = info_map.clone();
                    node_info.insert("ip".to_string(), ip.to_string());
                    data.push(node_info);
                    ()
                },
            );
            Ok(Event::default().data(serde_json::to_string(&data)
                .expect("Failed to serialize data to JSON")))
        });
    Sse::new(stream)
}

async fn index_handler() -> Html<&'static str> {
    Html(ROOT_HTML)
}

async fn clusters_json_handler(
    State(config): State<AppState>
) -> Json<serde_json::Value> {
    let mut names : Vec<String> = config.clusters.iter()
        .map(|c| c.name.clone())
        .collect();
    names.sort();
    let clusters: Vec<serde_json::Value> = names
        .into_iter()
        .map(|name| json!({ "name": name }))
        .collect();
    Json(json!({ "clusters": clusters }))
}

async fn named_index_handler(
    Path(name): Path<String>
) -> Html<String> {
    let html: String = format!(
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
        .route("/clusters.json", get(clusters_json_handler))
        .route("/:name", get(named_index_handler))
        .route("/:name/events", get(sse_handler))
        //.route("/:name/metrics", get(metrics_handler))
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
