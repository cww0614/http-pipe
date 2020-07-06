mod queue;

use actix_web::{get, put, web, App, HttpRequest, HttpResponse, HttpServer};
use anyhow::anyhow;
use bytes::BytesMut;
use clap::{crate_version, Clap};
use futures::stream::StreamExt;
use http_pipe::common::{headers, Packet};
use log::{debug, warn};
use queue::Queue;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::{self, Sender};

struct AppState {
    endpoints: Mutex<HashMap<String, Conn>>,
}

struct Conn {
    senders: Vec<Sender<Packet>>,
    queue: Arc<Queue>,
}

impl Conn {
    fn new(worker_num: usize) -> Self {
        let mut senders: Vec<Sender<Packet>> = Vec::new();
        let mut receivers = Vec::new();

        for _ in 0..worker_num {
            let (tx, rx) = mpsc::channel(1);
            senders.push(tx);
            receivers.push(rx);
        }

        let queue = Arc::new(Queue::new(16));

        let q = queue.clone();
        tokio::spawn(async move {
            let mut index = 0;
            'l: loop {
                for rx in &mut receivers {
                    loop {
                        if let Some(packet) = rx.recv().await {
                            if packet.index < index {
                                continue;
                            }

                            debug_assert!(packet.index == index);

                            let is_eof = packet.data.is_empty();
                            q.push(packet).await;
                            index += 1;

                            if is_eof {
                                break 'l;
                            }

                            break;
                        } else {
                            break 'l;
                        }
                    }
                }
            }
        });

        Conn { senders, queue }
    }
}

#[put("/{id}")]
async fn recv(
    data: web::Data<AppState>,
    path: web::Path<String>,
    req: HttpRequest,
    mut body: web::Payload,
) -> HttpResponse {
    match (move || async move {
        let path = path.into_inner();

        if let Some(worker_num) = req.headers().get(headers::RESET) {
            debug!("RESET {:?}", path);

            data.endpoints
                .lock()
                .unwrap()
                .insert(path, Conn::new(worker_num.to_str()?.parse()?));

            return Ok(HttpResponse::Ok().finish());
        }

        debug!("PUT {:?}", path);

        let worker_index: usize = if let Some(worker_index) = req.headers().get(headers::WORKER) {
            worker_index.to_str()?.parse()?
        } else {
            return Ok(HttpResponse::BadRequest().finish());
        };

        let data_index = if let Some(data_index) = req.headers().get(headers::INDEX) {
            data_index.to_str()?.parse()?
        } else {
            return Ok(HttpResponse::BadRequest().finish());
        };

        let mut sender = if let Some(conn) = data.endpoints.lock().unwrap().get(&path) {
            conn.senders[worker_index].clone()
        } else {
            return Ok(HttpResponse::PreconditionFailed().finish());
        };

        let mut bytes = BytesMut::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&(chunk.map_err(|e| anyhow!("payload error: {}", e))?));
        }

        sender
            .send(Packet {
                index: data_index,
                data: bytes.freeze(),
            })
            .await?;

        debug!("PUT {:?} ended", path);

        Ok::<_, anyhow::Error>(HttpResponse::Ok().finish())
    })()
    .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("Error: {}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[get("/{id}")]
async fn send(
    data: web::Data<AppState>,
    path: web::Path<String>,
    req: HttpRequest,
) -> HttpResponse {
    match (move || async move {
        let path = path.into_inner();

        debug!("GET {:?}", path);

        let queue = if let Some(conn) = data.endpoints.lock().unwrap().get(&path) {
            conn.queue.clone()
        } else {
            return Ok(HttpResponse::PreconditionFailed().finish());
        };

        if let Some(ack_num) = req.headers().get(headers::ACK) {
            let ack_num = ack_num.to_str()?.parse()?;
            queue.remove(ack_num);
        }

        let data_index = if let Some(data_index) = req.headers().get(headers::INDEX) {
            data_index.to_str()?.parse()?
        } else {
            return Ok(HttpResponse::BadRequest().finish());
        };

        debug!("GET {:?} ended", path);

        let data = if let Some(pkt) = queue.get(data_index).await {
            pkt.data.clone()
        } else {
            return Ok(HttpResponse::Gone().finish());
        };

        Ok::<_, anyhow::Error>(HttpResponse::Ok().body(data))
    })()
    .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("Error: {}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[derive(Clap)]
#[clap(version=crate_version!())]
struct Opts {
    #[clap(long = "debug")]
    debug: bool,
    addr: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();

    http_pipe::common::init_log(opts.debug);

    let local = tokio::task::LocalSet::new();
    let sys = actix_rt::System::run_in_tokio("server", &local);

    let app_state = web::Data::new(AppState {
        endpoints: Mutex::new(HashMap::new()),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .service(recv)
            .service(send)
    })
    .bind(&opts.addr)?
    .run()
    .await?;

    sys.await?;

    Ok(())
}
