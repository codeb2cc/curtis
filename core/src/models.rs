use std::collections::VecDeque;
use std::fmt;

use chrono::{offset::Utc, DateTime};

use crate::types;

#[derive(actix::Message)]
pub struct WSRequest(pub String);

#[derive(Clone, Debug)]
pub struct Order {
    pub exchange: types::Exchange,
    pub price: f64,
    pub amount: f64,
}

#[derive(Clone, Debug)]
pub enum TradeDirection {
    Buy,
    Sell,
}

impl fmt::Display for TradeDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TradeDirection::Buy => write!(f, "BUY "),
            TradeDirection::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Trade {
    pub exchange: types::Exchange,
    pub timestamp: DateTime<Utc>,
    pub price: f64,
    pub amount: f64,
    pub direction: TradeDirection,
}

#[derive(Clone)]
pub struct OrderBook {
    pub exchange: String,
    pub timestamp: DateTime<Utc>,

    pub asks: Vec<Order>,
    pub bids: Vec<Order>,

    pub buys: VecDeque<Trade>,
    pub sells: VecDeque<Trade>,
}

impl OrderBook {
    pub fn new(exchange: String) -> OrderBook {
        OrderBook {
            exchange,
            timestamp: Utc::now(),
            asks: Vec::new(),
            bids: Vec::new(),
            buys: VecDeque::new(),
            sells: VecDeque::new(),
        }
    }
}

pub trait OrderBookUpdater: OrderBookUpdaterClone + Send + Sync {
    fn exchange(&self) -> types::Exchange;
    fn update_book(&self, book: &mut OrderBook);
}

// Clone for Trait: https://stackoverflow.com/a/30353928
pub trait OrderBookUpdaterClone {
    fn clone_box(&self) -> Box<dyn OrderBookUpdater>;
}

impl<T> OrderBookUpdaterClone for T
where
    T: 'static + OrderBookUpdater + Clone,
{
    fn clone_box(&self) -> Box<dyn OrderBookUpdater> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn OrderBookUpdater> {
    fn clone(&self) -> Box<dyn OrderBookUpdater> {
        self.clone_box()
    }
}

#[derive(Clone)]
pub struct WatchEmpty {}

impl OrderBookUpdater for WatchEmpty {
    fn exchange(&self) -> types::Exchange {
        types::Exchange::Binance
    }

    fn update_book(&self, _book: &mut OrderBook) {}
}

#[derive(Clone)]
pub struct SpotTable {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub binance: OrderBook,
    pub gate: OrderBook,
    pub huobi: OrderBook,
    pub okex: OrderBook,
}

impl SpotTable {
    pub fn new(id: String) -> SpotTable {
        SpotTable {
            id,
            timestamp: Utc::now(),
            binance: OrderBook::new(String::from("Binance")),
            gate: OrderBook::new(String::from("Gate")),
            huobi: OrderBook::new(String::from("Huobi")),
            okex: OrderBook::new(String::from("OKEx")),
        }
    }
}

pub struct FuturesTable {
    pub id: String,
    pub timestamp: DateTime<Utc>,

    pub binance: OrderBook,
    pub huobi: OrderBook,
    pub gate: OrderBook,
    pub okex: OrderBook,
}

impl FuturesTable {
    pub fn new(id: String) -> FuturesTable {
        FuturesTable {
            id,
            timestamp: Utc::now(),
            binance: OrderBook::new(String::from("Binance")),
            huobi: OrderBook::new(String::from("Huobi")),
            gate: OrderBook::new(String::from("Gate")),
            okex: OrderBook::new(String::from("OKEx")),
        }
    }
}
