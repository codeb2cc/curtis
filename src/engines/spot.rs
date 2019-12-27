use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use actix::io::SinkWrite;
use actix::prelude::*;
use chrono::Utc;
use futures::{lazy, stream::Stream, Future};
use log::{debug, error, info, trace};
use tokio::{
    sync::{mpsc, watch},
    timer,
};

use curtis_binance::{adaptor::WSSpotActor as BinanceWSActor, models::BinanceConfig};
use curtis_core::models::{OrderBookUpdater, SpotTable, WSRequest, WatchEmpty};
use curtis_core::types::Exchange;
use curtis_core::utils::net::http1_client;
use curtis_gate::{adaptor::WSSpotActor as GateWSActor, models::GateConfig};
use curtis_huobi::{adaptor::WSSpotActor as HuobiWSActor, models::HuobiConfig};
use curtis_okex::{adaptor::WSSpotActor as OKExWSActor, models::OKExConfig};

use crate::config::Config;
use crate::tui;

#[derive(Debug)]
struct Tick;

impl Message for Tick {
    type Result = Result<bool, failure::Error>;
}

pub fn run(config: Config, tui_enabled: bool) {
    let quit = Arc::new(AtomicBool::new(false));

    // Different RX stream combinations have different types. Must predefine all posibile channels.
    let (binance_out_tx, binance_out_rx) =
        watch::channel::<Box<dyn OrderBookUpdater>>(Box::new(WatchEmpty {}));
    let (gate_out_tx, gate_out_rx) =
        watch::channel::<Box<dyn OrderBookUpdater>>(Box::new(WatchEmpty {}));
    let (huobi_out_tx, huobi_out_rx) =
        watch::channel::<Box<dyn OrderBookUpdater>>(Box::new(WatchEmpty {}));
    let (okex_out_tx, okex_out_rx) =
        watch::channel::<Box<dyn OrderBookUpdater>>(Box::new(WatchEmpty {}));
    let combined_out_rx = binance_out_rx
        .select(gate_out_rx)
        .select(huobi_out_rx)
        .select(okex_out_rx);

    // Binance WebSocket
    if let Some(config) = config.binance {
        if config.enabled {
            let (_in_tx, in_rx) = mpsc::unbounded_channel::<String>();
            let ws_config = config.clone();
            let _h = thread::Builder::new()
                .name("binance".to_string())
                .spawn(move || {
                    info!("Binance Thread Starts...");
                    let sys = System::new("binance");
                    binance_ws_start(ws_config, binance_out_tx, in_rx);
                    let _ = sys.run();
                })
                .unwrap();
        }
    }

    // Gate WebSocket
    if let Some(config) = config.gate {
        if config.enabled {
            let (_in_tx, in_rx) = mpsc::unbounded_channel::<String>();
            let ws_config = config.clone();
            let _h = thread::Builder::new()
                .name("gate".to_string())
                .spawn(move || {
                    info!("Gate Thread Starts...");
                    let sys = System::new("gate");
                    gate_ws_start(ws_config, gate_out_tx, in_rx);
                    let _ = sys.run();
                })
                .unwrap();
        }
    }

    // Huobi WebSocket
    if let Some(config) = config.huobi {
        if config.enabled {
            let (_in_tx, in_rx) = mpsc::unbounded_channel::<String>();
            let ws_config = config.clone();
            let _h = thread::Builder::new()
                .name("huobi".to_string())
                .spawn(move || {
                    info!("Huobi Thread Starts...");
                    let sys = System::new("huobi");
                    huobi_ws_start(ws_config, huobi_out_tx, in_rx);
                    let _ = sys.run();
                })
                .unwrap();
        }
    }

    // OKEx WebSocket
    if let Some(config) = config.okex {
        if config.enabled {
            let (_in_tx, in_rx) = mpsc::unbounded_channel::<String>();
            let ws_config = config.clone();
            let _h = thread::Builder::new()
                .name("okex".to_string())
                .spawn(move || {
                    info!("OKEx Thread Starts...");
                    let sys = System::new("okex");
                    okex_ws_start(ws_config, okex_out_tx, in_rx);
                    let _ = sys.run();
                })
                .unwrap();
        }
    }

    let table = Arc::new(RwLock::new(curtis_core::models::SpotTable::new(
        String::from("curtis"),
    )));

    // Reactor in main thread
    let table_th = table.clone();
    let quit_th = quit.clone();
    let reactor_th = thread::Builder::new()
        .name("engine".to_string())
        .spawn(move || {
            let sys = System::new("curtis");

            Arbiter::spawn(lazy(move || {
                timer::Interval::new(Instant::now(), Duration::from_millis(100))
                    .for_each(move |_| {
                        if quit_th.load(Ordering::Relaxed) {
                            System::current().stop();
                        }
                        Ok(())
                    })
                    .map_err(|_| ())
            }));

            let addr = {
                let table = table_th.clone();
                SyncArbiter::start(1, move || SpotEngineActor::new(table.clone()))
            };

            Arbiter::spawn(lazy(move || {
                combined_out_rx
                    .for_each(move |value| {
                        let mut t = table_th.write().unwrap();

                        t.timestamp = Utc::now();

                        match value.exchange() {
                            Exchange::Binance => value.update_book(&mut t.binance),
                            Exchange::Gate => value.update_book(&mut t.gate),
                            Exchange::Huobi => value.update_book(&mut t.huobi),
                            Exchange::OKEx => value.update_book(&mut t.okex),
                        }

                        if let Err(e) = addr.try_send(Tick) {
                            error!("{:?}", e);
                        }

                        Ok(())
                    })
                    .map_err(|_| ())
            }));
            let _ = sys.run();
        })
        .unwrap();

    if tui_enabled {
        let table_th = table.clone();
        let tui_th = thread::Builder::new()
            .name("tui".to_string())
            .spawn(move || {
                let _ = tui::spot::render(table_th);
            })
            .unwrap();
        let _ = tui_th.join();
    } else {
        let _ = reactor_th.join();
    }
}

