use artemis_core::{
    collectors::{block_collector::NewBlock},
};
use serde::{Deserialize};

#[derive(Debug, Clone)]
pub enum Event {
    NewBlock(NewBlock)
}

#[derive(Debug, Clone)]
pub enum Action {}

#[derive(Debug, Clone)]
pub struct Config {}

#[derive(Debug, Deserialize)]
pub struct BinanceOrdersResponse {
    pub asks: Vec<(String, String)>,
    pub bids: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
pub struct TokensPrice {
    pub usdc: f64,
    pub eth: f64,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Profit {
    pub profit: f64,
    pub amount: f64,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PriceDifference {
    pub difference_usd: f64,
    pub difference_in_percent: f64,
    pub name: String,
}