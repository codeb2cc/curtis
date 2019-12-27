use std::io::Read;
use std::time::Duration;

use actix::io::SinkWrite;
use actix::{
    io::WriteHandler, Actor, ActorContext, AsyncContext, Context, Handler, StreamHandler, System,
};
use actix_codec::{AsyncRead, AsyncWrite, Framed};
use awc::{
    self,
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
};
use flate2::read::GzDecoder;
use futures::stream::SplitSink;
use log::{error, trace};
use tokio::sync::watch;

use crate::models;
use curtis_core::models::{OrderBookUpdater, WSRequest};

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
            Frame::Ping(_) => {
                self.sink.write(Message::Pong(String::new())).unwrap();
            }
            Frame::Text(txt) => {
                trace!("WebSocket Receive(Text): {:?}", txt);
                // v3 Spot
                if let Some(s) = txt {
                    if let Ok(response) = serde_json::from_slice::<models::WSv3Response>(&s) {
                        if response.result.status != "success" {
                            error!("WebSocket Response Error: {:?}", response);
                            return;
                        }
                    }

                    if let Ok(update) = serde_json::from_slice::<models::WSv3Notification>(&s) {
                        if update.method == "depth.update" {
                            match serde_json::from_slice::<models::WSSpotDepthUpdate>(&s) {
                                Ok(data) => {
                                    let _ = self.tx.broadcast(Box::new(data));
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        } else if update.method == "trades.update" {
                            match serde_json::from_slice::<models::WSSpotTradeUpdate>(&s) {
                                Ok(data) => {
                                    let _ = self.tx.broadcast(Box::new(data));
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                }
            }
            Frame::Binary(b) => {
                if let Some(data) = b {
                    let mut d = GzDecoder::new(&data[..]);
                    let mut s = String::new();
                    d.read_to_string(&mut s).unwrap();
                    trace!("WebSocket Receive(Binary): {:?}", s);
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
            Frame::Ping(_) => {
                self.sink.write(Message::Pong(String::new())).unwrap();
            }
            Frame::Text(txt) => {
                trace!("WebSocket Receive(Text): {:?}", txt);
                if let Some(s) = txt {
                    // v4 Futures
                    if let Ok(response) = serde_json::from_slice::<models::WSv4Response>(&s) {
                        if response.error.is_some() {
                            error!("WebSocket Response Error: {:?}", response);
                            return;
                        }

                        if response.channel == "futures.order_book" {
                            if response.event == "all" {
                                match serde_json::from_slice::<models::WSv4FuturesDepthAll>(&s) {
                                    Ok(data) => {
                                        let _ = self.tx.broadcast(Box::new(data));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            } else if response.event == "update" {
                                match serde_json::from_slice::<models::WSv4FuturesDepthUpdate>(&s) {
                                    Ok(data) => {
                                        let _ = self.tx.broadcast(Box::new(data));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            }
                        } else if response.channel == "futures.trades" {
                            if response.event == "update" {
                                match serde_json::from_slice::<models::WSv4FuturesTradeUpdate>(&s) {
                                    Ok(data) => {
                                        let _ = self.tx.broadcast(Box::new(data));
                                    }
                                    Err(e) => error!("{:?}", e),
                                }
                            } else {
                                // response.event == "subscribe"
                            }
                        }
                    }
                }
            }
            Frame::Binary(b) => {
                if let Some(data) = b {
                    let mut d = GzDecoder::new(&data[..]);
                    let mut s = String::new();
                    d.read_to_string(&mut s).unwrap();
                    trace!("WebSocket Receive(Binary): {:?}", s);
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
