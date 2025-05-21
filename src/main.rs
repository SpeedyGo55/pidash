use std::{collections::HashMap, time::Duration};

// A Dashboard for my Raspberry PI which will display Component Temps, Fan speed, uptime etc.
use axum::{Json, Router, extract::Query, routing::get};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let conn = Connection::open("history.db").unwrap();
    match conn.execute(
        "CREATE TABLE IF NOT EXISTS 'values' (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        cpu_usage FLOAT,
        mem_total INTEGER,
        mem_used INTEGER,
        disk_total INTEGER,
        disk_used INTEGER,
        disk_free INTEGER,
        timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    ) {
        Ok(_) => {}
        Err(err) => {
            println!("Error creating table: {}", err);
        }
    }

    // build our application with a single route
    let app = Router::new()
        .route("/cpu_temp", get(get_cpu_temp))
        .route("/fan_speed", get(get_fan_speed))
        .route("/uptime", get(get_uptime))
        .route("/mem_usage", get(get_mem_usage))
        .route("/disk_usage", get(get_disk_usage))
        .route("/cpu_usage", get(get_cpu_usage))
        .route("/history", get(get_history));

    // spawn thread to handle database operations
    tokio::spawn(async move {
        value_logging();
        sleep(Duration::from_secs(60)).await;
    });
    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
async fn get_cpu_temp() -> Json<Value> {
    let thermal_zone = "/sys/class/thermal/thermal_zone0/temp";
    let temp = std::fs::read_to_string(thermal_zone).unwrap();
    let json = json!({
        "cpu_temp": temp.trim().parse::<i32>().unwrap()
    });
    Json(json)
}

async fn get_fan_speed() -> Json<Value> {
    let fan_speed = "/sys/devices/platform/cooling_fan/hwmon/hwmon2/fan1_input";
    let speed = std::fs::read_to_string(fan_speed).unwrap();
    let json = json!({
        "fan_speed": speed.trim().parse::<i32>().unwrap()
    });
    Json(json)
}

async fn get_uptime() -> Json<Value> {
    let uptime = "/proc/uptime";
    let uptime_str = std::fs::read_to_string(uptime).unwrap();
    let uptime_secs = uptime_str
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let uptime_millis = (uptime_secs * 1000.0).floor() as i64;
    let json = json!({
        "uptime": uptime_millis
    });
    Json(json)
}

async fn get_mem_usage() -> Json<Value> {
    let (mem_total, mem_used) = mem_usage();
    let json = json!({
        "mem_used": mem_used,
        "mem_total": mem_total,
        "mem_percent": ((mem_used as f64 / mem_total as f64 * 100.0).round() as i32)
    });
    Json(json)
}

async fn get_disk_usage() -> Json<Value> {
    let (total, used, free) = disk_usage();
    let json = json!({
        "total": total,
        "used": used,
        "free": free,
        "percent": ((used.parse::<f64>().unwrap() / total.parse::<f64>().unwrap() * 100.0).round() as i32)
    });
    Json(json)
}

async fn get_cpu_usage() -> Json<Value> {
    let cpu_usage = cpu_usage();
    let json = json!({
        "cpu_usage": cpu_usage
    });
    Json(json)
}

fn cpu_usage() -> f64 {
    let cpuinfo = "/proc/stat";
    let cpuinfo_str = std::fs::read_to_string(cpuinfo).unwrap();
    let cpu_user = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[1].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_nice = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[2].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_system = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[3].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_idle = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[4].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_iowait = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[5].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_irq = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[6].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_softirq = cpuinfo_str
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts[7].parse::<f64>().unwrap()
        })
        .sum::<f64>();
    let cpu_usage = ((cpu_user + cpu_system + cpu_iowait + cpu_irq + cpu_softirq + cpu_nice)
        / cpu_idle)
        * 100.0;
    cpu_usage
}

fn mem_usage() -> (i32, i32) {
    let meminfo = "/proc/meminfo";
    let meminfo_str = std::fs::read_to_string(meminfo).unwrap();
    let mem_total = meminfo_str
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let mem_free = meminfo_str
        .lines()
        .find(|line| line.starts_with("MemFree:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let mem_used = mem_total - mem_free;
    (mem_total, mem_used)
}
fn disk_usage() -> (String, String, String) {
    let df_output = std::process::Command::new("df").output().unwrap();
    let df_str = String::from_utf8_lossy(&df_output.stdout);
    let df_lines: Vec<&str> = df_str.lines().collect();

    // Get the root filesystem line (typically the first filesystem after headers)
    let root_line = df_lines.iter().skip(3).next().unwrap_or(&df_lines[1]);

    let parts: Vec<&str> = root_line.split_whitespace().collect();
    let total = parts[1].to_string();
    let used = parts[2].to_string();
    let free = parts[3].to_string();

    (total, used, free)
}

fn value_logging() {
    //log cpu usage and memory usage history in database
    let cpu_usage = cpu_usage();
    let mem_usage = mem_usage();
    let disk_usage = disk_usage();
    // log cpu_usage, mem_usage, and disk_usage to database
    let conn = Connection::open("history.db").unwrap();
    conn.execute(
        "INSERT INTO 'values' (cpu_usage, mem_total, mem_used, disk_total, disk_used, disk_free) VALUES (?, ?, ?, ?, ?, ?)",
        params![cpu_usage, mem_usage.0, mem_usage.1, disk_usage.0, disk_usage.1, disk_usage.2],
    ).unwrap();
}

async fn get_history(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    // Extract from and to dates from query parameters or use defaults (1970-01-01T00:00:00Z)
    let first = "1970-01-01T00:00:00Z".to_string();
    let last = "now".to_string();
    let from = params.get("from").unwrap_or(&first);
    let to = params.get("to").unwrap_or(&last);

    let conn = match Connection::open("history.db") {
        Ok(conn) => conn,
        Err(e) => {
            return Json(json!({
                "error": format!("Failed to open database: {}", e)
            }));
        }
    };

    let mut stmt = match conn.prepare("SELECT cpu_usage, mem_total, mem_used, disk_total, disk_used, disk_free, timestamp FROM 'values' WHERE timestamp BETWEEN ? AND ?") {
        Ok(stmt) => stmt,
        Err(e) => {
            return Json(json!({
                "error": format!("Failed to prepare statement: {}", e)
            }));
        }
    };

    let rows_result = stmt.query_map(params![from, to], |row| {
        Ok(json!({
            "cpu_usage": row.get::<_, f64>(0)?,
            "mem_total": row.get::<_, i32>(1)?,
            "mem_used": row.get::<_, i32>(2)?,
            "disk_total": row.get::<_, i32>(3)?,
            "disk_used": row.get::<_, i32>(4)?,
            "disk_free": row.get::<_, i32>(5)?,
            "timestamp": row.get::<_, String>(6)?,
        }))
    });

    match rows_result {
        Ok(rows) => {
            let mut values = Vec::new();
            for row in rows {
                match row {
                    Ok(value) => values.push(value),
                    Err(e) => {
                        return Json(json!({
                            "error": format!("Error processing row: {}", e)
                        }));
                    }
                }
            }
            Json(json!({ "data": values }))
        }
        Err(e) => Json(json!({
            "error": format!("Query execution failed: {}", e)
        })),
    }
}
