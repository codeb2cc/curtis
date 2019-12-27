use std::time::Duration;

use actix::io::SinkWrite;
use actix::{
    io::WriteHandler, Actor, ActorContext, AsyncContext, Context, Handler, StreamHandler, System,
};
use actix_codec::{AsyncRead, AsyncWrite, Framed};
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
};
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
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
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

impl<T: 'static> Handler<WSRequest> for WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Result = ();

    fn handle(&mut self, msg: WSRequest, _ctx: &mut Context<Self>) {
        if let Err(e) = self.sink.write(Message::Text(msg.0)) {
            error!("WebSocket Send Error: {:?}", e);
        }
    }
}

impl<T: 'static> StreamHandler<Frame, WsProtocolError> for WSSpotActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn handle(&mut self, msg: Frame, _ctx: &mut Context<Self>) {
        match msg {
            Frame::Ping(_) => {
                self.sink.write(Message::Pong(String::new())).unwrap();
            }
            Frame::Pong(_) => {
                trace!("WebSocket Receive(Pong): _");
            }
            Frame::Text(txt) => {
                trace!("WebSocket Receive(Text): {:?}", txt);
                // Spot
                if let Some(s) = txt {
                    if let Ok(combined) = serde_json::from_slice::<models::WSCombinedStream>(&s) {
                        if combined.stream.ends_with("@depth10@100ms") {
                            match serde_json::from_slice::<models::WSSpotCombinedPartialBookDepth>(
                                &s,
                            ) {
                                Ok(d) => {
                                    let _ = self.tx.broadcast(Box::new(d.data));
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        } else if combined.stream.ends_with("@trade") {
                            match serde_json::from_slice::<models::WSSpotCombinedTrade>(&s) {
                                Ok(d) => {
                                    let _ = self.tx.broadcast(Box::new(d.data));
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                }
            }
            Frame::Binary(b) => {
                if let Some(data) = b {
                    trace!("WebSocket Receive(Binary): {:?}", data);
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
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
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

impl<T: 'static> Handler<WSRequest> for WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Result = ();

    fn handle(&mut self, msg: WSRequest, _ctx: &mut Context<Self>) {
        if let Err(e) = self.sink.write(Message::Text(msg.0)) {
            error!("WebSocket Send Error: {:?}", e);
        }
    }
}

impl<T: 'static> StreamHandler<Frame, WsProtocolError> for WSFuturesActor<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn handle(&mut self, msg: Frame, _ctx: &mut Context<Self>) {
        match msg {
            Frame::Ping(_) => {
                self.sink.write(Message::Pong(String::new())).unwrap();
            }
            Frame::Pong(_) => {
                trace!("WebSocket Receive(Pong): _");
            }
            Frame::Text(txt) => {
                trace!("WebSocket Receive(Text): {:?}", txt);
                // Futures
                if let Some(s) = txt {
                    if let Ok(combined) = serde_json::from_slice::<models::WSCombinedStream>(&s) {
                        if combined.stream.ends_with("@depth10@100ms") {
                            if let Ok(d) = serde_json::from_slice::<
                                models::WSFuturesCombinedPartialBookDepth,
                            >(&s)
                            {
                                let _ = self.tx.broadcast(Box::new(d.data));
                            }
                        } else if combined.stream.ends_with("@aggTrade") {
                            match serde_json::from_slice::<models::WSFuturesCombinedAggregateTrade>(
                                &s,
                            ) {
                                Ok(d) => {
                                    let _ = self.tx.broadcast(Box::new(d.data));
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                }
            }
            Frame::Binary(b) => {
                if let Some(data) = b {
                    trace!("WebSocket Receive(Binary): {:?}", data);
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
