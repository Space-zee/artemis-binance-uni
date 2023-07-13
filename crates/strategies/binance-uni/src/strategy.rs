use super::types::{Action, Event, BinancePriceResponse, TokensPrice, PriceDifference};
use anyhow::Result;
use artemis_core::types::Strategy;
use async_trait::async_trait;
use ethers::{prelude::*};
use std::sync::Arc;
use artemis_core::collectors::block_collector::NewBlock;
use super::constants::{POOL_ADDRESS};
use bindings_binance_uni::uniswap_v3_pool::{UniswapV3Pool};
use std::collections::HashMap;

pub struct BinanceUni<M> {
    client: Arc<M>,
    pool_contract: Arc<UniswapV3Pool<M>>,
}

impl<M: Middleware + 'static> BinanceUni<M> {
    pub fn new(client: Arc<M>) -> Self {
        let pool_contract = Arc::new(UniswapV3Pool::new(
            *POOL_ADDRESS,
            client.clone(),
        ));
        Self { client, pool_contract }
    }
}

#[async_trait]
impl<M: Middleware + 'static> Strategy<Event, Action> for BinanceUni<M> {
    async fn sync_state(&mut self) -> Result<()> {
        Ok(())
    }
    async fn process_event(&mut self, event: Event) -> Option<Action> {
        match event {
            Event::NewBlock(block) => match self.process_new_block_event(block).await {
                Ok(_) => None,
                Err(e) => {
                    panic!("Strategy is out of sync {}", e);
                }
            },
        }
    }
}

