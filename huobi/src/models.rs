use chrono::{
    offset::{LocalResult, TimeZone, Utc},
    DateTime,
};
use log::trace;
use ring::hmac;
use serde::{Deserialize, Deserializer, Serialize};

use curtis_core::models::{
    Order as CoreOrder, OrderBook, OrderBookUpdater, Trade as CoreTrade, TradeDirection,
};
use curtis_core::{types::Exchange, utils::serde::de_str2num};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct HuobiConfig {
    pub enabled: bool,
    pub key: String,
    pub secret: String,
    pub spot_ws_endpoint: String,
    pub futures_ws_endpoint: String,

    pub init_requests: Option<Vec<String>>,
}

impl HuobiConfig {
    // https://huobiapi.github.io/docs/spot/v1/cn/#c64cd15fdc
    pub fn build_sign(
        &self,
        timestamp: String,
        method: String,
        host: String,
        path: String,
    ) -> String {
        let sign_key = hmac::Key::new(hmac::HMAC_SHA256, self.secret.as_bytes());
        let ts_encoded: String =
            url::form_urlencoded::byte_serialize(timestamp.as_bytes()).collect();
        let playload = format!(
            "{}\n\
             {}\n\
             {}\n\
             AccessKeyId={}&SignatureMethod=HmacSHA256&SignatureVersion=2&Timestamp={}",
            method, host, path, self.key, ts_encoded,
        );
        trace!("Huobi Signature Playload: {:?}", playload);
        let sign = hmac::sign(&sign_key, playload.as_bytes());
        base64::encode(sign.as_ref())
    }

    pub fn build_sign_ws(&self, host: String, timestamp: String) -> String {
        self.build_sign(
            timestamp.to_string(),
            "GET".to_string(),
            host,
            "/ws/v1".to_string(),
        )
    }
}

// WebSocket General

#[derive(Serialize, Debug)]
pub struct WSv1AuthRequest {
    pub op: String,
    pub cid: String,
    #[serde(rename(serialize = "AccessKeyId"))]
    pub access_key_id: String,
    #[serde(rename(serialize = "SignatureMethod"))]
    pub signature_method: String,
    #[serde(rename(serialize = "SignatureVersion"))]
    pub signature_version: String,
    #[serde(rename(serialize = "Timestamp"))]
    pub timestamp: String,
    #[serde(rename(serialize = "Signature"))]
    pub signature: String,
}

