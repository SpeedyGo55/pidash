use std::{collections::HashMap, time::Duration};

// A Dashboard for my Raspberry PI which will display Component Temps, Fan speed, uptime etc.
use axum::{Json, Router, extract::Query, routing::get, extract::ConnectInfo, middleware, extract};
use axum::extract::FromRequestParts;
use axum::middleware::Next;
use axum_client_ip::{ClientIp, ClientIpSource};
use log::{error, info, trace};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tokio::time::sleep;
use tracing_subscriber::{fmt, EnvFilter};
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};
use tower::ServiceBuilder;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::TRACE.into())
                .from_env_lossy(),
        )
        .with(fmt::layer())
        .init();
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
            error!("Failed to create table: {}", err);
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
        .route("/history", get(get_history))
        .layer(TraceLayer::new_for_http())
        .layer(
            ServiceBuilder::new()
                // Hardcode IP source, look into `examples/configurable.rs` for runtime
                // configuration
                .layer(ClientIpSource::ConnectInfo.into_extension())
                // Create a request span with a placeholder for IP
                .layer(
                    TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                        info_span!(
                            "request",
                            method = %request.method(),
                            uri = %request.uri(),
                            ip = tracing::field::Empty
                        )
                    }),
                )
                // Extract IP and fill the span placeholder
                .layer(middleware::from_fn(
                    async |request: extract::Request, next: Next| {
                        let (mut parts, body) = request.into_parts();
                        if let Ok(ip) = ClientIp::from_request_parts(&mut parts, &()).await {
                            let span = Span::current();
                            span.record("ip", ip.0.to_string());
                        }
                        next.run(extract::Request::from_parts(parts, body)).await
                    },
                )),
        );

    // spawn thread to handle database operations
    tokio::spawn(async move {
        loop {
            // log cpu usage and memory usage history in database
            value_logging();
            sleep(Duration::from_secs(60)).await;
        }
    });
    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
async fn get_cpu_temp() -> Json<Value> {
    // Read CPU temperature from the thermal zone file
    trace!("Reading CPU temperature from thermal zone file");
    let thermal_zone = "/sys/class/thermal/thermal_zone0/temp";
    let temp = std::fs::read_to_string(thermal_zone);
    let temp = match temp {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to read CPU temperature: {}", e);
            return Json(json!({"error": "Failed to read CPU temperature"}));
        }
    };
    trace!("CPU temperature read successfully: {}", temp);
    let json = json!({
        "cpu_temp": temp.trim().parse::<i32>().unwrap()
    });
    Json(json)
}

async fn get_fan_speed() -> Json<Value> {
    // Read fan speed from the hardware monitor file
    trace!("Reading fan speed from hardware monitor file");
    let fan_speed = "/sys/devices/platform/cooling_fan/hwmon/hwmon2/fan1_input";
    let speed = std::fs::read_to_string(fan_speed);
    let speed = match speed {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to read fan speed: {}", e);
            return Json(json!({"error": "Failed to read fan speed"}));
        }
    };
    trace!("Fan speed read successfully: {}", speed);
    let json = json!({
        "fan_speed": speed.trim().parse::<i32>().unwrap()
    });
    Json(json)
}

async fn get_uptime() -> Json<Value> {
    // Read system uptime from the /proc/uptime file
    trace!("Reading system uptime from /proc/uptime file");
    let uptime = "/proc/uptime";
    let uptime_str = std::fs::read_to_string(uptime);
    let uptime_str = match uptime_str {
        Ok(u) => u,
        Err(e) => {
            error!("Failed to read system uptime: {}", e);
            return Json(json!({"error": "Failed to read system uptime"}));
        }
    };
    trace!("System uptime read successfully: {}", uptime_str);
    let uptime_secs = uptime_str
        .split_whitespace()
        .next();
    let uptime_secs = match uptime_secs {
        Some(u) => u.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse system uptime");
            return Json(json!({"error": "Failed to parse system uptime"}));
        }
    };
    trace!("System uptime in seconds: {}", uptime_secs);
    // Convert uptime from seconds to milliseconds
    let uptime_millis = (uptime_secs * 1000.0).floor() as i64;
    let json = json!({
        "uptime": uptime_millis
    });
    Json(json)
}

