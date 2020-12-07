use clap::{crate_version, Clap};

mod client;
mod common;
mod server;

#[derive(Clap)]
#[clap(version = crate_version ! ())]
struct Opts {
    #[clap(long = "debug")]
    debug: bool,
    #[clap(long = "server")]
    server: bool,
    endpoint: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();

    common::init_log(opts.debug);

    if opts.server {
        server::main(opts.endpoint).await
    } else {
        client::main(opts.endpoint).await
    }
}
