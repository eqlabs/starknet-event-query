use time;
use tracing_subscriber::{EnvFilter, filter::LevelFilter, fmt::time::OffsetTime};

use std::collections::HashMap;

pub fn start_logger(default_level: LevelFilter) {
    let filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter
            .add_directive("alloy=off".parse().unwrap())
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap()),
        _ => EnvFilter::default()
            .add_directive(default_level.into())
            .add_directive("alloy=off".parse().unwrap())
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap()),
    };

    let timer_format = time::format_description::parse(
        "[year]-[month padding:zero]-[day padding:zero]T[hour]:[minute]:[second]",
    )
    .unwrap();
    let time_offset = time::UtcOffset::UTC;
    let timer = OffsetTime::new(time_offset, timer_format);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(timer)
        .init();
}

pub fn parse_event(raw_string: &str) -> eyre::Result<serde_json::Value> {
    let event_map: HashMap<String, serde_json::Value> =
        serde_json::from_str(raw_string)?;
    let s = serde_json::to_string(&event_map)?;
    let v: serde_json::Value = serde_json::from_str(&s)?;
    Ok(v)
}
