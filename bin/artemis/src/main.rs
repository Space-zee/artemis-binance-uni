use anyhow::Result;
use clap::Parser;
use binance_uni::types::{Action, Event};
use ethers::providers::{Provider, Ws};
use artemis_core::collectors::block_collector::BlockCollector;
use std::sync::Arc;
use binance_uni::strategy::BinanceUni;
use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap};
use tracing::{info};
use tracing_subscriber::{prelude::*};

/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    /// Ethereum node WS endpoint.
    #[arg(long)]
    pub wss: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let logfile = tracing_appender::rolling::never("../../logs", "binance_uni.log");
// Log `INFO` and above to stdout.
    let stdout = std::io::stdout.with_max_level(tracing::Level::INFO);

    tracing_subscriber::fmt()
        // Combine the stdout and log file `MakeWriter`s into one
        // `MakeWriter` that writes to both
        .with_writer(stdout.and(logfile))
        .init();

    let args = Args::parse();

    // Set up ethers provider.
    let ws = Ws::connect(args.wss).await?;
    let provider = Provider::new(ws);


    let provider = Arc::new(provider);

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();
    // Set up block collector.
    let block_collector = Box::new(BlockCollector::new(provider.clone()));
    let block_collector = CollectorMap::new(block_collector, Event::NewBlock);
    engine.add_collector(Box::new(block_collector));

    let strategy = BinanceUni::new(Arc::new(provider.clone()));
    engine.add_strategy(Box::new(strategy));

    // Start engine.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("res: {:?}", res);
        }
    }
    Ok(())
}