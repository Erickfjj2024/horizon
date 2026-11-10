//! Net Echo (server half): the smallest user of mod messages. A player's
//! client half sends bytes on `netecho:ping`; the server answers with the
//! bytes reversed on `netecho:pong`, and refuses to send on a channel that is
//! not its own (the answer to that is logged in the pong's text).
use horizon_mod_sdk::server::{Player, Registration, ServerMod, net};

struct Echo;

impl ServerMod for Echo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Echo, Registration::new().on_net()))
    }
    fn net_message(&mut self, player: &Player, channel: &str, data: &[u8]) {
        if channel != "netecho:ping" {
            return;
        }
        let mut back = data.to_vec();
        back.reverse();
        let _ = net::send(&player.id, "netecho:pong", &back);
        // Another mod's channel is refused.
        if let Err(e) = net::send(&player.id, "other:pong", &back) {
            let _ = net::send(&player.id, "netecho:refused", e.as_bytes());
        }
    }
}

horizon_mod_sdk::export_server_mod!(Echo);
