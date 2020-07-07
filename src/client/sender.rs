use anyhow::bail;
use bytes::BytesMut;
use http_pipe::common::{headers, Packet};
use log::debug;
use reqwest::Client;
use std::time::Duration;
use tokio::{
    io::AsyncReadExt,
    sync::mpsc::{self, Receiver, Sender},
};

const WORKER_NUM: u64 = 4;
const PACKET_SIZE: usize = 1 * 1024 * 1024;
const BUFFER_SIZE: usize = 64 * 1024;

struct Worker {
    rx: Receiver<Packet>,
    url: String,
    index: u64,
    client: Client,
}

impl Worker {
    fn new(url: &str, index: u64) -> (Sender<Packet>, Self) {
        let (tx, rx) = mpsc::channel(1);

        (
            tx,
            Worker {
                rx,
                index,
                client: Client::new(),
                url: url.into(),
            },
        )
    }

    async fn run(mut self) {
        while let Some(packet) = self.rx.recv().await {
            loop {
                if let Err(e) = self.send(&packet).await {
                    debug!("http error: {}", e);
                    tokio::time::delay_for(Duration::from_secs(3)).await;
                    continue;
                }

                break;
            }
        }
    }

    async fn send(&mut self, packet: &Packet) -> anyhow::Result<()> {
        let resp = self
            .client
            .put(&self.url)
            .header(headers::INDEX, packet.index)
            .header(headers::WORKER, self.index)
            .body(packet.data.clone())
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            // prevent poisoned connection from being reused
            self.client = Client::new();
            bail!("server returned failure status: {:?}", status);
        }

        Ok(())
    }
}

pub async fn send(url: &str) -> anyhow::Result<()> {
    let mut senders = Vec::new();
    let mut futures = Vec::new();

    for i in 0..WORKER_NUM {
        let (tx, worker) = Worker::new(url, i);

        futures.push(tokio::spawn(worker.run()));
        senders.push(tx);
    }

    Client::new()
        .put(url)
        .header(headers::RESET, WORKER_NUM)
        .send()
        .await?;

    let mut stdin = tokio::io::stdin();
    let mut buffer = vec![0; BUFFER_SIZE];
    let mut index = 0;
    let mut is_eof = false;

    'l: loop {
        for s in &mut senders {
            let mut bytes = BytesMut::new();

            if is_eof {
                s.send(Packet {
                    index,
                    data: bytes.freeze(),
                })
                .await?;
                break 'l;
            }

            while bytes.len() < PACKET_SIZE {
                let n = stdin.read(&mut buffer).await?;
                if n == 0 {
                    is_eof = true;
                    break;
                }

                bytes.extend_from_slice(&buffer[..n]);
            }

            s.send(Packet {
                index,
                data: bytes.freeze(),
            })
            .await?;
            index += 1;
        }
    }

    drop(senders);

    for f in futures {
        f.await?;
    }

    Ok(())
}