async fn get_mem_usage() -> Json<Value> {
    // Read memory usage from the /proc/meminfo file
    trace!("Fetching memory usage for http request");
    let (mem_total, mem_used) = mem_usage();
    let json = json!({
        "mem_used": mem_used,
        "mem_total": mem_total,
        "mem_percent": ((mem_used as f64 / mem_total as f64 * 100.0).round() as i32)
    });
    Json(json)
}

async fn get_disk_usage() -> Json<Value> {
    // Read disk usage from the df command output
    trace!("Fetching disk usage for http request");
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
    // Read CPU usage from the /proc/stat file
    trace!("Fetching CPU usage for http request");
    let cpu_usage = cpu_usage();
    let json = json!({
        "cpu_usage": cpu_usage
    });
    Json(json)
}

fn cpu_usage() -> f64 {
    // Read CPU usage from the /proc/stat file
    trace!("Reading CPU usage from /proc/stat file");
    let cpuinfo = "/proc/stat";
    let cpuinfo_str = std::fs::read_to_string(cpuinfo);
    let cpuinfo_str = match cpuinfo_str {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to read CPU usage: {}", e);
            return 0.0; // Return 0.0 if reading fails
        }
    };
    trace!("CPU usage read successfully: {}", cpuinfo_str);
    let cpu_user = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(1);
    let cpu_user = match cpu_user {
        Some(u) => u.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU user time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_nice = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(2);
    let cpu_nice = match cpu_nice {
        Some(n) => n.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU nice time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_system = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(3);
    let cpu_system = match cpu_system {
        Some(s) => s.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU system time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_idle = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(4);
    let cpu_idle = match cpu_idle {
        Some(i) => i.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU idle time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_iowait = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(5);
    let cpu_iowait = match cpu_iowait {
        Some(i) => i.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU iowait time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_irq = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(6);
    let cpu_irq = match cpu_irq {
        Some(i) => i.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU irq time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    let cpu_softirq = cpuinfo_str
        .lines()
        .find(|line| line.starts_with("cpu "))
        .unwrap()
        .split_whitespace()
        .nth(7);
    let cpu_softirq = match cpu_softirq {
        Some(s) => s.parse::<f64>().unwrap_or(0.0),
        None => {
            error!("Failed to parse CPU softirq time");
            return 0.0; // Return 0.0 if parsing fails
        }
    };
    trace!("CPU times - User: {}, Nice: {}, System: {}, Idle: {}, Iowait: {}, Irq: {}, Softirq: {}",
           cpu_user, cpu_nice, cpu_system, cpu_idle, cpu_iowait, cpu_irq, cpu_softirq);
    let cpu_total = cpu_user + cpu_system + cpu_iowait + cpu_irq + cpu_softirq + cpu_nice + cpu_idle;
    let cpu_usage = (cpu_total - cpu_idle) / cpu_total * 100.0;
    trace!("Calculated CPU usage: {}", cpu_usage);
    cpu_usage
}

fn mem_usage() -> (i32, i32) {
    // Read memory usage from the /proc/meminfo file
    trace!("Reading memory usage from /proc/meminfo file");
    let meminfo = "/proc/meminfo";
    let meminfo_str = std::fs::read_to_string(meminfo);
    let meminfo_str = match meminfo_str {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to read memory usage: {}", e);
            return (0, 0); // Return (0, 0) if reading fails
        }
    };
    let mem_total = meminfo_str
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .unwrap()
        .split_whitespace()
        .nth(1);
    let mem_total = match mem_total {
        Some(t) => t.parse::<i32>().unwrap_or(0),
        None => {
            error!("Failed to parse memory total");
            return (0, 0); // Return (0, 0) if parsing fails
        }
    };
    let mem_avail = meminfo_str
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))
        .unwrap()
        .split_whitespace()
        .nth(1);
    let mem_avail = match mem_avail {
        Some(a) => a.parse::<i32>().unwrap_or(0),
        None => {
            error!("Failed to parse memory available");
            return (0, 0); // Return (0, 0) if parsing fails
        }
    };
    trace!("Memory - Total: {}, Available: {}", mem_total, mem_avail);
    let mem_used = mem_total - mem_avail;
    trace!("Calculated memory usage: Used: {}, Total: {}", mem_used, mem_total);
    (mem_total, mem_used)
}
fn disk_usage() -> (String, String, String) {
    // Read disk usage from the df command output
    trace!("Running df command to get disk usage");
    let df_output = std::process::Command::new("df").output();
    let df_output = match df_output {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to run df command: {}", e);
            return ("0".to_string(), "0".to_string(), "0".to_string()); // Return (0, 0, 0) if running fails
        }
    };
    if !df_output.status.success() {
        error!("df command failed with status: {}", df_output.status);
        return ("0".to_string(), "0".to_string(), "0".to_string()); // Return (0, 0, 0) if command fails
    }
    trace!("df command executed successfully, processing output");
    let df_str = String::from_utf8_lossy(&df_output.stdout);
    let df_lines: Vec<&str> = df_str.lines().collect();

    // Get the root filesystem line (typically the first filesystem after headers)
    let root_line = df_lines.iter().skip(3).next().unwrap_or(&df_lines[1]);

    let parts: Vec<&str> = root_line.split_whitespace().collect();
    let total = parts[1].to_string();
    let used = parts[2].to_string();
    let free = parts[3].to_string();
    trace!("Disk usage - Total: {}, Used: {}, Free: {}", total, used, free);
    (total, used, free)
}

fn value_logging() {
    info!("Logging CPU and memory usage to database");
    trace!("Starting value logging process");
    //log cpu usage and memory usage history in database
    let cpu_usage = cpu_usage();
    trace!("Logging CPU usage: {}", cpu_usage);
    let mem_usage = mem_usage();
    trace!("Logging memory usage: Total: {}, Used: {}", mem_usage.0, mem_usage.1);
    let disk_usage = disk_usage();
    trace!("Logging disk usage: Total: {}, Used: {}, Free: {}", disk_usage.0, disk_usage.1, disk_usage.2);
    // log cpu_usage, mem_usage, and disk_usage to database
    let conn = Connection::open("history.db");
    let conn = match conn {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to open database: {}", e);
            return; // Exit if database connection fails
        }
    };
    let res = conn.execute(
        "INSERT INTO 'values' (cpu_usage, mem_total, mem_used, disk_total, disk_used, disk_free) VALUES (?, ?, ?, ?, ?, ?)",
        params![cpu_usage, mem_usage.0, mem_usage.1, disk_usage.0, disk_usage.1, disk_usage.2],
    );
    match res {
        Ok(_) => {
            trace!("Values logged successfully to database");
        }
        Err(e) => {
            error!("Failed to log values to database: {}", e);
        }
    }
}

