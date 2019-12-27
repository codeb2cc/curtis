use std::time::Duration;

use awc::Client;
use openssl::ssl::{SslConnector, SslMethod};

// actix http client doesn't handle HTTP/2 websocket downgrade well
pub fn http1_client() -> Client {
    let ssl = {
        let mut ssl = SslConnector::builder(SslMethod::tls()).unwrap();
        let _ = ssl.set_alpn_protos(b"\x08http/1.1");
        ssl.build()
    };
    let connector = awc::Connector::new()
        .ssl(ssl)
        .timeout(Duration::from_secs(3))
        .finish();

    awc::ClientBuilder::new().connector(connector).finish()
}
