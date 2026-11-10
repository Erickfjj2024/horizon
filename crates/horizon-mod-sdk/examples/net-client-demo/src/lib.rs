//! Net Client Demo (client half): answers the bytes the server's half sends
//! on `netclientdemo:hello` with their length on `netclientdemo:ack`.
use horizon_mod_sdk::{Mod, Registration, net_client};

struct Demo;

impl Mod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Demo, Registration::new()))
    }
    fn net_message(&mut self, channel: &str, data: &[u8]) {
        if channel == "netclientdemo:hello" {
            let _ = net_client::send_to_server("netclientdemo:ack", &[data.len() as u8]);
        }
    }
}

horizon_mod_sdk::export_mod!(Demo);