impl<M: Middleware + 'static> BinanceUni<M> {
    async fn process_new_block_event(&mut self, event: NewBlock) -> Result<()> {
        println!("blockNumber {}", event.number);
        //let binance_price = self.get_binance_price().await?;
        let amount_in_eth = U256::from_dec_str("1000000000000000000").unwrap();
        let amount_in_usdc = U256::from_dec_str("1000000").unwrap();
        let slot0 = self.pool_contract.slot_0().call().await?;
        let liquidity = self.pool_contract.liquidity().call().await?;

        let sqrtPX96 = slot0.0;
        let uni_price = self.get_uni_price(sqrtPX96);
        let uni_price_next_eth = self.get_uni_price_after_swap(U256::from(sqrtPX96), U256::from(liquidity), amount_in_eth, false);
        let amount_out_eth = self.get_amount_out(uni_price_next_eth, U256::from(sqrtPX96), U256::from(liquidity), false);
        println!("uni_price_next_eth, {:?}", uni_price_next_eth);
        println!("amount_out_eth {}", amount_out_eth);

        let uni_price_next_usdc = self.get_uni_price_after_swap(U256::from(sqrtPX96), U256::from(liquidity), amount_in_usdc, true);
        let amount_out_usdc = self.get_amount_out(uni_price_next_usdc, U256::from(sqrtPX96), U256::from(liquidity), true);
        //
        println!("uni_price_next_usdc, {:?}", uni_price_next_usdc);
        println!("amount_out_usdc {}", amount_out_usdc);
        //println!("binance_price {:#?}", binance_price);
        //println!("uni_price, {:#?}", uni_price);
        // let price_difference = self.price_difference(binance_price, uni_price);
        // println!("price_difference {:?}", price_difference);
        Ok(())
    }


    fn price_difference(&self, a: TokensPrice, b: TokensPrice) -> HashMap<String, PriceDifference> {
        let mut map: HashMap<String, PriceDifference> = HashMap::new();
        let eth_price_difference: PriceDifference = if a.eth > b.eth {
            PriceDifference {
                difference_usd: a.eth - b.eth,
                difference_in_percent: ((a.eth - b.eth) / a.eth) * 100.0,
                name: (a.name).clone(),
            }
        } else {
            PriceDifference {
                difference_usd: b.eth - a.eth,
                difference_in_percent: ((b.eth - a.eth) / b.eth) * 100.0,
                name: (b.name).clone(),
            }
        };
        map.insert(String::from("ETH"), eth_price_difference);
        let usdc_price_difference: PriceDifference = if a.usdc > b.usdc {
            PriceDifference {
                difference_usd: a.usdc - b.usdc,
                difference_in_percent: ((a.usdc - b.usdc) / a.usdc) * 100.0,
                name: (a.name).clone(),
            }
        } else {
            PriceDifference {
                difference_usd: b.usdc - a.usdc,
                difference_in_percent: ((b.usdc - a.usdc) / b.usdc) * 100.0,
                name: (b.name).clone(),
            }
        };
        map.insert(String::from("USDC"), usdc_price_difference);
        map
    }

    async fn get_binance_price(&self) -> Result<TokensPrice> {
        let res: BinancePriceResponse = reqwest::get("https://api.binance.com/api/v3/ticker/bookTicker?symbol=ETHUSDC")
            .await?
            .json()
            .await?;
        let price = res.askPrice.parse::<f32>().unwrap();
        let tokens_price: TokensPrice = TokensPrice {
            usdc: 1.0 / price,
            eth: price,
            name: String::from("Binance"),
        };
        Ok(tokens_price)
    }

    fn get_uni_price(&self, sqrt_px96: U256) -> TokensPrice {
        let price = (self.sqrt_px96_to_price(sqrt_px96).to_string()).parse::<f32>().unwrap();
        TokensPrice {
            usdc: price / 1e12 as f32,
            eth: (1.0 / (price)) * 1e12 as f32,
            name: String::from("Uniswap"),
        }
    }

    fn sqrt_px96_to_price(&self, sqrt_px96: U256) -> U256 {
        sqrt_px96.pow(U256::from(2)) / U256::from(2).pow(U256::from(192))
    }

    fn get_uni_price_after_swap(&self, sqrt_px96: U256, liquidity: U256, amount_in: U256, zero_for_one: bool) -> U256 {
        if zero_for_one {
            let numerator1 = liquidity << U256::from(96);
            let product = self.multiply_in256(amount_in, sqrt_px96);
            if (product / amount_in) == sqrt_px96 {
                let denominator = self.add_in256(numerator1, product);
                if denominator >= numerator1 {
                    let res = self.mul_div_rounding_up(numerator1, sqrt_px96, denominator);
                    return res;
                }
            }
            self.mul_div_rounding_up(numerator1, U256::from(1), amount_in + (numerator1 / sqrt_px96))
        } else {
            let max_uint160: U256 = U256::from_dec_str("1461501637330902918203684832716283019655932542976").unwrap();
            let quotient = if amount_in <= max_uint160 {
                (amount_in << U256::from(96)) / liquidity
            } else {
                (amount_in * U256::from(2).pow(U256::from(96))) / liquidity
            };
            sqrt_px96 + quotient
        }
    }

    fn get_amount_out(&self, mut sqrt_ratio_ax_96: U256, mut sqrt_ratio_bx_96: U256, liquidity: U256, zero_for_one: bool) -> U256 {
        if zero_for_one {
            self.get_amount_1_delta(sqrt_ratio_ax_96, sqrt_ratio_bx_96, liquidity)
        } else {
            self.get_amount_0_delta(sqrt_ratio_ax_96, sqrt_ratio_bx_96, liquidity)
        }
    }

    fn get_amount_0_delta(&self, mut sqrt_ratio_ax_96: U256, mut sqrt_ratio_bx_96: U256, liquidity: U256) -> U256 {
        if sqrt_ratio_ax_96 > sqrt_ratio_bx_96 {
            (sqrt_ratio_ax_96, sqrt_ratio_bx_96) = (sqrt_ratio_bx_96, sqrt_ratio_ax_96)
        };
        ((liquidity * U256::from(2).pow(U256::from(96))) * (sqrt_ratio_bx_96 - sqrt_ratio_ax_96) / sqrt_ratio_bx_96 / sqrt_ratio_ax_96) * U256::from(9995u64) / U256::from(10000u64)
    }

    fn get_amount_1_delta(&self, mut sqrt_ratio_ax_96: U256, mut sqrt_ratio_bx_96: U256, liquidity: U256) -> U256 {
        if sqrt_ratio_ax_96 > sqrt_ratio_bx_96 {
            (sqrt_ratio_ax_96, sqrt_ratio_bx_96) = (sqrt_ratio_bx_96, sqrt_ratio_ax_96)
        };
        (liquidity * (sqrt_ratio_bx_96 - sqrt_ratio_ax_96) / U256::from(2).pow(U256::from(96))) * U256::from(9995u64) / U256::from(10000u64)
    }

    fn mul_div_rounding_up(&self, a: U256, b: U256, denominator: U256) -> U256 {
        let _a = U512::from(a);
        let _b = U512::from(b);
        let _denominator = U512::from(denominator);
        let product = _a * _b;
        let mut result = product / _denominator;
        if (product % _denominator) != U512::zero() {
            result = result + U512::from(1)
        }
        U256::from_dec_str(result.to_string().as_str()).unwrap()
    }

    fn multiply_in256(&self, a: U256, b: U256) -> U256 {
        let product = a * b;
        product & U256::MAX
    }

    fn add_in256(&self, a: U256, b: U256) -> U256 {
        let sum = a + b;
        sum & U256::MAX
    }
}