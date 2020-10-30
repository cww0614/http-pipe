use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::str::FromStr;

use actix_web::{App, get, HttpRequest, HttpResponse, HttpServer, put, web};
use actix_web::error::{
    ErrorBadRequest, ErrorGone, ErrorInternalServerError, ErrorPreconditionFailed,
};
use anyhow::anyhow;
use bytes::BytesMut;
use clap::{Clap, crate_version};
use futures::stream::StreamExt;
use log::debug;
use tokio::sync::mpsc::{self, Sender};

use http_pipe::common::{headers, Packet};
use queue::Queue;

mod queue;

#[derive(Debug, thiserror::Error)]
enum ControllerError {
    #[error(transparent)]
    Actix(#[from] actix_web::error::Error),
    #[error("Missing required fields: {0}")]
    MissingRequiredFields(String),
    #[error("Failed to decode value: {0}")]
    Decode(#[from] actix_web::http::header::ToStrError),
    #[error("Failed to parse integer: {0}")]
    IntegerParse(#[from] std::num::ParseIntError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

type ControllerResult<T> = Result<T, ControllerError>;

impl From<ControllerError> for actix_web::error::Error {
    fn from(e: ControllerError) -> actix_web::error::Error {
        match e {
            ControllerError::Actix(inner) => inner,
            ControllerError::MissingRequiredFields(_)
            | ControllerError::Decode(_)
            | ControllerError::IntegerParse(_) => ErrorBadRequest(e),
            _ => ErrorInternalServerError(e),
        }
    }
}

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

fn parse_from_header<T>(req: &HttpRequest, name: &str) -> ControllerResult<T>
    where
        T: FromStr,
        ControllerError: From<T::Err>,
{
    Ok(req
        .headers()
        .get(name)
        .ok_or_else(|| ControllerError::MissingRequiredFields(name.into()))?
        .to_str()?
        .parse()?)
}

#[put("/{id}")]
async fn recv(
    data: web::Data<AppState>,
    path: web::Path<String>,
    req: HttpRequest,
    mut body: web::Payload,
) -> ControllerResult<HttpResponse> {
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

    let worker_index: usize = parse_from_header(&req, headers::WORKER)?;
    let data_index = parse_from_header(&req, headers::INDEX)?;

    let mut sender = if let Some(conn) = data.endpoints.lock().unwrap().get(&path) {
        conn.senders[worker_index].clone()
    } else {
        return Err(ErrorPreconditionFailed("sender not available").into());
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
        .await
        .map_err(|_| anyhow!("failed to send packet to the channel"))?;

    debug!("PUT {:?} ended", path);

    Ok(HttpResponse::Ok().finish())
}

#[get("/{id}")]
async fn send(
    data: web::Data<AppState>,
    path: web::Path<String>,
    req: HttpRequest,
) -> ControllerResult<HttpResponse> {
    let path = path.into_inner();

    if let Some(_) = req.headers().get(headers::RESET) {
        data.endpoints.lock().unwrap().remove(&path);
        debug!("FIN {:?}", path);
        return Ok(HttpResponse::Ok().finish());
    }

    debug!("GET {:?}", path);

    let queue = if let Some(conn) = data.endpoints.lock().unwrap().get(&path) {
        conn.queue.clone()
    } else {
        return Err(ErrorPreconditionFailed("queue not available").into());
    };

    if let Some(ack_num) = req.headers().get(headers::ACK) {
        let ack_num = ack_num.to_str()?.parse()?;
        queue.remove(ack_num);
    }

    let data_index = parse_from_header(&req, headers::INDEX)?;

    debug!("GET {:?} ended", path);

    let data = if let Some(pkt) = queue.get(data_index).await {
        pkt.data.clone()
    } else {
        return Err(ErrorGone("data not avaiable").into());
    };

    Ok(HttpResponse::Ok().body(data))
}

#[derive(Clap)]
#[clap(version = crate_version ! ())]
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
