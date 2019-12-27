use std::io::Read;
use std::time::Duration;

use actix::io::SinkWrite;
use actix::{
    io::WriteHandler, Actor, ActorContext, AsyncContext, Context, Handler, StreamHandler, System,
};
use actix_codec::{AsyncRead, AsyncWrite, Framed};
use actix_http::http::header::CONTENT_TYPE;
use awc::{
    self,
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
};
use flate2::read::DeflateDecoder;
use futures::stream::SplitSink;
use log::{error, trace};
use tokio::sync::watch;

use crate::models;
use crate::models::OKExConfig;
use curtis_core::models::{OrderBookUpdater, WSRequest};

trait OKExRequest {
    fn prepare_header(self, config: &OKExConfig) -> Self;
}

impl OKExRequest for awc::ClientRequest {
    fn prepare_header(self, config: &OKExConfig) -> Self {
        let method = self.get_method().to_string();
        let path = self.get_uri().path().to_string();

        let now = chrono::offset::Utc::now();
        let ts = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let sign = config.build_sign(ts.clone(), method, path);

        self.header(
            "User-Agent",
            "Mozilla/5.0 (compatible; MSIE 9.0; Windows Phone OS 7.5; Trident/5.0; IEMobile/9.0)",
        )
        .header(CONTENT_TYPE, "application/json")
        .header("OK-ACCESS-KEY", config.key.clone())
        .header("OK-ACCESS-SIGN", sign)
        .header("OK-ACCESS-TIMESTAMP", ts)
        .header("OK-ACCESS-PASSPHRASE", config.passphrase.clone())
    }
}

pub struct OKExHttpClient {
    config: OKExConfig,
    client: awc::Client,
}

impl OKExHttpClient {
    pub fn new(config: OKExConfig) -> OKExHttpClient {
        OKExHttpClient {
            config,
            client: awc::Client::default(),
        }
    }

    pub fn account_wallet(&self) -> awc::SendClientRequest {
        self.client
            .get("https://www.okex.com/api/account/v3/wallet")
            .prepare_header(&self.config)
            .send()
    }

    pub fn margin_accounts(&self) -> awc::SendClientRequest {
        self.client
            .get("https://www.okex.com/api/margin/v3/accounts")
            .prepare_header(&self.config)
            .send()
    }
}

// Spot

pub struct WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    pub sink: SinkWrite<SplitSink<Framed<T, Codec>>>,

    pub tx: watch::Sender<Box<dyn OrderBookUpdater>>,
}

impl<T: 'static> Actor for WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        // println!("Disconnected");

        // Stop application on disconnect
        System::current().stop();
    }
}

impl<T: 'static> WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.sink.write(Message::Ping(String::new())).unwrap();
            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }
}

/// Handle stdin commands
impl<T: 'static> Handler<WSRequest> for WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Result = ();

    fn handle(&mut self, msg: WSRequest, _ctx: &mut Context<Self>) {
        self.sink.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl<T: 'static> StreamHandler<Frame, WsProtocolError> for WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn handle(&mut self, msg: Frame, _ctx: &mut Context<Self>) {
        match msg {
            Frame::Text(_txt) => {}
            Frame::Binary(b) => {
                if let Some(data) = b {
                    let mut d = DeflateDecoder::new(&data[..]);
                    let mut s = String::new();
                    d.read_to_string(&mut s).unwrap();
                    trace!("WebSocket Receive(Binary): {:?}", s);

                    if let Ok(response) = serde_json::from_str::<models::WSTableResponse>(&s) {
                        match response.table.as_str() {
                            "spot/depth5" => {
                                match serde_json::from_str::<models::WSSpotDepthResponse>(&s) {
                                    Ok(response) => {
                                        let _ = self.tx.broadcast(Box::new(response));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                            "spot/trade" => {
                                match serde_json::from_str::<models::WSSpotTradeResponse>(&s) {
                                    Ok(response) => {
                                        let _ = self.tx.broadcast(Box::new(response));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                            _ => (),
                        }
                    }
                };
            }
            _ => (),
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        // println!("Connected");
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        error!("Server disconnected");
        ctx.stop()
    }
}

impl<T: 'static> WriteHandler<WsProtocolError> for WSSpotActor<T> where T: AsyncRead + AsyncWrite {}

// Futures
pub struct WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    pub sink: SinkWrite<SplitSink<Framed<T, Codec>>>,

    pub tx: watch::Sender<Box<dyn OrderBookUpdater>>,
}

impl<T: 'static> Actor for WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        // println!("Disconnected");

        // Stop application on disconnect
        System::current().stop();
    }
}

impl<T: 'static> WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.sink.write(Message::Ping(String::new())).unwrap();
            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }
}

/// Handle stdin commands
impl<T: 'static> Handler<WSRequest> for WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Result = ();

    fn handle(&mut self, msg: WSRequest, _ctx: &mut Context<Self>) {
        self.sink.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl<T: 'static> StreamHandler<Frame, WsProtocolError> for WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn handle(&mut self, msg: Frame, _ctx: &mut Context<Self>) {
        match msg {
            Frame::Text(_txt) => {}
            Frame::Binary(b) => {
                if let Some(data) = b {
                    let mut d = DeflateDecoder::new(&data[..]);
                    let mut s = String::new();
                    d.read_to_string(&mut s).unwrap();
                    trace!("WebSocket Receive(Binary): {:?}", s);

                    if let Ok(response) = serde_json::from_str::<models::WSTableResponse>(&s) {
                        match response.table.as_str() {
                            "swap/trade" => {
                                match serde_json::from_str::<models::WSSpotTradeResponse>(&s) {
                                    Ok(response) => {
                                        let _ = self.tx.broadcast(Box::new(response));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                            "futures/depth5" | "swap/depth5" => {
                                match serde_json::from_str::<models::WSFuturesDepthResponse>(&s) {
                                    Ok(response) => {
                                        let _ = self.tx.broadcast(Box::new(response));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                            "futures/trade" => {
                                match serde_json::from_str::<models::WSFuturesTradeResponse>(&s) {
                                    Ok(response) => {
                                        let _ = self.tx.broadcast(Box::new(response));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                            _ => (),
                        }
                    }
                };
            }
            _ => (),
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        // println!("Connected");
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        error!("Server disconnected");
        ctx.stop()
    }
}

impl<T: 'static> WriteHandler<WsProtocolError> for WSFuturesActor<T> where T: AsyncRead + AsyncWrite {}
