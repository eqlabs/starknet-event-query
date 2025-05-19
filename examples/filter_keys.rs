use clap::Parser;
use eyre::anyhow;
use regex::Regex;
use serde_json::json;
use tracing_subscriber::filter::LevelFilter;

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use starknet_event_query::util::{parse_event, start_logger};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        short = 'u',
        long_help = "Filter for keys in any order",
        default_value = "false"
    )]
    pub unordered: bool,
    #[arg(
        long,
        value_name = "fixtures",
        long_help = "Path to fixture directory",
        default_value = "ground"
    )]
    pub fixture_dir: PathBuf,
}

fn cond_refract(cli: &Cli, unfiltered_rx: &Regex, fixture: PathBuf) -> eyre::Result<()> {
    let os_stem = fixture
        .file_stem()
        .ok_or_else(|| anyhow!("invalid fixture path: {:?}", fixture))?;
    let stem = os_stem
        .to_str()
        .ok_or_else(|| anyhow!("invalid fixture name: {:?}", fixture))?;
    if !unfiltered_rx.is_match(stem) {
        return Ok(());
    }

    let mut known_keys = HashSet::new();
    let mut events: Vec<serde_json::Value> = Vec::new();
    let source = fs::File::open(&fixture)?;
    let reader = BufReader::new(source);
    for line in reader.lines() {
        let event = parse_event(&line?)?;
        let serde_json::Value::Array(ref keys) = event["keys"] else {
            return Err(anyhow!("unexpected keys type"));
        };

        if !keys.is_empty() {
            let canon_keys = if !cli.unordered {
                keys.clone()
            } else {
                let mut str_keys = Vec::new();
                for v in keys {
                    if let serde_json::Value::String(k) = v {
                        str_keys.push(k);
                    } else {
                        return Err(anyhow!("unexpected key type"));
                    }
                }
                str_keys.sort();
                str_keys
                    .iter()
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect()
            };
            known_keys.insert(canon_keys);
            events.push(event);
        }
    }

    for (index, keys) in known_keys.into_iter().enumerate() {
        let filter_no = index + 1;
        let filter_name = format!("{}f{}.json", stem, filter_no);
        let filter_path = cli.fixture_dir.join(filter_name);
        let filter_keys: Vec<Vec<serde_json::Value>> = if !cli.unordered {
            keys.iter().map(|k| vec![k.clone()]).collect()
        } else {
            (0..keys.len()).map(|_| keys.clone()).collect()
        };
        let filter_json = json!({
            "keys": filter_keys
        });
        fs::write(filter_path, filter_json.to_string())?;

        let output_name = format!("{}w{}.jsonl", stem, filter_no);
        let output_path = cli.fixture_dir.join(output_name);
        let mut output_file = fs::File::create(&output_path)?;
        for event in events.iter() {
            let serde_json::Value::Array(ref event_keys) = event["keys"] else {
                return Err(anyhow!("unexpected event keys type"));
            };

            let accept = if !cli.unordered {
                event_keys.starts_with(&keys)
            } else {
                let event_key_set: HashSet<serde_json::Value> =
                    event_keys.iter().cloned().collect();
                keys.iter().all(|k| event_key_set.contains(k))
            };
            if accept {
                writeln!(&mut output_file, "{}", event)?;
            }
        }
    }

    Ok(())
}

fn main() -> eyre::Result<()> {
    start_logger(LevelFilter::INFO);
    let cli = Cli::parse();

    let mask_path = cli.fixture_dir.join("*.jsonl");
    let path_str = mask_path
        .to_str()
        .ok_or_else(|| anyhow!("invalid fixture dir: {:?}", cli.fixture_dir))?;
    let unfiltered_rx = Regex::new("^([0-9]+)(?:[+]([1-9][0-9]*))?$").unwrap();
    for entry in glob::glob(path_str)? {
        cond_refract(&cli, &unfiltered_rx, entry?)?;
    }

    Ok(())
}
