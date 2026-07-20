use std::{env, net::SocketAddr, process::ExitCode, time::Duration};

use network_emulator::{Config, DirectionConfig, run};

fn main() -> ExitCode {
    match parse_args(env::args().skip(1).collect()) {
        Ok(config) => match run(config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("network_emulator: {}", err);
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{}", message);
            }
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<Config, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Err(String::new());
    }

    let mut listen_addr: SocketAddr = "127.0.0.1:42069"
        .parse::<SocketAddr>()
        .map_err(|err| err.to_string())?;
    let mut server_addr: SocketAddr = "127.0.0.1:42070"
        .parse::<SocketAddr>()
        .map_err(|err| err.to_string())?;
    let mut server_bind_addr = None;
    let mut buf_size = 65_535usize;
    let mut seed = 1u64;

    let mut uplink = DirectionConfig {
        loss: 0.1,
        min_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(200),
        ..DirectionConfig::default()
    };
    let mut downlink = uplink.clone();

    let mut index = 0usize;
    while index < args.len() {
        let key = &args[index];
        let next = |index: &mut usize| -> Result<&str, String> {
            *index += 1;
            args.get(*index)
                .map(String::as_str)
                .ok_or_else(|| format!("missing value for {}", key))
        };

        match key.as_str() {
            "--listen" | "--client" => {
                listen_addr = next(&mut index)?
                    .parse()
                    .map_err(|err| format!("invalid listen addr: {}", err))?;
            }
            "--server" => {
                server_addr = next(&mut index)?
                    .parse()
                    .map_err(|err| format!("invalid server addr: {}", err))?;
            }
            "--server-bind" => {
                server_bind_addr = Some(
                    next(&mut index)?
                        .parse()
                        .map_err(|err| format!("invalid server bind addr: {}", err))?,
                );
            }
            "--buf" => {
                buf_size = next(&mut index)?
                    .parse()
                    .map_err(|err| format!("invalid buf size: {}", err))?;
            }
            "--seed" => {
                seed = next(&mut index)?
                    .parse()
                    .map_err(|err| format!("invalid seed: {}", err))?;
            }
            "--loss" => {
                let value = parse_probability(next(&mut index)?)?;
                uplink.loss = value;
                downlink.loss = value;
            }
            "--duplicate" => {
                let value = parse_probability(next(&mut index)?)?;
                uplink.duplicate = value;
                downlink.duplicate = value;
            }
            "--corrupt" => {
                let value = parse_probability(next(&mut index)?)?;
                uplink.corrupt = value;
                downlink.corrupt = value;
            }
            "--reorder" => {
                let value = parse_probability(next(&mut index)?)?;
                uplink.reorder = value;
                downlink.reorder = value;
            }
            "--mindelay" => {
                let value = parse_millis(next(&mut index)?)?;
                uplink.min_delay = value;
                downlink.min_delay = value;
            }
            "--maxdelay" => {
                let value = parse_millis(next(&mut index)?)?;
                uplink.max_delay = value;
                downlink.max_delay = value;
            }
            "--reorder-window" => {
                let value = parse_millis(next(&mut index)?)?;
                uplink.reorder_window = value;
                downlink.reorder_window = value;
            }
            "--up-loss" => uplink.loss = parse_probability(next(&mut index)?)?,
            "--up-duplicate" => uplink.duplicate = parse_probability(next(&mut index)?)?,
            "--up-corrupt" => uplink.corrupt = parse_probability(next(&mut index)?)?,
            "--up-reorder" => uplink.reorder = parse_probability(next(&mut index)?)?,
            "--up-mindelay" => uplink.min_delay = parse_millis(next(&mut index)?)?,
            "--up-maxdelay" => uplink.max_delay = parse_millis(next(&mut index)?)?,
            "--up-reorder-window" => uplink.reorder_window = parse_millis(next(&mut index)?)?,
            "--down-loss" => downlink.loss = parse_probability(next(&mut index)?)?,
            "--down-duplicate" => downlink.duplicate = parse_probability(next(&mut index)?)?,
            "--down-corrupt" => downlink.corrupt = parse_probability(next(&mut index)?)?,
            "--down-reorder" => downlink.reorder = parse_probability(next(&mut index)?)?,
            "--down-mindelay" => downlink.min_delay = parse_millis(next(&mut index)?)?,
            "--down-maxdelay" => downlink.max_delay = parse_millis(next(&mut index)?)?,
            "--down-reorder-window" => downlink.reorder_window = parse_millis(next(&mut index)?)?,
            other => {
                return Err(format!(
                    "unknown argument: {}\n\nUse --help for usage.",
                    other
                ));
            }
        }

        index += 1;
    }

    validate_direction("uplink", &uplink)?;
    validate_direction("downlink", &downlink)?;

    Ok(Config {
        listen_addr,
        server_addr,
        server_bind_addr,
        buf_size,
        seed,
        uplink,
        downlink,
    })
}

fn validate_direction(label: &str, config: &DirectionConfig) -> Result<(), String> {
    if config.max_delay < config.min_delay {
        return Err(format!("{} maxdelay must be >= mindelay", label));
    }
    Ok(())
}

fn parse_probability(text: &str) -> Result<f64, String> {
    let value: f64 = text
        .parse()
        .map_err(|err| format!("invalid probability {}: {}", text, err))?;
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("probability must be between 0.0 and 1.0: {}", text));
    }
    Ok(value)
}

fn parse_millis(text: &str) -> Result<Duration, String> {
    let millis: u64 = text
        .parse()
        .map_err(|err| format!("invalid millisecond value {}: {}", text, err))?;
    Ok(Duration::from_millis(millis))
}

fn print_help() {
    println!(
        "\
network_emulator

Usage:
  cargo run -p network_emulator -- --listen 127.0.0.1:42069 --server 127.0.0.1:42070

Common flags:
  --listen ADDR              address clients connect to (default 127.0.0.1:42069)
  --server ADDR              real server address (default 127.0.0.1:42070)
  --server-bind ADDR         local upstream bind address (default wildcard, ephemeral port)
  --buf BYTES                UDP receive buffer size (default 65535)
  --seed N                   deterministic seed (default 1)

Shared impairment flags:
  --loss P                   packet loss probability
  --mindelay MS              minimum one-way delay
  --maxdelay MS              maximum one-way delay
  --duplicate P              packet duplication probability
  --reorder P                probability of adding reorder delay
  --corrupt P                probability of flipping one random bit
  --reorder-window MS        extra delay added to reordered packets

Directional overrides:
  --up-loss P                uplink-only override
  --up-mindelay MS           uplink-only override
  --up-maxdelay MS           uplink-only override
  --up-duplicate P           uplink-only override
  --up-reorder P             uplink-only override
  --up-corrupt P             uplink-only override
  --up-reorder-window MS     uplink-only override
  --down-loss P              downlink-only override
  --down-mindelay MS         downlink-only override
  --down-maxdelay MS         downlink-only override
  --down-duplicate P         downlink-only override
  --down-reorder P           downlink-only override
  --down-corrupt P           downlink-only override
  --down-reorder-window MS   downlink-only override
"
    );
}