fn binance_ws_start(
    config: BinanceConfig,
    tx: watch::Sender<Box<dyn OrderBookUpdater>>,
    rx: mpsc::UnboundedReceiver<String>,
) {
    // WebSocket handler
    Arbiter::spawn(lazy(move || {
        let endpoint = url::Url::parse(&config.spot_ws_endpoint).unwrap();

        http1_client()
            .ws(endpoint.as_str())
            .connect()
            .map_err(|e| {
                error!("Connection Error: {}", e);
            })
            .map(move |(response, framed)| {
                trace!("Websocket Connect: {:?}", response);
                let (sink, stream) = framed.split();
                let addr = BinanceWSActor::create(|ctx| {
                    BinanceWSActor::add_stream(stream, ctx);
                    BinanceWSActor {
                        sink: SinkWrite::new(sink, ctx),
                        tx,
                    }
                });

                let req =
                    r#"{"method":"SET_PROPERTY","params": ["combined",true],"id":1}"#.to_string();
                addr.do_send(WSRequest(req));

                if let Some(reqs) = config.init_requests {
                    for req in reqs.iter() {
                        addr.do_send(WSRequest(req.to_owned()));
                    }
                }

                // Handle pending request
                Arbiter::spawn(lazy(move || {
                    rx.for_each(move |value| {
                        addr.do_send(WSRequest(value));
                        Ok(())
                    })
                    .map_err(|_| ())
                }));
            })
    }));
}

fn gate_ws_start(
    config: GateConfig,
    tx: watch::Sender<Box<dyn OrderBookUpdater>>,
    rx: mpsc::UnboundedReceiver<String>,
) {
    // WebSocket handler
    Arbiter::spawn(lazy(move || {
        let endpoint = url::Url::parse(&config.spot_ws_endpoint).unwrap();

        http1_client()
            .ws(endpoint.as_str())
            .connect()
            .map_err(|e| {
                error!("Connection Error: {}", e);
            })
            .map(move |(response, framed)| {
                trace!("Websocket: {:?}", response);
                let (sink, stream) = framed.split();
                let addr = GateWSActor::create(|ctx| {
                    GateWSActor::add_stream(stream, ctx);
                    GateWSActor {
                        sink: SinkWrite::new(sink, ctx),
                        tx,
                    }
                });

                // Auth
                // let ts = Utc::now().timestamp_millis();
                // let sign = config.build_sign_ws(ts);
                // let auth = models::WSv3AuthRequest::new(config, ts, sign);
                // let auth_req = serde_json::to_string(&auth).unwrap();
                // addr.do_send(WSRequest(auth_req));

                if let Some(reqs) = config.init_requests {
                    for req in reqs.iter() {
                        addr.do_send(WSRequest(req.to_owned()));
                    }
                }

                // Handle pending request
                Arbiter::spawn(lazy(move || {
                    rx.for_each(move |value| {
                        addr.do_send(WSRequest(value));
                        Ok(())
                    })
                    .map_err(|_| ())
                }));
            })
    }));
}

