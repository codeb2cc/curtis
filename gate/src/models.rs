use std::cmp::Ordering;

use chrono::offset::{LocalResult, TimeZone, Utc};
use ring::hmac;
use serde::{Deserialize, Deserializer, Serialize};

use curtis_core::models::{
    Order as CoreOrder, OrderBook, OrderBookUpdater, Trade as CoreTrade, TradeDirection,
};
use curtis_core::{types::Exchange, utils::serde::de_str2num};

const NANOS_PER_SEC: u32 = 1_000_000_000;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct GateConfig {
    pub enabled: bool,
    pub key: String,
    pub secret: String,
    pub spot_ws_endpoint: String,
    pub futures_ws_endpoint: String,

    pub init_requests: Option<Vec<String>>,
}

impl GateConfig {
    // https://www.gate.io/docs/websocket/index.html#auth-api
    pub fn build_sign(&self, timestamp: i64) -> String {
        let sign_key = hmac::Key::new(hmac::HMAC_SHA512, self.secret.as_bytes());
        let sign = hmac::sign(&sign_key, timestamp.to_string().as_bytes());
        base64::encode(sign.as_ref())
    }

    pub fn build_sign_ws(&self, timestamp: i64) -> String {
        self.build_sign(timestamp)
    }
}

// WebSocket General

#[derive(Serialize, Debug)]
pub struct AuthParams(String, String, i64);

// WebSocket v3 (Spot)

#[derive(Serialize, Debug)]
pub struct WSv3AuthRequest {
    pub id: i64,
    pub method: String,
    pub params: AuthParams,
}

