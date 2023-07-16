use super::types::{Action, Event, BinanceOrdersResponse, TokensPrice, PriceDifference, Profit};
use anyhow::Result;
use artemis_core::types::Strategy;
use async_trait::async_trait;
use ethers::{prelude::*};
use std::sync::Arc;
use artemis_core::collectors::block_collector::NewBlock;
use super::constants::{POOL_ADDRESS, FEE};
use bindings_binance_uni::uniswap_v3_pool::{UniswapV3Pool};
use std::collections::HashMap;
use ethers::utils::{parse_units, format_units};
use tracing::{info};

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
        info!("blockNumber {}", event.number);
        let slot0 = self.pool_contract.slot_0().call().await?;
        let liquidity = self.pool_contract.liquidity().call().await?;
        let sqrt_px96 = slot0.0;
        self.arb_possibility(U256::from(sqrt_px96), U256::from(liquidity)).await;
        Ok(())
    }

    async fn get_binance_orders(&self) -> Result<BinanceOrdersResponse> {
        let res: BinanceOrdersResponse = reqwest::get("https://api.binance.com/api/v3/depth?symbol=ETHUSDC")
            .await?
            .json()
            .await?;
        Ok(res)
    }

    async fn arb_possibility(&self, sqrt_px96: U256, liquidity: U256) {
        let uni_price = self.get_uni_price(sqrt_px96);
        let binance_orders = self.get_binance_orders().await;

        if let Ok(binance_response) = binance_orders {
            // Access the bids field
            let bids = binance_response.bids;
            let asks = binance_response.asks;
            let binance_price_bids: f64 = (bids[0].0).to_string().parse::<f64>().unwrap();
            let binance_price_asks: f64 = (asks[0].0).to_string().parse::<f64>().unwrap();
            info!("binance_price_bids {:?}", binance_price_bids);
            info!("binance_price_asks {:?}", binance_price_asks);
            info!("uni_price.eth {:?}", uni_price.eth);
            let mut i = 0;
            let mut amount: f64 = 0.0;
            // ETH -> USDC -> ETH
            if uni_price.eth > binance_price_bids {
                let mut sell_price: f64 = (asks[0].0).to_string().parse::<f64>().unwrap();
                while i < asks.len() && sell_price <= uni_price.eth {
                    amount = amount + ((asks[i].1).to_string().parse::<f64>().unwrap());
                    sell_price = (asks[i].0).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                }
                let res = self.binary_search_uni(amount, sqrt_px96, liquidity, asks.clone(), false);
                info!("ETH {:?}", res);
            } else {
                let mut buy_price: f64 = (bids[0].0).to_string().parse::<f64>().unwrap();
                while i < bids.len() && buy_price >= uni_price.eth {
                    amount = amount + ((bids[i].1).to_string().parse::<f64>().unwrap());
                    buy_price = (bids[i].0).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                }
                let res = self.binary_search_binance(amount, sqrt_px96, liquidity, bids.clone(), false);
                info!("ETH {:?}", res);
            }

            // USDC -> ETH -> USDC
            if uni_price.eth < binance_price_asks {
                let mut buy_price: f64 = (bids[0].0).to_string().parse::<f64>().unwrap();
                while i < bids.len() && buy_price >= uni_price.eth {
                    amount = amount + ((bids[i].1).to_string().parse::<f64>().unwrap() * (bids[i].0).to_string().parse::<f64>().unwrap());
                    buy_price = (bids[i].0).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                }
                let res = self.binary_search_uni(amount, sqrt_px96, liquidity, bids.clone(), true);
                info!("USDC {:?}", res);
            } else {
                let mut sell_price: f64 = (asks[0].0).to_string().parse::<f64>().unwrap();
                while i < asks.len() && sell_price <= uni_price.eth {
                    amount = amount + ((asks[i].1).to_string().parse::<f64>().unwrap() * (asks[i].0).to_string().parse::<f64>().unwrap());
                    sell_price = (asks[i].0).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                }
                let res = self.binary_search_binance(amount, sqrt_px96, liquidity, asks.clone(), true);
                info!("USDC {:?}", res);
            }
        } else {
            println!("Error: BinanceOrdersResponse is not available");
        }
    }

    fn check_uni_profit(&self, amount: f64, sqrt_px96: U256, liquidity: U256, asks: Vec<(String, String)>, zero_for_one: bool) -> f64 {
        if zero_for_one {
            let next_price = self.get_uni_price_after_swap(sqrt_px96, liquidity, parse_units(amount.to_string(), "mwei").unwrap().into(), zero_for_one);
            let amount_out = self.get_amount_out(next_price, sqrt_px96, liquidity, zero_for_one);
            let mut amount_local: f64 = 0.0;
            let mut profit_amount = 0.0;
            let mut i = 0;
            while amount_local != self.u256_to_f64(amount_out, "ether") {
                if (amount_local + (asks[i].1).to_string().parse::<f64>().unwrap()) < self.u256_to_f64(amount_out, "ether") {
                    amount_local = amount_local + (asks[i].1).to_string().parse::<f64>().unwrap();
                    profit_amount = profit_amount + ((asks[i].1).to_string().parse::<f64>().unwrap() * (asks[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                } else {
                    let difference = self.u256_to_f64(amount_out, "ether") - amount_local;
                    amount_local = self.u256_to_f64(amount_out, "ether");
                    profit_amount = profit_amount + (difference * (asks[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                }
            }
            profit_amount - amount
        } else {
            let next_price = self.get_uni_price_after_swap(sqrt_px96, liquidity, parse_units(amount.to_string(), "ether").unwrap().into(), zero_for_one);
            let amount_out = self.get_amount_out(next_price, sqrt_px96, liquidity, false);
            let mut amount_local: f64 = 0.0;
            let mut profit_amount = 0.0;
            let mut i = 0;
            while amount_local != self.u256_to_f64(amount_out, "mwei") {
                if amount_local + ((asks[i].1).to_string().parse::<f64>().unwrap() * (asks[i].0).to_string().parse::<f64>().unwrap()) < self.u256_to_f64(amount_out, "mwei") {
                    amount_local = amount_local + ((asks[i].1).to_string().parse::<f64>().unwrap() * (asks[i].0).to_string().parse::<f64>().unwrap());
                    profit_amount = profit_amount + (asks[i].1).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                } else {
                    let difference = self.u256_to_f64(amount_out, "mwei") - amount_local;
                    amount_local = self.u256_to_f64(amount_out, "mwei");
                    profit_amount = profit_amount + (difference / (asks[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                }
            }
            profit_amount - amount
        }
    }

    fn check_binance_profit(&self, amount: f64, sqrt_px96: U256, liquidity: U256, bids: Vec<(String, String)>, zero_for_one: bool) -> f64 {
        if zero_for_one {
            let mut amount_local: f64 = 0.0;
            let mut amount_out: f64 = 0.0;
            let mut i = 0;
            while amount_local != amount {
                if (amount_local + ((bids[i].1).to_string().parse::<f64>().unwrap() * (bids[i].0).to_string().parse::<f64>().unwrap())) < amount {
                    amount_out = amount_out + (bids[i].1).to_string().parse::<f64>().unwrap();
                    amount_local = amount_local + ((bids[i].1).to_string().parse::<f64>().unwrap() * (bids[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                } else {
                    let difference = amount - amount_local;
                    amount_local = amount;
                    amount_out = amount_out + (difference / (bids[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                }
            }
            let next_price = self.get_uni_price_after_swap(sqrt_px96, liquidity, parse_units(amount_out.to_string(), "ether").unwrap().into(), !zero_for_one);
            let amount_out = self.get_amount_out(next_price, sqrt_px96, liquidity, !zero_for_one);
            self.u256_to_f64(amount_out, "mwei") - amount
        } else {
            let mut amount_local: f64 = 0.0;
            let mut amount_out: f64 = 0.0;
            let mut i = 0;
            while amount_local != amount {
                if amount_local + (bids[i].1).to_string().parse::<f64>().unwrap() < amount {
                    amount_out = amount_out + ((bids[i].1).to_string().parse::<f64>().unwrap() * (bids[i].0).to_string().parse::<f64>().unwrap());
                    amount_local = amount_local + (bids[i].1).to_string().parse::<f64>().unwrap();
                    i = i + 1;
                } else {
                    let difference = amount - amount_local;
                    amount_local = amount;
                    amount_out = amount_out + (difference * (bids[i].0).to_string().parse::<f64>().unwrap());
                    i = i + 1;
                }
            }
            let next_price = self.get_uni_price_after_swap(sqrt_px96, liquidity, parse_units(amount_out.to_string(), "mwei").unwrap().into(), !zero_for_one);
            let amount_out = self.get_amount_out(next_price, sqrt_px96, liquidity, !zero_for_one);
            self.u256_to_f64(amount_out, "ether") - amount
        }
    }

    fn binary_search_uni(&self, amount: f64, sqrt_px96: U256, liquidity: U256, asks: Vec<(String, String)>, zero_for_one: bool) -> Profit {
        let mut left = 1.0;
        let mut right = amount;
        let mut max_return_value: f64 = 0.0;
        let mut amount_local: f64 = 0.0;

        while left <= right {
            let mid = (left + right) / 2.0;
            let return_value = self.check_uni_profit(mid, sqrt_px96, liquidity, asks.clone(), zero_for_one);
            if return_value > max_return_value {
                right = mid - 1.0;
                max_return_value = return_value;
                amount_local = mid;
            } else {
                left = mid + 1.0;
            }
        }
        Profit {
            profit: max_return_value,
            amount: amount_local,
            name: String::from("Uniswap"),
        }
    }

    fn binary_search_binance(&self, amount: f64, sqrt_px96: U256, liquidity: U256, bids: Vec<(String, String)>, zero_for_one: bool) -> Profit {
        let mut left = 1.0;
        let mut right = amount;
        let mut max_return_value: f64 = 0.0;
        let mut amount_local: f64 = 0.0;

        while left <= right {
            let mid = (left + right) / 2.0;
            let return_value = self.check_binance_profit(mid, sqrt_px96, liquidity, bids.clone(), zero_for_one);
            if return_value > max_return_value {
                right = mid - 1.0;
                max_return_value = return_value;
                amount_local = mid;
            } else {
                left = mid + 1.0;
            }
        }
        Profit {
            profit: max_return_value,
            amount: amount_local,
            name: String::from("Binance"),
        }
    }

    fn get_uni_price(&self, sqrt_px96: U256) -> TokensPrice {
        let value = self.sqrt_px96_to_price(sqrt_px96);
        let price = self.minus_fee(value).to_string().parse::<f64>().unwrap();
        TokensPrice {
            usdc: (price / 1e12),
            eth: (1.0 / price * 1e12),
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

    fn get_amount_out(&self, sqrt_ratio_ax_96: U256, sqrt_ratio_bx_96: U256, liquidity: U256, zero_for_one: bool) -> U256 {
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
        let _sqrt_ratio_ax_96 = U512::from(sqrt_ratio_ax_96);
        let _sqrt_ratio_bx_96 = U512::from(sqrt_ratio_bx_96);
        let _liquidity = U512::from(liquidity);
        let value = (_liquidity * U512::from(2).pow(U512::from(96))) * (_sqrt_ratio_bx_96 - _sqrt_ratio_ax_96) / _sqrt_ratio_bx_96 / _sqrt_ratio_ax_96;
        self.minus_fee(U256::from_dec_str(value.to_string().as_str()).unwrap())
    }

    fn get_amount_1_delta(&self, mut sqrt_ratio_ax_96: U256, mut sqrt_ratio_bx_96: U256, liquidity: U256) -> U256 {
        if sqrt_ratio_ax_96 > sqrt_ratio_bx_96 {
            (sqrt_ratio_ax_96, sqrt_ratio_bx_96) = (sqrt_ratio_bx_96, sqrt_ratio_ax_96)
        };
        let value = liquidity * (sqrt_ratio_bx_96 - sqrt_ratio_ax_96) / U256::from(2).pow(U256::from(96));
        self.minus_fee(value)
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

    fn minus_fee(&self, value: U256) -> U256 {
        let decrease_amount = value / U256::from(100) * U256::from(FEE as u128);
        value - decrease_amount
    }

    fn u256_to_f64(&self, amount: U256, unit_name: &str) -> f64 {
        (format_units(amount, unit_name).unwrap()).parse::<f64>().unwrap()
    }
}