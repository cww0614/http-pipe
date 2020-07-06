mod receiver;
mod sender;

use anyhow::bail;
use atty::Stream;
use clap::{crate_version, Clap};

#[derive(Clap)]
#[clap(version=crate_version!())]
struct Opts {
    #[clap(long = "debug")]
    debug: bool,
    endpoint: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();

    http_pipe::common::init_log(opts.debug);

    match (atty::is(Stream::Stdin), atty::is(Stream::Stdout)) {
        (false, true) => sender::send(&opts.endpoint).await?,
        (true, _) => receiver::receive(&opts.endpoint).await?,
        _ => bail!("Invalid usage, please use this with a single pipe"),
    }

    Ok(())
}