fn huobi_ws_start(
    config: HuobiConfig,
    tx: watch::Sender<Box<dyn OrderBookUpdater>>,
    rx: mpsc::UnboundedReceiver<String>,
) {
    // WebSocket handler
    Arbiter::spawn(lazy(move || {
        let endpoint = url::Url::parse(&config.spot_ws_endpoint).unwrap();
        let host = endpoint.host().unwrap().to_string();

        http1_client()
            .ws(endpoint.as_str())
            .connect()
            .map_err(|e| {
                error!("Connection Error: {}", e);
            })
            .map(move |(response, framed)| {
                trace!("Websocket: {:?}", response);
                let (sink, stream) = framed.split();
                let addr = HuobiWSActor::create(|ctx| {
                    HuobiWSActor::add_stream(stream, ctx);
                    HuobiWSActor {
                        sink: SinkWrite::new(sink, ctx),
                        tx,
                    }
                });

                // TODO:
                if false {
                    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
                    let sign = config.build_sign_ws(host, ts.clone());
                    let auth = curtis_huobi::models::WSv1AuthRequest::new(config.key, ts, sign);
                    let auth_req = serde_json::to_string(&auth).unwrap();

                    trace!("WebSocket Auth: {}", auth_req);
                    addr.do_send(WSRequest(auth_req));
                }

                if let Some(reqs) = config.init_requests {
                    for req in reqs.iter() {
                        addr.do_send(WSRequest(req.to_owned()));
                    }
                }

                // Handle pending request
                Arbiter::spawn(lazy(move || {
                    rx.for_each(move |value| {
                        addr.do_send(WSRequest(value));
                        Ok(())
                    })
                    .map_err(|_| ())
                }));
            })
    }));
}

fn okex_ws_start(
    config: OKExConfig,
    tx: watch::Sender<Box<dyn OrderBookUpdater>>,
    rx: mpsc::UnboundedReceiver<String>,
) {
    let key = config.key.clone();
    let passphrase = config.passphrase.clone();

    // WebSocket handler
    Arbiter::spawn(lazy(move || {
        let endpoint = url::Url::parse(&config.spot_ws_endpoint).unwrap();

        http1_client()
            .ws(endpoint.as_str())
            .connect()
            .map_err(|e| {
                error!("Error: {}", e);
            })
            .map(move |(_response, framed)| {
                let (sink, stream) = framed.split();
                let addr = OKExWSActor::create(|ctx| {
                    OKExWSActor::add_stream(stream, ctx);
                    OKExWSActor {
                        sink: SinkWrite::new(sink, ctx),
                        tx,
                    }
                });

                let ts = chrono::offset::Local::now().timestamp();
                let sign = config.build_sign_ws(ts);
                let login = format!(
                    r#"{{"op":"login","args":["{}","{}","{}","{}"]}}{}"#,
                    key, passphrase, ts, sign, "\n\n",
                );
                addr.do_send(WSRequest(login));

                if let Some(reqs) = config.init_requests {
                    for req in reqs.iter() {
                        addr.do_send(WSRequest(req.to_owned()));
                    }
                }

                Arbiter::spawn(lazy(move || {
                    rx.for_each(move |value| {
                        addr.do_send(WSRequest(value));
                        Ok(())
                    })
                    .map_err(|_| ())
                }));
            })
    }));
}

struct SpotEngineActor {
    table: Arc<RwLock<SpotTable>>,

    last_trade_price: f64,
}

impl SpotEngineActor {
    pub fn new(table: Arc<RwLock<SpotTable>>) -> SpotEngineActor {
        SpotEngineActor {
            table,
            last_trade_price: 0.0,
        }
    }
}

impl Actor for SpotEngineActor {
    type Context = SyncContext<Self>;

    fn started(&mut self, _ctx: &mut SyncContext<Self>) {
        debug!("Actor is alive");
    }

    fn stopped(&mut self, _ctx: &mut SyncContext<Self>) {
        debug!("Actor is stopped");
    }
}

