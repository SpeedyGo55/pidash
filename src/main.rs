// A Dashboard for my Raspberry PI which will display Component Temps, Fan speed, uptime etc.

use axum::{Router, routing::get};

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route("/cpu_temp", get(get_cpu_temp))
        .route("/fan_speed", get(get_fan_speed))
        .route("/uptime", get(get_uptime));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn get_cpu_temp() -> String {
    let thermal_zone = "/sys/class/thermal/thermal_zone0/temp";
    let temp = std::fs::read_to_string(thermal_zone).unwrap();
    temp.trim().parse::<i32>().unwrap().to_string()
}

async fn get_fan_speed() -> String {
    let fan_speed = "/sys/devices/platform/cooling_fan/hwmon/hwmon2/fan1_input";
    let speed = std::fs::read_to_string(fan_speed).unwrap();
    speed.trim().parse::<i32>().unwrap().to_string()
}

async fn get_uptime() -> String {
    let uptime = "/proc/uptime";
    let uptime_str = std::fs::read_to_string(uptime).unwrap();
    let uptime_secs = uptime_str
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let uptime_millis = (uptime_secs * 1000.0).floor() as i64;
    uptime_millis.to_string()
}

asyn
