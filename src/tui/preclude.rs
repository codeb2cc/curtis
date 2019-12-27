use lazy_static::lazy_static;
use tui::style::{Color, Style};

lazy_static! {
    pub static ref STYLE_OK: Style = { Style::default().fg(Color::Green) };
    pub static ref STYLE_WARN: Style = { Style::default().fg(Color::Yellow) };
    pub static ref STYLE_ERROR: Style = { Style::default().fg(Color::Red) };
    pub static ref STYLE_BUY: Style = { Style::default().fg(Color::Red) };
    pub static ref STYLE_SELL: Style = { Style::default().fg(Color::Green) };
    pub static ref STYLE_ASK: Style = { Style::default().fg(Color::LightRed) };
    pub static ref STYLE_BID: Style = { Style::default().fg(Color::LightGreen) };
}