impl Handler<Tick> for SpotEngineActor {
    type Result = Result<bool, failure::Error>;

    fn handle(&mut self, _msg: Tick, _ctx: &mut SyncContext<Self>) -> Self::Result {
        // Simple arbitrage POC
        let table = self.table.read().unwrap();
        let bids = vec![
            table.binance.bids.first(),
            table.gate.bids.first(),
            table.huobi.bids.first(),
            table.okex.bids.first(),
        ];
        let asks = vec![
            table.binance.asks.first(),
            table.gate.asks.first(),
            table.huobi.asks.first(),
            table.okex.asks.first(),
        ];

        // Asks
        let mut min_exchange = Exchange::Binance;
        let min_asks = asks.iter().fold(std::f64::MAX, |acc, ask| {
            if let Some(order) = ask {
                if order.price < acc {
                    min_exchange = order.exchange;
                    order.price
                } else {
                    acc
                }
            } else {
                acc
            }
        });

        for item in bids.iter() {
            if let Some(bid) = item {
                if bid.price / min_asks > 1.003 {
                    debug!(
                        "{:24} | {:32}",
                        format!("ASK BID: {}/{}", min_exchange, min_asks),
                        format!(
                            "BID: {}/{} ({:.5})",
                            bid.exchange,
                            bid.price,
                            bid.price / min_asks,
                        ),
                    );
                }
            }
        }

        // Bids
        let mut min_exchange = Exchange::Binance;
        let min_bid = bids.iter().fold(std::f64::MAX, |acc, bid| {
            if let Some(order) = bid {
                if order.price < acc {
                    min_exchange = order.exchange;
                    order.price
                } else {
                    acc
                }
            } else {
                acc
            }
        });
        let next_bid = match min_exchange {
            Exchange::OKEx => min_bid + 0.01,
            Exchange::Binance => min_bid + 0.001,
            _ => min_bid + 0.0001,
        };

        for item in bids.iter() {
            if let Some(bid) = item {
                if bid.price / next_bid > 1.003 {
                    debug!(
                        "{:24} | {:32}",
                        format!("NEXT BID: {}/{}", min_exchange, next_bid),
                        format!(
                            "BID: {}/{} ({:.5})",
                            bid.exchange,
                            bid.price,
                            bid.price / next_bid,
                        ),
                    );
                }
            }
        }

        // New Trade
        let mut new_trades = Vec::new();
        for t in table
            .binance
            .sells
            .iter()
            .chain(table.gate.sells.iter())
            .chain(table.huobi.sells.iter())
            .chain(table.okex.sells.iter())
        {
            if Utc::now() - t.timestamp > chrono::Duration::milliseconds(100) {
                new_trades.push(t);
            }
        }
        if !new_trades.is_empty() {
            new_trades.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

            let t = &new_trades[0];
            if let Some(std::cmp::Ordering::Equal) = self.last_trade_price.partial_cmp(&t.price) {
                return Ok(true);
            } else {
                self.last_trade_price = t.price;
            }

            let fs = vec![
                table.binance.bids.first(),
                table.gate.bids.first(),
                table.huobi.bids.first(),
                table.okex.bids.first(),
            ];
            for item in fs.iter() {
                if let Some(bid) = item {
                    if bid.price / t.price > 1.003 {
                        let order = match t.exchange {
                            Exchange::Binance => table.binance.bids.first(),
                            Exchange::Gate => table.gate.bids.first(),
                            Exchange::Huobi => table.huobi.bids.first(),
                            Exchange::OKEx => table.okex.bids.first(),
                        };
                        let bid1 = match order {
                            Some(o) => o.price,
                            None => 0.0,
                        };

                        debug!(
                            "{:32} | {:32} | {:40}",
                            format!(
                                "LAST SELL: {} {}/{}",
                                t.timestamp.format("%H:%M:%S%.3f"),
                                t.exchange,
                                t.price
                            ),
                            format!(
                                "BID: {}/{} ({:.5})",
                                bid.exchange,
                                bid.price,
                                bid.price / t.price,
                            ),
                            format!(
                                "SELL-BID1: {}/{}/{} ({:.5})",
                                t.exchange,
                                bid1,
                                bid.price,
                                bid.price / bid1,
                            ),
                        );
                    }
                }
            }
        }

        Ok(true)
    }
}