impl WSv1AuthRequest {
    pub fn new(key: String, timestamp: String, signature: String) -> WSv1AuthRequest {
        WSv1AuthRequest {
            op: String::from("auth"),
            cid: String::from("0"),
            access_key_id: key,
            signature_method: String::from("HmacSHA256"),
            signature_version: String::from("2"),
            timestamp,
            signature,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct WSv1AuthResponse {
    pub op: String,
    pub cid: String,
    #[serde(rename(deserialize = "err-code"))]
    pub error_code: u32,
    #[serde(rename(deserialize = "ts"))]
    pub timestamp: i64,
}

#[derive(Deserialize, Debug)]
pub struct WSResponse {
    pub id: String,
    pub status: String,
    #[serde(skip)]
    pub subbed: String,
    pub ts: i64,
}

#[derive(Deserialize, Debug)]
pub struct WSUpdate {
    #[serde(rename(deserialize = "ch"))]
    pub channel: String,
    pub ts: i64,
    #[serde(skip)]
    pub tick: Option<()>,
}

pub trait DepthTick: Send + Sync + Clone {
    fn get_timestamp(&self) -> Option<DateTime<Utc>>;
    fn get_bids(&self) -> &Vec<Order>;
    fn get_asks(&self) -> &Vec<Order>;
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSDepthUpdate<T>
where
    T: DepthTick,
{
    #[serde(rename(deserialize = "ch"))]
    pub channel: String,
    pub ts: i64,
    pub tick: T,
}

impl<T: 'static> OrderBookUpdater for WSDepthUpdate<T>
where
    T: DepthTick,
{
    fn exchange(&self) -> Exchange {
        Exchange::Huobi
    }

    fn update_book(&self, book: &mut OrderBook) {
        match self.tick.get_timestamp() {
            Some(dt) => book.timestamp = dt,
            None => {
                book.timestamp = Utc::now();
            }
        }

        book.bids.clear();
        for o in self.tick.get_bids().iter() {
            book.bids.push(CoreOrder {
                exchange: Exchange::Huobi,
                price: o.price,
                amount: o.volume,
            });
        }

        book.asks.clear();
        for o in self.tick.get_asks().iter() {
            book.asks.push(CoreOrder {
                exchange: Exchange::Huobi,
                price: o.price,
                amount: o.volume,
            });
        }
    }
}

#[derive(Debug)]
pub struct OrderRaw(f64, f64);

#[derive(Clone, Deserialize, Debug)]
pub struct Order {
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub volume: f64,
}

impl<'de> Deserialize<'de> for OrderRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
            .map(|Order { price, volume }| OrderRaw(price, volume))
    }
}

// WebSocket Spot

pub type WSSpotDepthUpdate = WSDepthUpdate<SpotDepthTick>;

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotTradeUpdate {
    #[serde(rename(deserialize = "ch"))]
    pub channel: String,
    pub ts: i64,
    pub tick: SpotTradeData,
}

impl OrderBookUpdater for WSSpotTradeUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Huobi
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.tick.data.iter() {
            let ts = match Utc.timestamp_millis_opt(t.ts) {
                LocalResult::Single(ts) => ts,
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::Huobi,
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
pub struct SpotDepthTick {
    pub version: i64,
    pub ts: i64,

    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
}

impl DepthTick for SpotDepthTick {
    fn get_timestamp(&self) -> Option<DateTime<Utc>> {
        Some(Utc.timestamp_millis(self.ts))
    }

    fn get_bids(&self) -> &Vec<Order> {
        &self.bids
    }

    fn get_asks(&self) -> &Vec<Order> {
        &self.asks
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct SpotTrade {
    pub ts: i64,
    #[serde(rename(deserialize = "tradeId"))]
    pub trade_id: u64,
    pub price: f64,
    pub amount: f64,
    pub direction: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct SpotTradeData {
    pub id: i64,
    pub ts: i64,
    pub data: Vec<SpotTrade>,
}

// WebSocket Futures
pub type WSFuturesDepthUpdate = WSDepthUpdate<FuturesDepthTick>;

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesDepthTick {
    pub mrid: u64,
    pub id: u64,
    pub asks: Vec<Order>,
    pub bids: Vec<Order>,
    pub ts: i64,
    pub version: i64,
    #[serde(rename(deserialize = "ch"))]
    pub channel: String,
}

impl DepthTick for FuturesDepthTick {
    fn get_timestamp(&self) -> Option<DateTime<Utc>> {
        Some(Utc.timestamp_millis(self.ts))
    }

    fn get_bids(&self) -> &Vec<Order> {
        &self.bids
    }
    fn get_asks(&self) -> &Vec<Order> {
        &self.asks
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesTradeUpdate {
    #[serde(rename(deserialize = "ch"))]
    pub channel: String,
    pub ts: i64,
    pub tick: FuturesTradeData,
}

impl OrderBookUpdater for WSFuturesTradeUpdate {
    fn exchange(&self) -> Exchange {
        Exchange::Huobi
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.tick.data.iter() {
            let ts = match Utc.timestamp_millis_opt(t.ts) {
                LocalResult::Single(ts) => ts,
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::Huobi,
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
pub struct FuturesTrade {
    pub ts: i64,
    pub id: u64,
    pub price: f64,
    pub amount: f64,
    pub direction: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesTradeData {
    pub id: i64,
    pub ts: i64,
    pub data: Vec<FuturesTrade>,
}

// Unit Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize() {
        let s = "{\"ch\":\"market.ethusdt.trade.detail\",\"ts\":1574219176875,\
                 \"tick\":{\"id\":102670221777,\"ts\":1574219176791,\"data\":[\
                 {\"id\":10267022177756517857785,\"ts\":1574219176791,\"tradeI\
                 d\":101886520290,\"amount\":1.806,\"price\":176.14,\"directio\
                 n\":\"buy\"},{\"id\":10267022177756517857362,\"ts\":157421917\
                 6791,\"tradeId\":101886520289,\"amount\":29.999,\"price\":176\
                 .14,\"direction\":\"buy\"}]}}";
        assert!(serde_json::from_str::<WSSpotTradeUpdate>(&s).is_ok());
    }
}