async fn get_history(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    // Handle history requests with optional query parameters
    trace!("Fetching history data with parameters: {:?}", params);
    // Extract from and to dates from query parameters or use defaults (1970-01-01T00:00:00Z)
    let first = "1970-01-01T00:00:00Z".to_string();
    let last = "now".to_string();
    let from = params.get("from").unwrap_or(&first);
    let to = params.get("to").unwrap_or(&last);
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(100);

    let conn = match Connection::open("history.db") {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to open database: {}", e);
            return Json(json!({
                "error": format!("Failed to open database: {}", e)
            }));
        }
    };
    trace!("Preparing to query history data from database with from: {}, to: {}, limit: {}", from, to, limit);

    let mut stmt = match conn.prepare("SELECT cpu_usage, mem_total, mem_used, disk_total, disk_used, disk_free, timestamp FROM 'values' WHERE timestamp BETWEEN ? AND ? ORDER BY timestamp DESC LIMIT ?") {
        Ok(stmt) => stmt,
        Err(e) => {
            error!("Failed to prepare statement: {}", e);
            return Json(json!({
                "error": format!("Failed to prepare statement: {}", e)
            }));
        }
    };
    trace!("Executing query with parameters: from: {}, to: {}, limit: {}", from, to, limit);

    let rows_result = stmt.query_map(params![from, to, limit], |row| {
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

    trace!("Query executed, processing results");

    match rows_result {
        Ok(rows) => {
            let mut values = Vec::new();
            for row in rows {
                match row {
                    Ok(value) => values.push(value),
                    Err(e) => {
                        error!("Error processing row: {}", e);
                        return Json(json!({
                            "error": format!("Error processing row: {}", e)
                        }));
                    }
                }
            }
            trace!("Successfully processed {} rows", values.len());
            Json(json!({ "data": values }))
        }
        Err(e) => {
            error!("Query execution failed: {}", e);
            Json(json!({
            "error": format!("Query execution failed: {}", e)
        }))
        },
    }
}
