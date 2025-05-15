use clap::Parser;
use eyre::anyhow;
use pretty_assertions_sorted::assert_eq;
use starknet::{
    core::types::{BlockId, EventFilter, Felt},
    providers::{
        Provider, Url,
        jsonrpc::{HttpTransport, JsonRpcClient},
    },
};
use tracing_subscriber::filter::LevelFilter;

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;

use starknet_event_query::util::{parse_event, start_logger};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        value_name = "url",
        long_help = "Server URL",
        default_value = "http://127.0.0.1:9545"
    )]
    pub pathfinder_rpc_url: String,
    #[arg(
        long,
        value_name = "fixtures",
        long_help = "Path to fixture directory",
        default_value = "ground"
    )]
    pub fixture_dir: PathBuf,
}

struct FilterSeed {
    pub from_block: u64,
    pub to_block: u64,
    pub with_name: Option<String>,
}

impl FilterSeed {
    pub fn from_stem(stem: &str) -> eyre::Result<Self> {
        let ret = match stem.find('+') {
            Some(pos) => {
                let from_block = str::parse::<u64>(&stem[..pos])
                    .map_err(|_| anyhow!("from block not a number"))?;
                let (block_count, with_name) = Self::parse_tail(&stem[pos + 1..])?;
                let to_block = from_block
                    .checked_add(block_count)
                    .ok_or_else(|| anyhow!("adding block count overflows"))?;
                Self {
                    from_block,
                    to_block,
                    with_name,
                }
            }
            None => {
                let (from_block, with_name) = Self::parse_tail(stem)?;
                Self {
                    from_block,
                    to_block: from_block,
                    with_name,
                }
            }
        };
        Ok(ret)
    }

    pub fn format_filter_basename(&self) -> Option<String> {
        if let Some(with_name) = &self.with_name {
            let head = if self.from_block == self.to_block {
                self.from_block.to_string()
            } else {
                format!("{}+{}", self.from_block, self.to_block - self.from_block)
            };

            Some(format!("{}f{}.json", head, with_name))
        } else {
            None
        }
    }

    fn parse_tail(tail: &str) -> eyre::Result<(u64, Option<String>)> {
        let pair = match tail.find('w') {
            Some(pos) => {
                let n = str::parse::<u64>(&tail[..pos])
                    .map_err(|_| anyhow!("stem tail doesn't start with a number"))?;
                let s = tail[pos + 1..].to_string();
                (n, Some(s))
            }
            None => {
                let n = str::parse::<u64>(tail).map_err(|_| anyhow!("stem tail not a number"))?;
                (n, None)
            }
        };
        Ok(pair)
    }
}

async fn check_fixture(provider: &impl Provider, fixture: PathBuf) -> eyre::Result<()> {
    let os_stem = fixture
        .file_stem()
        .ok_or_else(|| anyhow!("invalid fixture path: {:?}", fixture))?;
    let stem = os_stem
        .to_str()
        .ok_or_else(|| anyhow!("invalid fixture name: {:?}", fixture))?;
    let filter_seed = FilterSeed::from_stem(stem)?;
    let raw_address = if let Some(basename) = filter_seed.format_filter_basename() {
        let fixture_dir = fixture
            .parent()
            .ok_or_else(|| anyhow!("fixture without path: {:?}", fixture))?;
        let filter_path = fixture_dir.join(basename);
        let contents = fs::read_to_string(filter_path)?;
        let filter_map: HashMap<String, serde_json::Value> = serde_json::from_str(&contents)?;
        if let serde_json::Value::String(ref addr) = filter_map["address"] {
            Some(addr.clone())
        } else {
            None
        }
    } else {
        None
    };
    let address = match raw_address {
        Some(s) => {
            let tail = s.strip_prefix("0x").unwrap_or(&s);
            let addr = format!("{:0>64}", tail);
            let buf = hex::decode(addr)?;
            let slice = buf.as_slice();
            let arr: [u8; 32] = slice.try_into()?;
            Some(Felt::from_bytes_be(&arr))
        }
        None => None,
    };

    let filter = EventFilter {
        from_block: Some(BlockId::Number(filter_seed.from_block)),
        to_block: Some(BlockId::Number(filter_seed.to_block)),
        address,
        keys: None,
    };
    let mut token = None;
    let mut destination = tempfile::tempfile()?;
    let mut actual_count = 0;
    let mut page_count = 0;
    loop {
        let page = provider.get_events(filter.clone(), token, 1024).await?;
        page_count += 1;
        for event in page.events {
            let raw_string = serde_json::to_string(&event)?;
            let mut event_map: HashMap<String, serde_json::Value> =
                serde_json::from_str(&raw_string)?;
            event_map.remove("block_hash");
            let s = serde_json::to_string(&event_map)?;
            let v: serde_json::Value = serde_json::from_str(&s)?;
            writeln!(&mut destination, "{}", v)?;
            actual_count += 1;
        }

        token = page.continuation_token;
        if token.is_none() {
            break;
        }
    }

    tracing::debug!("retrieved {} events in {} pages", actual_count, page_count);

    destination.seek(SeekFrom::Start(0))?;
    let actual_reader = BufReader::new(destination);
    let source = fs::File::open(fixture)?;
    let expected_reader = BufReader::new(source);
    for (actual_line, expected_line) in actual_reader.lines().zip(expected_reader.lines()) {
        let actual_event = parse_event(&actual_line?)?;
        let expected_event = parse_event(&expected_line?)?;
        assert_eq!(actual_event, expected_event);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    start_logger(LevelFilter::INFO);

    let cli = Cli::parse();
    let rpc_url: Url = cli.pathfinder_rpc_url.parse()?;
    let provider = JsonRpcClient::new(HttpTransport::new(rpc_url));
    let mask_path = cli.fixture_dir.join("*.jsonl");
    let path_str = mask_path
        .to_str()
        .ok_or_else(|| anyhow!("invalid fixture dir: {:?}", cli.fixture_dir))?;
    for entry in glob::glob(path_str)? {
        check_fixture(&provider, entry?).await?;
    }

    Ok(())
}
