// SPDX-License-Identifier: MIT OR Apache-2.0

//! `rustyac-gateway`: one WebSocket per player to one Assetto Corsa server.
//!
//! ```text
//! rustyac-gateway --name spa --listen 0.0.0.0:9601 --server 127.0.0.1:9600 \
//!     --origins https://rustyac.example.com
//! rustyac-gateway --check 127.0.0.1:9601        (exit code 0: the gateway answers)
//! ```
//!
//! Every option can also come from the environment (`GATEWAY_NAME`, `GATEWAY_LISTEN`,
//! `GATEWAY_SERVER`, `GATEWAY_UDP_PORT`, `GATEWAY_ORIGINS`, `GATEWAY_MAX_CLIENTS`,
//! `GATEWAY_MAX_MESSAGE`, `GATEWAY_IDLE_TIMEOUT`, `GATEWAY_PING_INTERVAL`); the command
//! line wins.

use rustyac_gateway::gateway::{self, Config, Gateway};

const HELP: &str = "rustyac-gateway: carries Assetto Corsa clients over WebSockets to an acServer

  --name <text>           a short name for the log lines            [gateway]
  --listen <addr:port>    where WebSockets are accepted             [127.0.0.1:9601]
  --server <host:port>    the game server's TCP port                [127.0.0.1:9600]
  --udp-port <n>          the game server's UDP port                [the TCP port]
  --origins <list>        browser origins that may connect, comma-separated:
                          https://site.example, http://127.0.0.1:* (any port), * (all)
                                                [http://127.0.0.1:*,http://localhost:*]
                          (a client that is no browser sends no Origin and is let in)
  --max-clients <n>       clients at once                           [24]
  --max-message <bytes>   the largest WebSocket message taken       [4096]
  --idle-timeout <s>      drop a client that is silent this long    [300]
  --ping-interval <s>     ping every client this often              [20]
  --quiet                 no log lines
  --check [addr:port]     ask a running gateway's /healthz and exit (0: it answers)

GET /healthz answers \"ok\". The log never holds what is carried: no player id, no
password, no name, no chat.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let mut config = Config::default();
    let mut check: Option<Option<String>> = None;
    let number = |name: &str, text: &str| -> u64 {
        text.parse().unwrap_or_else(|_| {
            eprintln!("{name}: {text:?} is no number");
            std::process::exit(2);
        })
    };
    let set = |config: &mut Config, name: &str, value: String| match name {
        "name" => config.name = value,
        "listen" => config.listen = value,
        "server" => config.server = value,
        "udp-port" => config.udp_port = Some(number(name, &value) as u16),
        "origins" => config.origins = value.split(',').map(|origin| origin.trim().to_string()).filter(|origin| !origin.is_empty()).collect(),
        "max-clients" => config.max_clients = number(name, &value) as usize,
        "max-message" => config.max_message = (number(name, &value) as usize).max(64),
        "idle-timeout" => config.idle_timeout_s = number(name, &value),
        "ping-interval" => config.ping_interval_s = number(name, &value),
        _ => unreachable!(),
    };
    const OPTIONS: [&str; 9] = ["name", "listen", "server", "udp-port", "origins", "max-clients", "max-message", "idle-timeout", "ping-interval"];
    for name in OPTIONS {
        if let Some(value) = env(&format!("GATEWAY_{}", name.replace('-', "_").to_uppercase())) {
            set(&mut config, name, value);
        }
    }
    let mut at = 0;
    while at < args.len() {
        let arg = args[at].as_str();
        at += 1;
        match arg {
            "--help" | "-h" => {
                println!("{HELP}");
                return;
            }
            "--quiet" => config.quiet = true,
            "--check" => {
                let address = args.get(at).filter(|next| !next.starts_with("--")).cloned();
                at += address.is_some() as usize;
                check = Some(address);
            }
            option if option.starts_with("--") && OPTIONS.contains(&&option[2..]) => {
                let Some(value) = args.get(at) else {
                    eprintln!("{option} needs a value");
                    std::process::exit(2);
                };
                at += 1;
                set(&mut config, &option[2..], value.clone());
            }
            other => {
                eprintln!("unknown option {other} (--help lists them)");
                std::process::exit(2);
            }
        }
    }
    if let Some(address) = check {
        let address = address.unwrap_or_else(|| config.listen.clone());
        match gateway::check(&address) {
            Ok(()) => return,
            Err(why) => {
                eprintln!("{why}");
                std::process::exit(1);
            }
        }
    }
    match Gateway::start(config) {
        Ok(gateway) => gateway.wait(),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
