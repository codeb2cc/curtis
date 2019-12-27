use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use actix::{io::SinkWrite, Actor, Arbiter, StreamHandler, System};
use chrono::Utc;
use futures::{lazy, stream::Stream, Future};
use log::{error, info, trace};
use tokio::{
    sync::{mpsc, watch},
    timer,
};

use curtis_binance::{adaptor::WSFuturesActor as BinanceWSActor, models::BinanceConfig};
use curtis_core::models::{OrderBookUpdater, WSRequest, WatchEmpty};
use curtis_core::types::Exchange;
use curtis_core::utils::net::http1_client;
use curtis_gate::{adaptor::WSFuturesActor as GateWSActor, models::GateConfig};
use curtis_huobi::{adaptor::WSFuturesActor as HuobiWSActor, models::HuobiConfig};
use curtis_okex::{adaptor::WSFuturesActor as OKExWSActor, models::OKExConfig};

use crate::config::Config;
use crate::tui;

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

    let table = Arc::new(RwLock::new(curtis_core::models::FuturesTable::new(
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
                let _ = tui::futures::render(table_th);
            })
            .unwrap();
        let _ = tui_th.join();
    } else {
        let _ = reactor_th.join();
    }

    quit.store(true, Ordering::Relaxed);
    thread::sleep(::std::time::Duration::from_millis(100));
}

fn binance_ws_start(
    config: BinanceConfig,
    tx: watch::Sender<Box<dyn OrderBookUpdater>>,
    rx: mpsc::UnboundedReceiver<String>,
) {
    // WebSocket handler
    Arbiter::spawn(lazy(move || {
        let endpoint = url::Url::parse(&config.futures_ws_endpoint).unwrap();

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

                let set_req =
                    r#"{"method":"SET_PROPERTY","params": ["combined",true],"id":1}"#.to_string();
                addr.do_send(WSRequest(set_req));
                let sub_req = r#"{"method":"SUBSCRIBE","params":["btcusdt@depth10@100ms", "btcusdt@aggTrade"],"id":2}"#
                    .to_string();
                addr.do_send(WSRequest(sub_req));

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
        let endpoint = url::Url::parse(&config.futures_ws_endpoint).unwrap();

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

                // trace!(LOG, "WebSocket Auth: {}", auth_req);
                // addr.do_send(WSRequest(auth_req));

                let req = r#"{"time":0,"channel":"futures.order_book","event":"subscribe","payload":["BTC_USD","10","0"]}"#.to_string();
                addr.do_send(WSRequest(req));
                let req = r#"{"time":1,"channel":"futures.trades","event":"subscribe","payload":["BTC_USD"]}"#.to_string();
                addr.do_send(WSRequest(req));

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
        let endpoint = url::Url::parse(&config.futures_ws_endpoint).unwrap();
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

                if false {
                    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
                    let sign = config.build_sign_ws(host, ts.clone());
                    let auth = curtis_huobi::models::WSv1AuthRequest::new(config.key, ts, sign);
                    let auth_req = serde_json::to_string(&auth).unwrap();

                    trace!("WebSocket Auth: {}", auth_req);
                    addr.do_send(WSRequest(auth_req));
                }

                let sub_req = r#"{"sub": "market.BTC_CQ.depth.step0", "id": "1"}"#.to_string();
                addr.do_send(WSRequest(sub_req));
                let sub_req = r#"{"sub": "market.BTC_CQ.trade.detail", "id": "2"}"#.to_string();
                addr.do_send(WSRequest(sub_req));

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
        let endpoint = url::Url::parse(&config.futures_ws_endpoint).unwrap();

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

                // Futures
                let req =
                    r#"{"op": "subscribe", "args": ["futures/depth5:BTC-USD-191227"]}"#.to_string();
                addr.do_send(WSRequest(req));
                let req =
                    r#"{"op": "subscribe", "args": ["futures/trade:BTC-USD-191227"]}"#.to_string();
                addr.do_send(WSRequest(req));

                // Swap
                // let req =
                //     r#"{"op": "subscribe", "args": ["swap/depth5:BTC-USD-SWAP"]}"#.to_string();
                // addr.do_send(WSRequest(req));
                // let req =
                //     r#"{"op": "subscribe", "args": ["swap/trade:BTC-USD-SWAP"]}"#.to_string();
                // addr.do_send(WSRequest(req));

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
