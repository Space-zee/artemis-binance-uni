use anyhow::Result;
use clap::Parser;
use binance_uni::types::{Action, Event, Config};
use ethers::providers::{Provider, Ws};
use tracing::{info};
use artemis_core::collectors::block_collector::BlockCollector;
use std::sync::Arc;
use binance_uni::strategy::BinanceUni;
use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap};

/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    /// Ethereum node WS endpoint.
    #[arg(long)]
    pub wss: String,
}

#[tokio::main]
async fn main() -> Result<()> {
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