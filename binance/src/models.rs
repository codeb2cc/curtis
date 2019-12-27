use chrono::offset::{LocalResult, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

use curtis_core::models::{Order as CoreOrder, OrderBook, OrderBookUpdater, Trade, TradeDirection};
use curtis_core::{types::Exchange, utils::serde::de_str2num};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct BinanceConfig {
    pub enabled: bool,
    pub key: String,
    pub secret: String,
    pub spot_ws_endpoint: String,
    pub futures_ws_endpoint: String,

    pub init_requests: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct WSResponse {
    pub id: u64,
}

#[derive(Deserialize, Debug)]
pub struct WSEvent {
    #[serde(rename(deserialize = "e"))]
    pub event_type: String,
}

#[derive(Deserialize, Debug)]
pub struct WSCombinedStream {
    pub stream: String,
    #[serde(skip)]
    pub data: Option<()>,
}

#[derive(Debug)]
pub struct OrderRaw(f64, f64);

#[derive(Clone, Deserialize, Debug)]
pub struct Order {
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(deserialize_with = "de_str2num")]
    pub quantity: f64,
}

impl<'de> Deserialize<'de> for OrderRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
            .map(|Order { price, quantity }| OrderRaw(price, quantity))
    }
}

// WebSocket Spot

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotCombinedPartialBookDepth {
    pub stream: String,
    pub data: WSSpotPartialBookDepth,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotCombinedTrade {
    pub stream: String,
    pub data: WSSpotTrade,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotPartialBookDepth {
    #[serde(rename(deserialize = "lastUpdateId"))]
    pub last_update_id: u64,
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
}

impl OrderBookUpdater for WSSpotPartialBookDepth {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn update_book(&self, book: &mut OrderBook) {
        book.timestamp = Utc::now();

        book.bids.clear();
        for o in self.bids.iter() {
            book.bids.push(CoreOrder {
                exchange: Exchange::Binance,
                price: o.price,
                amount: o.quantity,
            });
        }

        book.asks.clear();
        for o in self.asks.iter() {
            book.asks.push(CoreOrder {
                exchange: Exchange::Binance,
                price: o.price,
                amount: o.quantity,
            });
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSSpotTrade {
    #[serde(rename(deserialize = "e"))]
    pub event_type: String,
    #[serde(rename(deserialize = "E"))]
    pub event_time: i64,
    #[serde(rename(deserialize = "s"))]
    pub symbol: String,
    #[serde(rename(deserialize = "t"))]
    pub trade_id: i64,
    #[serde(rename(deserialize = "p"))]
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(rename(deserialize = "q"))]
    #[serde(deserialize_with = "de_str2num")]
    pub quantity: f64,
    #[serde(rename(deserialize = "b"))]
    pub buyer_order_id: i64,
    #[serde(rename(deserialize = "a"))]
    pub seller_order_id: i64,
    #[serde(rename(deserialize = "T"))]
    pub trade_time: i64,
    #[serde(rename(deserialize = "m"))]
    pub buyer_is_maker: bool,
    #[serde(skip)]
    #[serde(rename(deserialize = "M"))]
    pub _unknown: bool,
}

impl OrderBookUpdater for WSSpotTrade {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn update_book(&self, book: &mut OrderBook) {
        let ts = match Utc.timestamp_millis_opt(self.trade_time) {
            LocalResult::Single(ts) => ts,
            _ => Utc::now(),
        };
        let mut trade = Trade {
            exchange: Exchange::Binance,
            timestamp: ts,
            price: self.price,
            amount: self.quantity,
            direction: TradeDirection::Buy,
        };
        if self.buyer_is_maker {
            trade.direction = TradeDirection::Sell;
            book.sells.push_front(trade);
            book.sells.truncate(10);
        } else {
            book.buys.push_front(trade);
            book.buys.truncate(10);
        }
    }
}

// WebSocket Futures

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesCombinedPartialBookDepth {
    pub stream: String,
    pub data: WSFuturesPartialBookDepth,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesPartialBookDepth {
    #[serde(rename(deserialize = "e"))]
    pub event_type: String,
    #[serde(rename(deserialize = "E"))]
    pub event_time: i64,
    #[serde(rename(deserialize = "T"))]
    pub transaction_time: i64,
    #[serde(rename(deserialize = "s"))]
    pub symbol: String,

    #[serde(skip)]
    #[serde(rename(deserialize = "U"))]
    pub _u1: i64,
    #[serde(skip)]
    #[serde(rename(deserialize = "u"))]
    pub _u2: i64,
    #[serde(skip)]
    #[serde(rename(deserialize = "pu"))]
    pub _u3: i64,

    #[serde(rename(deserialize = "b"))]
    pub bids: Vec<Order>,
    #[serde(rename(deserialize = "a"))]
    pub asks: Vec<Order>,
}

impl OrderBookUpdater for WSFuturesPartialBookDepth {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn update_book(&self, book: &mut OrderBook) {
        // TODO: use `_opt` method
        book.timestamp = Utc.timestamp_millis(self.transaction_time);

        book.bids.clear();
        for o in self.bids.iter() {
            book.bids.push(CoreOrder {
                exchange: Exchange::Binance,
                price: o.price,
                amount: o.quantity,
            });
        }

        book.asks.clear();
        for o in self.asks.iter() {
            book.asks.push(CoreOrder {
                exchange: Exchange::Binance,
                price: o.price,
                amount: o.quantity,
            });
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesCombinedAggregateTrade {
    pub stream: String,
    pub data: WSFuturesAggregateTrade,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WSFuturesAggregateTrade {
    #[serde(rename(deserialize = "e"))]
    pub event_type: String,
    #[serde(rename(deserialize = "E"))]
    pub event_time: i64,
    #[serde(rename(deserialize = "s"))]
    pub symbol: String,
    #[serde(rename(deserialize = "a"))]
    pub trade_id: i64,
    #[serde(rename(deserialize = "T"))]
    pub trade_time: i64,
    #[serde(rename(deserialize = "p"))]
    #[serde(deserialize_with = "de_str2num")]
    pub price: f64,
    #[serde(rename(deserialize = "q"))]
    #[serde(deserialize_with = "de_str2num")]
    pub quantity: f64,
    #[serde(rename(deserialize = "f"))]
    pub first_trade_id: i64,
    #[serde(rename(deserialize = "l"))]
    pub last_trade_id: i64,
    #[serde(rename(deserialize = "m"))]
    pub buyer_is_maker: bool,
}

impl OrderBookUpdater for WSFuturesAggregateTrade {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn update_book(&self, book: &mut OrderBook) {
        let ts = match Utc.timestamp_millis_opt(self.trade_time) {
            LocalResult::Single(ts) => ts,
            _ => Utc::now(),
        };
        let mut trade = Trade {
            exchange: Exchange::Binance,
            timestamp: ts,
            price: self.price,
            amount: self.quantity,
            direction: TradeDirection::Buy,
        };
        if self.buyer_is_maker {
            trade.direction = TradeDirection::Sell;
            book.sells.push_front(trade);
            book.sells.truncate(10);
        } else {
            book.buys.push_front(trade);
            book.buys.truncate(10);
        }
    }
}

// Unit Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize() {
        let s = "{\"lastUpdateId\":1374673057,\"bids\":[[\"8131.98000000\",\"4\
                 .10000000\"],[\"8131.95000000\",\"0.20000000\"],[\"8131.94000\
                 000\",\"2.00000000\"],[\"8131.82000000\",\"2.10000000\"],[\"8\
                 131.76000000\",\"0.05122000\"],[\"8131.62000000\",\"0.7007290\
                 0\"],[\"8131.61000000\",\"0.00190000\"],[\"8131.08000000\",\"\
                 0.05143800\"],[\"8131.07000000\",\"0.05000000\"],[\"8130.9100\
                 0000\",\"0.40479800\"]],\"asks\":[[\"8131.99000000\",\"0.7542\
                 9000\"],[\"8133.75000000\",\"0.18135800\"],[\"8133.79000000\"\
                 ,\"0.09373000\"],[\"8133.94000000\",\"1.55530600\"],[\"8133.9\
                 5000000\",\"2.10000000\"],[\"8133.96000000\",\"2.00000000\"],\
                 [\"8133.97000000\",\"1.24948100\"],[\"8133.98000000\",\"2.100\
                 00000\"],[\"8134.29000000\",\"2.17841300\"]]}";
        assert!(serde_json::from_str::<WSSpotPartialBookDepth>(&s).is_ok());
    }

    #[test]
    fn deserialize_futures_combined_partial_book_depth() {
        let s = "{\"stream\":\"btcusdt@depth10@100ms\",\"data\":{\"e\":\"depth\
                 Update\",\"E\":1574425182123,\"T\":1574425182117,\"s\":\"BTCU\
                 SDT\",\"U\":799743591,\"u\":799743794,\"pu\":799743589,\"b\":\
                 [[\"7077.64\",\"0.075\"],[\"7077.37\",\"0.462\"],[\"7077.28\"\
                 ,\"0.392\"],[\"7077.16\",\"0.300\"],[\"7076.96\",\"1.248\"], \
                 [\"7076.39\",\"4.195\"],[\"7075.91\",\"0.300\"],[\"7075.52\",\
                 \"2.000\"],[\"7075.22\",\"1.232\"],[\"7075.21\",\"0.020\"]], \
                 \"a\":[[\"7078.98\",\"0.015\"],[\"7078.99\",\"2.600\"],[\"707\
                 9.25\",\"0.430\"],[\"7081.03\",\"1.563\"],[\"7081.04\",\"0.04\
                 0\"],[\"7081.06\",\"0.005\"],[\"7081.07\",\"0.500\"],[\"7081.\
                 08\",\"2.400\"],[\"7081.12\",\"1.350\"],[\"7081.13\", \"2.920\
                 \"]]}}";
        assert!(serde_json::from_str::<WSFuturesCombinedPartialBookDepth>(&s).is_ok());
    }

    #[test]
    fn deserialize_combined_trade() {
        let s = "{\"stream\":\"bchabcusdt@trade\",\"data\":{\"e\":\"trade\",\"\
                 E\":1574404951586,\"s\":\"BCHABCUSDT\",\"t\":20659948,  \"p\"\
                 :\"222.26000000\",\"q\":\"0.04000000\",\"b\":240366642,\"a\":\
                 240366724,\"T\":1574404951581,\"m\":true,\"M\":true}}";
        assert!(serde_json::from_str::<WSSpotCombinedTrade>(&s).is_ok());
    }
}
