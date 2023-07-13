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
pub struct BinancePriceResponse {
    pub bidPrice: String,
    pub bidQty: String,
    pub askPrice: String,
    pub askQty: String,
}

#[derive(Debug, Deserialize)]
pub struct TokensPrice {
    pub usdc: f32,
    pub eth: f32,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PriceDifference {
    pub difference_usd: f32,
    pub difference_in_percent: f32,
    pub name: String,
}