mod device;
mod input;
mod ring;
mod serve;
mod video;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::Read;
use std::net::SocketAddr;

/// iOS simulators for AI agents, on Linux.
#[derive(Parser)]
#[command(name = "agent-sim", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stream a running phone to a browser and take input back.
    Serve {
        /// The x86Sim device: its name (`a` for device-a64-a) or its directory. Empty for x86Sim's default.
        #[arg(long, default_value = "")]
        device: String,
        /// Address to listen on.
        #[arg(long, default_value = "127.0.0.1:7979")]
        listen: SocketAddr,
        /// Access token in the URL path; random when not given.
        #[arg(long, env = "AGENT_SIM_TOKEN")]
        token: Option<String>,
        /// Stream at 1/N of the device's pixel size in each direction.
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=4))]
        scale: u8,
        /// Video bitrate in kbit/s.
        #[arg(long, default_value_t = 4000)]
        bitrate: u32,
    },
}

fn random_token() -> Result<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .context("reading /dev/urandom")?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            device,
            listen,
            token,
            scale,
            bitrate,
        } => {
            let token = match token {
                Some(t) if t.len() >= 16 && t.chars().all(|c| c.is_ascii_alphanumeric()) => t,
                Some(_) => anyhow::bail!("--token must be at least 16 letters or digits"),
                None => random_token()?,
            };
            let opts = serve::Options {
                device: device::Device::find(&device)?,
                listen,
                token,
                scale: scale as usize,
                bitrate_bps: bitrate.saturating_mul(1000),
            };
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(serve::run(opts))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }
}
