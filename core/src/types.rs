use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum Exchange {
    Binance,
    Gate,
    Huobi,
    OKEx,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            &Exchange::Binance => write!(f, "Binance"),
            &Exchange::Gate => write!(f, "Gate.io"),
            &Exchange::Huobi => write!(f, "Huobi"),
            &Exchange::OKEx => write!(f, "OKEx"),
        }
    }
}
