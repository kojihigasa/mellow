use std::collections::HashMap;

use redis::Connection;

const URI: &str = "redis://127.0.0.1/";

fn setup_redis_client(uri: &str) -> Connection {
    let con = redis::Client::open(uri)
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
    let mut map = HashMap::new();
    for line in info.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn main() {
    let mut con: Connection = setup_redis_client(URI);
    let info: String = get_redis_info(&mut con);
    let info_map: HashMap<String, String> = parse_redis_info(&info);
    println!("{:#?}", info_map);
}
