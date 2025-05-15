use clap::Parser;
use eyre::anyhow;
use regex::Regex;
use tracing_subscriber::filter::LevelFilter;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use starknet_event_query::util::{parse_event, start_logger};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        value_name = "fixtures",
        long_help = "Path to fixture directory",
        default_value = "ground"
    )]
    pub fixture_dir: PathBuf,
}

fn cond_count(unfiltered_rx: &Regex, fixture: PathBuf) -> eyre::Result<()> {
    let os_stem = fixture
        .file_stem()
        .ok_or_else(|| anyhow!("invalid fixture path: {:?}", fixture))?;
    let stem = os_stem
        .to_str()
        .ok_or_else(|| anyhow!("invalid fixture name: {:?}", fixture))?;
    if !unfiltered_rx.is_match(stem) {
        return Ok(());
    }

    let mut mn = 0;
    let mut mx = 0;
    let source = fs::File::open(&fixture)?;
    let reader = BufReader::new(source);
    for line in reader.lines() {
        let event = parse_event(&line?)?;
        let serde_json::Value::Array(ref keys) = event["keys"] else {
            return Err(anyhow!("unexpected keys type"));
        };

        let l = keys.len();
        if l < mn {
            mn = l;
        }
        if l > mx {
            mx = l;
        }
    }

    if mx > 0 {
        println!("{}: {}-{}", stem, mn, mx);
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
        cond_count(&unfiltered_rx, entry?)?;
    }

    Ok(())
}
