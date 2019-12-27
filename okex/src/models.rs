use chrono::{offset::Utc, DateTime, NaiveDateTime};
use log::warn;
use ring::hmac;
use serde::{Deserialize, Deserializer};

use curtis_core::models::{
    Order as CoreOrder, OrderBook, OrderBookUpdater, Trade as CoreTrade, TradeDirection,
};
use curtis_core::{types::Exchange, utils::serde::de_str2num};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct OKExConfig {
    pub enabled: bool,
    pub key: String,
    pub secret: String,
    pub passphrase: String,
    pub spot_ws_endpoint: String,
    pub futures_ws_endpoint: String,

    pub init_requests: Option<Vec<String>>,
}

impl OKExConfig {
    pub fn build_sign(&self, timestamp: String, method: String, path: String) -> String {
        let sign_key = hmac::Key::new(hmac::HMAC_SHA256, self.secret.as_bytes());
        let sign = hmac::sign(
            &sign_key,
            format!("{}{}{}", timestamp, method, path).as_bytes(),
        );
        base64::encode(sign.as_ref())
    }

    pub fn build_sign_ws(&self, timestamp: i64) -> String {
        self.build_sign(
            timestamp.to_string(),
            "GET".to_string(),
            "/users/self/verify".to_string(),
        )
    }
}

// RESTful API

#[derive(Deserialize, Debug)]
pub struct MarginAccountCurrency {
    #[serde(deserialize_with = "de_str2num")]
    pub available: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub balance: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub borrowed: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub can_withdraw: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub frozen: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub hold: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub holds: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub lending_fee: f64,
}

#[derive(Deserialize, Debug)]
pub struct MarginAccountInstrument {
    pub instrument_id: String,
    #[serde(deserialize_with = "de_str2num")]
    pub liquidation_price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub margin_ratio: f64,
    pub product_id: String,
    #[serde(deserialize_with = "de_str2num")]
    pub risk_rate: f64,

    #[serde(rename(deserialize = "currency:USDT"))]
    pub c_usdt: Option<MarginAccountCurrency>,
    #[serde(rename(deserialize = "currency:BTC"))]
    pub c_btc: Option<MarginAccountCurrency>,
    #[serde(rename(deserialize = "currency:ETH"))]
    pub c_eth: Option<MarginAccountCurrency>,
}

// WebSocket General

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum WSResponse {
    WSEventResponse,
    WSTableSpotTicketResponse,
    WSTableSpotDepthResponse,
}

#[derive(Deserialize, Debug)]
pub struct WSEventResponse {
    pub event: String,
    pub channel: String,
}

#[derive(Deserialize, Debug)]
pub struct WSTableResponse {
    pub table: String,
    #[serde(skip)]
    pub data: Option<()>,
}

