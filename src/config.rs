use std::fs::File;
use std::io::Read;

use serde::Deserialize;
use toml;

use curtis_binance::models::BinanceConfig;
use curtis_gate::models::GateConfig;
use curtis_huobi::models::HuobiConfig;
use curtis_okex::models::OKExConfig;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Config {
    pub binance: Option<BinanceConfig>,
    pub gate: Option<GateConfig>,
    pub huobi: Option<HuobiConfig>,
    pub okex: Option<OKExConfig>,
}

pub fn read_config(config_file_path: String) -> Config {
    let mut file = File::open(config_file_path).expect("Config file not found");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Failure while reading config file");
    toml::from_str(&content).unwrap()
}