impl WSv3AuthRequest {
    pub fn new(key: String, timestamp: i64, signature: String) -> WSv3AuthRequest {
        WSv3AuthRequest {
            id: 1,
            method: "server.sign".to_string(),
            params: AuthParams(key, signature, timestamp),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct WSv3ResponseResult {
    pub status: String,
}

#[derive(Deserialize, Debug)]
pub struct WSv3Response {
    pub id: i64,
    pub error: Option<String>,
    pub result: WSv3ResponseResult,
}

#[derive(Deserialize, Debug)]
pub struct WSv3Notification {
    pub id: Option<i64>,
    pub method: String,
    #[serde(skip)]
    pub params: Option<()>,
}

// https://www.gate.io/docs/websocket/index.html#depth-notification
#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotDepthUpdate {
    pub id: Option<i64>,
    pub method: String,
    pub params: WSSpotDepthUpdateParams,
}

impl OrderBookUpdater for WSSpotDepthUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn update_book(&self, book: &mut OrderBook) {
        book.timestamp = Utc::now();
        // It's complete result
        if self.params.0 {
            book.asks.clear();
            book.bids.clear();
        }

        if let Some(asks) = &self.params.1.asks {
            for o in asks.iter() {
                if let Ok(i) = book
                    .asks
                    .binary_search_by(|s| s.price.partial_cmp(&o.price).unwrap())
                {
                    book.asks[i].amount = o.amount;
                } else {
                    let s = CoreOrder {
                        exchange: Exchange::Gate,
                        price: o.price,
                        amount: o.amount,
                    };
                    book.asks.push(s);
                }
            }
            book.asks.retain(|s| s.amount > 0.0);
            book.asks.truncate(10);
            book.asks
                .sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
        }

        if let Some(bids) = &self.params.1.bids {
            for o in bids.iter() {
                if let Some(i) = book
                    .bids
                    .iter()
                    .position(|s| s.price.partial_cmp(&o.price).unwrap() == Ordering::Equal)
                {
                    book.bids[i].amount = o.amount;
                } else {
                    let s = CoreOrder {
                        exchange: Exchange::Gate,
                        price: o.price,
                        amount: o.amount,
                    };
                    book.bids.push(s);
                }
            }
            book.bids.retain(|s| s.amount > 0.0);
            book.bids.truncate(10);
            book.bids
                .sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotTradeUpdate {
    pub id: Option<i64>,
    pub method: String,
    pub params: WSSpotTradeUpdateParams,
}

impl OrderBookUpdater for WSSpotTradeUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.params.1.iter() {
            let ts = match Utc.timestamp_opt(
                t.time.trunc() as i64,
                (t.time.fract() * NANOS_PER_SEC as f64).trunc() as u32,
            ) {
                LocalResult::Single(ts) => ts,
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::Gate,
                timestamp: ts,
                price: t.price,
                amount: t.amount,
                direction: TradeDirection::Buy,
            };
            if t.direction == "sell" {
                trade.direction = TradeDirection::Sell;
                book.sells.push_front(trade);
            } else {
                book.buys.push_front(trade);
            }
        }

        book.sells.truncate(10);
        book.buys.truncate(10);
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotDepthUpdateParams(pub bool, pub SpotDepthData, pub String);

#[derive(Clone, Deserialize, Debug)]
pub struct SpotDepthData {
    pub asks: Option<Vec<Order>>,
    pub bids: Option<Vec<Order>>,
}

#[derive(Debug)]
pub struct OrderRaw(f64, f64);

#[derive(Clone, Deserialize, Debug)]
pub struct Order {
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub amount: f64,
}

impl<'de> Deserialize<'de> for OrderRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
            .map(|Order { price, amount }| OrderRaw(price, amount))
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotTradeUpdateParams(pub String, pub Vec<Trade>);

#[derive(Clone, Deserialize, Debug)]
pub struct Trade {
    pub id: i64,
    pub time: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub amount: f64,
    #[serde(rename(deserialize = "type"))]
    pub direction: String,
}

// WebSocket v4 (Futures)

#[derive(Clone, Deserialize, Debug)]
pub struct WSv4Request<T> {
    pub time: i64,
    pub channel: String,
    pub auth: Option<String>,
    pub event: String,
    #[serde(skip)]
    pub payload: T,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSv4Response {
    pub time: i64,
    pub channel: String,
    pub event: String,
    pub error: Option<WSv4Error>,
    #[serde(skip)]
    pub result: Option<()>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSv4Error {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSv4FuturesDepthResponse<T> {
    pub time: i64,
    pub channel: String,
    pub event: String,
    #[serde(skip)]
    pub error: Option<()>,
    pub result: T,
}

pub type WSv4FuturesDepthAll = WSv4FuturesDepthResponse<FuturesDepthAll>;

impl OrderBookUpdater for WSv4FuturesDepthAll {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn update_book(&self, book: &mut OrderBook) {
        book.timestamp = Utc.timestamp(self.time, 0);

        book.bids.clear();
        for o in self.result.bids.iter() {
            book.bids.push(CoreOrder {
                exchange: Exchange::Gate,
                price: o.price,
                amount: o.size as f64,
            });
        }

        book.asks.clear();
        for o in self.result.asks.iter() {
            book.asks.push(CoreOrder {
                exchange: Exchange::Gate,
                price: o.price,
                amount: o.size as f64,
            });
        }
    }
}

pub type WSv4FuturesDepthUpdate = WSv4FuturesDepthResponse<FuturesDepthUpdate>;

impl OrderBookUpdater for WSv4FuturesDepthUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn update_book(&self, book: &mut OrderBook) {
        book.timestamp = Utc.timestamp(self.time, 0);

        for o in self.result.iter().filter(|o| o.size < 0) {
            if let Ok(i) = book
                .asks
                .binary_search_by(|s| s.price.partial_cmp(&o.price).unwrap())
            {
                book.asks[i].amount = o.size as f64;
            } else {
                book.bids.push(CoreOrder {
                    exchange: Exchange::Gate,
                    price: o.price,
                    amount: o.size as f64,
                });
            }
        }
        book.asks.retain(|s| s.amount > 0.0);
        book.asks.truncate(10);
        book.asks
            .sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

        for o in self.result.iter().filter(|o| o.size > 0) {
            if let Some(i) = book
                .bids
                .iter()
                .position(|s| s.price.partial_cmp(&o.price).unwrap() == Ordering::Equal)
            {
                book.bids[i].amount = o.size as f64;
            } else {
                book.bids.push(CoreOrder {
                    exchange: Exchange::Gate,
                    price: o.price,
                    amount: o.size as f64,
                });
            }
        }
        book.bids.retain(|s| s.amount > 0.0);
        book.bids.truncate(10);
        book.bids
            .sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesDepthAll {
    pub contract: String,
    pub asks: Vec<FuturesDepthOrder>,
    pub bids: Vec<FuturesDepthOrder>,
}

pub type FuturesDepthUpdate = Vec<FuturesDepthUpdateOrder>;

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesDepthOrder {
    #[serde(deserialize_with = "de_str2num")]
    #[serde(rename(deserialize = "p"))]
    pub price: f64,
    #[serde(rename(deserialize = "s"))]
    pub size: i64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesDepthUpdateOrder {
    #[serde(deserialize_with = "de_str2num")]
    #[serde(rename(deserialize = "p"))]
    pub price: f64,
    #[serde(rename(deserialize = "s"))]
    pub size: i64,
    #[serde(rename(deserialize = "c"))]
    pub channel: String,
    pub id: u64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSv4FuturesTradeUpdate {
    pub time: i64,
    pub channel: String,
    pub event: String,
    #[serde(skip)]
    pub error: Option<()>,
    pub result: FuturesTradeResult,
}

impl OrderBookUpdater for WSv4FuturesTradeUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.result.iter() {
            let ts = match Utc.timestamp_opt(t.time, 0) {
                LocalResult::Single(ts) => ts,
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::Gate,
                timestamp: ts,
                price: t.price,
                amount: t.size.abs() as f64,
                direction: TradeDirection::Buy,
            };
            if t.size < 0 {
                trade.direction = TradeDirection::Sell;
                book.sells.push_front(trade);
            } else {
                book.buys.push_front(trade);
            }
        }

        book.sells.truncate(10);
        book.buys.truncate(10);
    }
}

pub type FuturesTradeResult = Vec<FuturesTrade>;

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesTrade {
    pub id: i64,
    #[serde(rename(deserialize = "create_time"))]
    pub time: i64,
    pub contract: String,
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    pub size: i64,
}

// Unit Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize() {
        let s = "{\"method\": \"trades.update\", \"params\": [\"ETH_USDT\", [{\
                 \"id\": 208874359, \"time\": 1574221130.948936, \"price\": \"\
                 176.01\", \"amount\":\"0.03457001\", \"type\": \"buy\"}]], \"\
                 id\": null}";
        assert!(serde_json::from_str::<WSSpotTradeUpdate>(&s).is_ok());
    }

    #[test]
    fn deserialize_v4_error() {
        let s = "{\"time\":1574328293,\"channel\":\"futures.order_book\",\"eve\
                 nt\":\"subscribe\",\"error\":{\"code\":2,\"message\":\"market\
                 is not found.\"},\"result\":{\"status\":\"failed\"}}";
        assert!(serde_json::from_str::<WSv4Error>(&s).is_ok());
    }
}
