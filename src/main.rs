use clap::Parser;
use redis::Commands;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use warp::Filter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    redis_url: String,
    #[arg(short, long)]
    log_key: String,
    #[arg(last = true)]
    command: Vec<String>,
}

fn connect_to_redis(url: &str) -> redis::RedisResult<redis::Connection> {
    // connect to redis
    let client = redis::Client::open(url)?;
    client.get_connection()
}

fn save_log(
    conn: &mut redis::Connection,
    key: &str,
    label: &str,
    msg: &str,
) -> redis::RedisResult<()> {
    conn.xadd(key, "*", &[("label", label), ("msg", msg)])
}

fn get_hostname() -> String {
    let output = Command::new("hostname")
        .output()
        .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    if output.status.success() {
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    } else {
        String::from("unknown")
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct RuntimeInfo {
    hostname: String,
    pid: u32,
    up: bool,
    start_time: u64,
    last_restart: u64,
    restarts: u32,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut conn = connect_to_redis(args.redis_url.as_str()).unwrap();
    println!("Connected to redis");

    let hostname = get_hostname();
    println!("Hostname: {}", hostname);

    let label = format!("forever({})", hostname);
    save_log(
        &mut conn,
        args.log_key.as_str(),
        label.as_str(),
        format!("Starting command: {}", args.command.join(" ")).as_str(),
    )
    .unwrap();

    let runtime_info = Arc::new(RwLock::new(RuntimeInfo {
        hostname: hostname,
        pid: std::process::id(),
        up: false,
        start_time: 0,
        last_restart: 0,
        restarts: 0,
    }));

    let should_stop = Arc::new(Mutex::new(false));

    {
        let runtime_info = runtime_info.clone();
        let index = warp::get()
            .and(warp::path::end())
            .map(|| "Welcome to ForEver");
        let hello = warp::path!("info").map(move || {
            let runtime_info = runtime_info.read().unwrap();
            warp::reply::json(&*runtime_info)
        });
        let router = index.or(hello);
        tokio::spawn(async move {
            warp::serve(router).run(([127, 0, 0, 1], 3030)).await;
        });
    }

    {
        let should_stop = should_stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::signal::ctrl_c().await.unwrap();
                println!("Ctrl-c received!");
                {
                    let mut should_stop = should_stop.lock().unwrap();
                    *should_stop = true;
                }
            }
        });
    }

    loop {
        println!("Running command: {}", args.command.join(" "));
        let mut child = Command::new(&args.command[0])
            .args(&args.command[1..])
            .spawn()
            .unwrap();

        {
            let mut runtime_info = runtime_info.write().unwrap();
            runtime_info.pid = child.id();
            runtime_info.up = true;
            runtime_info.start_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;
        }

        let status = child.wait().unwrap();
        println!("Command exited with status: {}", status);
        {
            let mut runtime_info = runtime_info.write().unwrap();
            runtime_info.up = false;
            runtime_info.last_restart = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;
        }

        {
            let should_stop = should_stop.lock().unwrap();
            if *should_stop {
                break;
            }
        }

        save_log(
            &mut conn,
            args.log_key.as_str(),
            label.as_str(),
            format!("Command exited, restarting: {}", args.command.join(" ")).as_str(),
        )
        .unwrap();

        {
            let mut runtime_info = runtime_info.write().unwrap();
            runtime_info.restarts += 1;
        }
    }
}