pub trait Order {
    fn get_price(&self) -> f64;
    fn get_amount(&self) -> f64;
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSDepthResponse<T>
where
    T: Order,
{
    pub table: String,
    pub action: Option<String>,
    pub data: Vec<DepthData<T>>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct DepthData<T>
where
    T: Order,
{
    pub instrument_id: String,
    pub timestamp: String,

    pub asks: Vec<T>,
    pub bids: Vec<T>,
}

impl<T> OrderBookUpdater for WSDepthResponse<T>
where
    T: 'static + Order + Send + Sync + Clone,
{
    fn exchange(&self) -> Exchange {
        Exchange::OKEx
    }

    fn update_book(&self, book: &mut OrderBook) {
        if self.data.is_empty() {
            return;
        }

        match NaiveDateTime::parse_from_str(&self.data[0].timestamp, "%Y-%m-%dT%H:%M:%S%.3fZ") {
            Ok(dt) => book.timestamp = DateTime::<Utc>::from_utc(dt, Utc),
            Err(_) => {
                warn!("Invalid datetime format: {}", &self.data[0].timestamp);
                book.timestamp = Utc::now();
            }
        }

        book.bids.clear();
        for o in self.data[0].bids.iter() {
            book.bids.push(CoreOrder {
                exchange: Exchange::OKEx,
                price: o.get_price(),
                amount: o.get_amount(),
            });
        }

        book.asks.clear();
        for o in self.data[0].asks.iter() {
            book.asks.push(CoreOrder {
                exchange: Exchange::OKEx,
                price: o.get_price(),
                amount: o.get_amount(),
            });
        }
    }
}

// WebSocket Spot

#[derive(Deserialize, Debug)]
pub struct WSSpotTicketResponse {
    pub table: String,
    pub action: Option<String>,
    pub data: Vec<SpotTicketData>,
}

pub type WSSpotDepthResponse = WSDepthResponse<SpotOrder>;

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotTradeResponse {
    pub table: String,
    pub data: Vec<SpotTrade>,
}

impl OrderBookUpdater for WSSpotTradeResponse {
    fn exchange(&self) -> Exchange {
        Exchange::OKEx
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.data.iter() {
            let ts = match NaiveDateTime::parse_from_str(&t.timestamp, "%Y-%m-%dT%H:%M:%S%.3fZ") {
                Ok(dt) => DateTime::<Utc>::from_utc(dt, Utc),
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::OKEx,
                timestamp: ts,
                price: t.price,
                amount: t.size,
                direction: TradeDirection::Buy,
            };
            if t.side == "sell" {
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

#[derive(Deserialize, Debug)]
pub struct SpotTicketData {
    pub instrument_id: String,
    pub timestamp: String,

    #[serde(deserialize_with = "de_str2num")]
    pub last: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub best_bid: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub best_ask: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub open_24h: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub high_24h: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub low_24h: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub base_volume_24h: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub quote_volume_24h: f64,
}

#[derive(Debug)]
pub struct SpotOrderRaw(f64, f64, i32);

#[derive(Clone, Deserialize, Debug)]
pub struct SpotOrder {
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub amount: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub orders: i32,
}

impl<'de> Deserialize<'de> for SpotOrderRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(
            |SpotOrder {
                 price,
                 amount,
                 orders,
             }| SpotOrderRaw(price, amount, orders),
        )
    }
}

impl Order for SpotOrder {
    fn get_price(&self) -> f64 {
        self.price
    }

    fn get_amount(&self) -> f64 {
        self.amount
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct SpotDepthData {
    pub instrument_id: String,
    pub timestamp: String,

    pub asks: Vec<SpotOrder>,
    pub bids: Vec<SpotOrder>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct SpotTrade {
    pub instrument_id: String,
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    pub side: String,
    #[serde(deserialize_with = "de_str2num")]
    pub size: f64,
    pub timestamp: String,
    pub trade_id: String,
}

// WebSocket Futures
pub type WSFuturesDepthResponse = WSDepthResponse<FuturesOrder>;

#[derive(Debug)]
pub struct FuturesOrderRaw(f64, f64, i32, i32);

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesOrder {
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub amount: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub liquidated_orders: i32,
    #[serde(deserialize_with = "de_str2num")]
    pub orders: i32,
}

impl<'de> Deserialize<'de> for FuturesOrderRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(
            |FuturesOrder {
                 price,
                 amount,
                 liquidated_orders,
                 orders,
             }| FuturesOrderRaw(price, amount, liquidated_orders, orders),
        )
    }
}

impl Order for FuturesOrder {
    fn get_price(&self) -> f64 {
        self.price
    }

    fn get_amount(&self) -> f64 {
        self.amount
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesDepthData {
    pub instrument_id: String,
    pub timestamp: String,

    pub asks: Vec<FuturesOrder>,
    pub bids: Vec<FuturesOrder>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct FuturesTrade {
    pub trade_id: String,
    pub instrument_id: String,
    pub timestamp: String,
    pub side: String,
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub qty: f64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesTradeResponse {
    pub table: String,
    pub data: Vec<FuturesTrade>,
}

impl OrderBookUpdater for WSFuturesTradeResponse {
    fn exchange(&self) -> Exchange {
        Exchange::OKEx
    }

    fn update_book(&self, book: &mut OrderBook) {
        for t in self.data.iter() {
            let ts = match NaiveDateTime::parse_from_str(&t.timestamp, "%Y-%m-%dT%H:%M:%S%.3fZ") {
                Ok(dt) => DateTime::<Utc>::from_utc(dt, Utc),
                _ => Utc::now(),
            };
            let mut trade = CoreTrade {
                exchange: Exchange::OKEx,
                timestamp: ts,
                price: t.price,
                amount: t.qty,
                direction: TradeDirection::Buy,
            };
            if t.side == "sell" {
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

// Unit Tests

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn parse_timestamp() {
        let s = "2019-11-22T11:33:15.818Z";
        let r = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.3fZ");
        assert!(r.is_ok());

        assert_eq!(
            Utc.ymd(2019, 11, 22).and_hms_milli(11, 33, 15, 818),
            DateTime::<Utc>::from_utc(r.unwrap(), Utc)
        );
    }

    #[test]
    fn deserialize_spot_trade() {
        let s = "{\"table\":\"spot/trade\",\"data\":[{\"instrument_id\":\"ETH-\
                 USDT\",\"price\":\"175.79\",\"side\":\"buy\",\"size\":\"0.852\
                 437\",\"timestamp\":\"2019-11-20T03:22:48.504Z\",\"trade_id\"\
                 :\"2640006429\"}]}";
        assert!(serde_json::from_str::<WSSpotTradeResponse>(&s).is_ok());
    }

    #[test]
    fn deserialize_futures_depth() {
        let s = "{\"table\":\"futures/depth5\",\"data\":[{\"asks\":  [[\"8093.5\
                 4\",\"175\",\"0\",\"2\"],[\"8094.41\",\"40\",\"0\",\"1\"],[\"8\
                 094.5\",\"94\",\"0\",\"1\"],[\"8094.51\",\"360\",\"0\",\"1\"],\
                 [\"8095.1\",\"5\",\"0\",\"1\"]],\"bids\":[[\"8093.53\",\"595\"\
                 ,\"0\",\"4\"],[\"8093.26\",\"96\",\"0\",\"1\"],[\"8093.25\",\"\
                 35\",\"0\",\"1\"],[\"8093.09\",\"76\",\"0\",\"1\"],  [\"8092.8\
                 6\",\"20\",\"0\",\"2\"]],\"instrument_id\": \"BTC-USD-191122\"\
                 ,\"timestamp\":\"2019-11-21T06:34:30.537Z\"}]}";
        assert!(serde_json::from_str::<WSFuturesDepthResponse>(&s).is_ok());
    }

    #[test]
    fn deserialize_futures_trade() {
        let s = "{\"table\":\"futures/trade\",\"data\":[{\"side\":\"buy\",\"tra\
                 de_id\":\"26241748\",\"price\":\"6714.54\",\"qty\":\"1\",\"ins\
                 trument_id\":\"BTC-USD-191227\",\"timestamp\":\"2019-11-25T03:\
                 07:10.407Z\"}]}";
        assert!(serde_json::from_str::<WSFuturesTradeResponse>(&s).is_ok());
    }
}
