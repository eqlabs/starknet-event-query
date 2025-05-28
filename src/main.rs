use clap::Parser;
use eyre::anyhow;
use pretty_assertions_sorted::assert_eq;
use starknet::{
    core::types::{BlockId, EventFilter},
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

use starknet_event_query::{
    config::Cli,
    filter_seed::FilterSeed,
    util::{parse_event, start_logger},
};

async fn check_fixture(provider: &impl Provider, fixture: PathBuf) -> eyre::Result<()> {
    let filter_seed = FilterSeed::load(&fixture)?;
    let (address, keys) = filter_seed.get_filter_address_and_keys(&fixture)?;
    let filter = EventFilter {
        from_block: Some(BlockId::Number(filter_seed.from_block)),
        to_block: Some(BlockId::Number(filter_seed.to_block)),
        address,
        keys,
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
