use anyhow::bail;
use atty::Stream;

mod receiver;
mod sender;

pub async fn main(endpoint: String) -> anyhow::Result<()> {
    match (atty::is(Stream::Stdin), atty::is(Stream::Stdout)) {
        (false, true) => sender::send(&endpoint).await?,
        (true, _) => receiver::receive(&endpoint).await?,
        _ => bail!("Invalid usage, please use this with a single pipe"),
    }

    Ok(())
}
