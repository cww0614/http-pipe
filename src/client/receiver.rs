use anyhow::bail;
use bytes::Bytes;
use http_pipe::common::headers;
use log::debug;
use reqwest::Client;
use std::time::Duration;
use tokio::{
    io::AsyncWriteExt,
    sync::mpsc::{self, Receiver, Sender},
};

const WORKER_NUM: u64 = 4;

struct Worker {
    tx: Sender<Bytes>,
    index: u64,
    worker_num: u64,
    url: String,
    client: Client,
}

impl Worker {
    fn new(url: &str, index: u64, worker_num: u64) -> (Receiver<Bytes>, Worker) {
        let (tx, rx) = mpsc::channel(1);

        (
            rx,
            Worker {
                tx,
                index,
                worker_num,
                url: url.into(),
                client: Client::new(),
            },
        )
    }

    async fn run(mut self) {
        let mut ack = None;
        'l: loop {
            loop {
                match self.receive(ack).await {
                    Ok(bytes) => {
                        if bytes.len() == 0 {
                            break 'l;
                        }

                        ack = Some(self.index);
                        self.index += self.worker_num;

                        if let Err(_) = self.tx.send(bytes).await {
                            panic!("receiver closed before sender");
                        }

                        break;
                    }

                    Err(e) => {
                        debug!("http error: {}", e);
                        tokio::time::delay_for(Duration::from_secs(3)).await;
                    }
                }
            }
        }
    }

    async fn receive(&self, ack: Option<u64>) -> anyhow::Result<Bytes> {
        let mut r = self.client.get(&self.url);

        if let Some(ack) = ack {
            r = r.header(headers::ACK, ack);
        }

        let resp = r.header(headers::INDEX, self.index).send().await?;

        let status = resp.status();
        if !status.is_success() {
            bail!("server returned failure status: {:?}", status);
        }

        Ok(resp.bytes().await?)
    }
}

pub async fn receive(url: &str) -> anyhow::Result<()> {
    let mut receivers: Vec<_> = (0..WORKER_NUM)
        .map(|i| {
            let (rx, worker) = Worker::new(url, i, WORKER_NUM);

            tokio::spawn(worker.run());
            rx
        })
        .collect();

    let mut stdout = tokio::io::stdout();

    'l: loop {
        for r in &mut receivers {
            if let Some(bytes) = r.recv().await {
                stdout.write_all(&bytes).await?;
            } else {
                break 'l;
            }
        }
    }

    loop {
        match Client::new()
            .get(url)
            .header(headers::RESET, 0)
            .send()
            .await
        {
            Ok(_) => break,
            Err(e) => {
                debug!("http error: {}", e);
                tokio::time::delay_for(Duration::from_secs(3)).await;
            }
        }
    }

    Ok(())
}
