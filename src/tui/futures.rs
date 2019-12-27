use std::cmp::min;
use std::io;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, SecondsFormat, Utc};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::{Backend, TermionBackend};
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::widgets::{Block, Borders, Paragraph, Row, Table, Text, Widget};
use tui::{Frame, Terminal};

use super::event::{Event, Events};
use super::preclude::*;
use curtis_core::models::{FuturesTable, Order, OrderBook, Trade, TradeDirection};

const TABLE_WIDTH: u16 = 18;

pub(crate) fn render(data: Arc<RwLock<FuturesTable>>) -> Result<(), failure::Error> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let events = Events::new();

    let mut freeze = false;
    // Input
    loop {
        if !freeze {
            // Release read lock ASAP
            {
                let d = data.read().unwrap();
                terminal.draw(|mut f| {
                    let rects = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [
                                Constraint::Length(5),
                                Constraint::Min(12),
                                Constraint::Length(14),
                                Constraint::Length(3),
                            ]
                            .as_ref(),
                        )
                        .margin(1)
                        .split(f.size());

                    draw_time_info(&mut f, rects[0], &d);

                    {
                        let chunks = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints(
                                [
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                    Constraint::Min(TABLE_WIDTH + 2),
                                ]
                                .as_ref(),
                            )
                            .split(rects[1]);

                        draw_table(
                            &mut f,
                            chunks[0],
                            *STYLE_ASK,
                            String::from("Binance ASK"),
                            &d.binance.asks,
                        );
                        draw_table(
                            &mut f,
                            chunks[1],
                            *STYLE_BID,
                            String::from("Binance BID"),
                            &d.binance.bids,
                        );
                        draw_table(
                            &mut f,
                            chunks[2],
                            *STYLE_ASK,
                            String::from("Gate ASK"),
                            &d.gate.asks,
                        );
                        draw_table(
                            &mut f,
                            chunks[3],
                            *STYLE_BID,
                            String::from("Gate BID"),
                            &d.gate.bids,
                        );
                        draw_table(
                            &mut f,
                            chunks[4],
                            *STYLE_ASK,
                            String::from("Huobi ASK"),
                            &d.huobi.asks,
                        );
                        draw_table(
                            &mut f,
                            chunks[5],
                            *STYLE_BID,
                            String::from("Huobi BID"),
                            &d.huobi.bids,
                        );
                        draw_table(
                            &mut f,
                            chunks[6],
                            *STYLE_ASK,
                            String::from("OKEx ASK"),
                            &d.okex.asks,
                        );
                        draw_table(
                            &mut f,
                            chunks[7],
                            *STYLE_BID,
                            String::from("OKEx BID"),
                            &d.okex.bids,
                        );
                    }

                    {
                        let chunks = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints(
                                [
                                    Constraint::Percentage(25),
                                    Constraint::Percentage(25),
                                    Constraint::Percentage(25),
                                    Constraint::Percentage(25),
                                ]
                                .as_ref(),
                            )
                            .split(rects[2]);

                        draw_trade(&mut f, chunks[0], &d.binance);
                        draw_trade(&mut f, chunks[1], &d.gate);
                        draw_trade(&mut f, chunks[2], &d.huobi);
                        draw_trade(&mut f, chunks[3], &d.okex);
                    }

                    let text = [Text::raw("q - quit | f - toggle data update")];
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title_style(Style::default().modifier(Modifier::BOLD));
                    Paragraph::new(text.iter())
                        .block(block.clone().title(" Help "))
                        .alignment(Alignment::Left)
                        .render(&mut f, rects[3]);
                })?;
            }
        }

        // Block `tick_rate` ms and control frame render rate
        if let Ok(Event::Input(key)) = events.next() {
            match key {
                Key::Char('q') => {
                    break;
                }
                Key::Char('f') => {
                    freeze = !freeze;
                }
                _ => (),
            }
        };
    }

    Ok(())
}

fn format_latency<'b>(now: DateTime<Utc>, t: DateTime<Utc>) -> Text<'b> {
    const LATENCY_WARN_MILLIS: i64 = 300;
    const LATENCY_ERROR_MILLIS: i64 = 1000;

    let mut style = *STYLE_OK;
    let latency = (now - t).num_milliseconds();
    if latency > LATENCY_ERROR_MILLIS {
        style = *STYLE_ERROR;
    } else if latency > LATENCY_WARN_MILLIS {
        style = *STYLE_WARN;
    }
    Text::styled(format!("{:12}", format!("(-{}ms)", latency)), style)
}

fn draw_time_info<B>(f: &mut Frame<B>, area: Rect, table: &FuturesTable)
where
    B: Backend,
{
    let block = Block::default().title(" Curtis ").borders(Borders::ALL);
    let now = Utc::now();
    let text = [
        Text::raw(format!(
            "Local: {}  |  ",
            now.to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        Text::raw(format!(
            "Last Update: {} ",
            table.timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        format_latency(now, table.timestamp),
        Text::raw(format!(
            "\nBinance:   {} ",
            table
                .binance
                .timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        format_latency(now, table.binance.timestamp),
        Text::raw(format!(
            "Gate:     {} ",
            table
                .gate
                .timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        format_latency(now, table.gate.timestamp),
        Text::raw(format!(
            "\nHuobi:     {} ",
            table
                .huobi
                .timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        format_latency(now, table.huobi.timestamp),
        Text::raw(format!(
            "OKEx:     {} ",
            table
                .okex
                .timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        )),
        format_latency(now, table.okex.timestamp),
    ];
    Paragraph::new(text.iter())
        .block(block)
        .alignment(Alignment::Left)
        .render(f, area);
}

fn format_order_i(orders: &[Order], i: usize) -> String {
    if orders.len() > i {
        format!("{}({})", orders[i].price, orders[i].amount)
    } else {
        String::from("")
    }
}

fn draw_table<B>(f: &mut Frame<B>, area: Rect, style: Style, header: String, orders: &[Order])
where
    B: Backend,
{
    let header = vec![header];
    let mut rows = Vec::new();

    let lines = min(orders.len(), 10);
    for i in 0..lines {
        rows.push(vec![format_order_i(orders, i)]);
    }

    let rows = rows
        .iter()
        .enumerate()
        .map(|(_i, item)| Row::StyledData(item.iter(), style));

    Table::new(header.iter(), rows)
        .block(Block::default().borders(Borders::NONE))
        .widths(&[TABLE_WIDTH])
        .render(f, area);
}

fn draw_trade<B>(f: &mut Frame<B>, area: Rect, ob: &OrderBook)
where
    B: Backend,
{
    let mut trades = Vec::<Trade>::new();
    for t in ob.buys.iter() {
        trades.push(t.clone());
    }
    for t in ob.sells.iter() {
        trades.push(t.clone());
    }
    trades.sort_by_key(|t| t.timestamp);
    trades.reverse();

    let mut text = Vec::<Text>::new();
    for t in trades.iter() {
        let line = format!(
            "{} {} {}  {}\n",
            t.timestamp.format("%H:%M:%S%.3f"),
            t.direction,
            t.price,
            t.amount
        );
        match t.direction {
            TradeDirection::Buy => text.push(Text::styled(line, *STYLE_BUY)),
            TradeDirection::Sell => text.push(Text::styled(line, *STYLE_SELL)),
        }
    }

    Paragraph::new(text.iter())
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .title(ob.exchange.as_str()),
        )
        .alignment(Alignment::Left)
        .render(f, area);
}
