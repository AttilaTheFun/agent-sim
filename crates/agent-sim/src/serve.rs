//! `agent-sim serve`: the phone's screen as low-latency H.264 over a WebSocket, input back over the same socket,
//! and a small viewer page. Everything sits under a random token path.

use crate::device::Device;
use crate::input::Hid;
use crate::ring::Ring;
use crate::video::{VideoEncoder, bgra_to_i420};
use anyhow::{Context, Result};
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

const VIEWER: &str = include_str!("viewer.html");
/// With `--scale 0`, the stream is the largest integer fraction of the screen at most this wide: a 2x phone streams
/// at full size, a 3x one at half.
const MAX_AUTO_WIDTH: usize = 900;

pub struct Options {
    pub device: Device,
    pub listen: SocketAddr,
    pub token: String,
    pub scale: usize,
    pub bitrate_bps: u32,
}

/// One encoded frame as sent to viewers: a 25-byte header (flags, sequence, publish time, send time; little endian)
/// followed by the Annex B access unit.
struct Packet {
    keyframe: bool,
    bytes: axum::body::Bytes,
}

struct Shared {
    token: String,
    frames: broadcast::Sender<Arc<Packet>>,
    viewers: AtomicUsize,
    want_keyframe: AtomicBool,
    input: std::sync::mpsc::Sender<Input>,
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Input {
    Touch { p: String, x: f64, y: f64 },
    Button { b: String, d: bool },
    Text { s: String },
    Keyframe,
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Waits for new frames and encodes them while anyone is watching.
fn encode_loop(shared: Arc<Shared>, ring_path: std::path::PathBuf, scale: usize, bitrate_bps: u32) {
    let mut ring = Ring::new(&ring_path);
    let mut encoder = match VideoEncoder::new(bitrate_bps) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("agent-sim: {e:#}");
            std::process::exit(1);
        }
    };
    let mut last = 0u64;
    loop {
        if shared.viewers.load(Ordering::Acquire) == 0 {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let seq = ring.wait(last, Duration::from_millis(50));
        let key = shared.want_keyframe.swap(false, Ordering::AcqRel);
        if seq == last && !key {
            continue;
        }
        let Some(frame) = ring.latest() else { continue };
        last = frame.seq;
        if key {
            encoder.force_keyframe();
        }
        let scale = if scale == 0 {
            frame.width.div_ceil(MAX_AUTO_WIDTH).max(1)
        } else {
            scale
        };
        let yuv = bgra_to_i420(&frame.bgra, frame.width, frame.height, scale);
        let out = match encoder.encode(&yuv) {
            Ok(o) if !o.annexb.is_empty() => o,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("agent-sim: {e:#}");
                continue;
            }
        };
        let mut bytes = Vec::with_capacity(25 + out.annexb.len());
        bytes.push(out.keyframe as u8);
        bytes.extend_from_slice(&frame.seq.to_le_bytes());
        bytes.extend_from_slice(&frame.published_ns.to_le_bytes());
        bytes.extend_from_slice(&now_ns().to_le_bytes());
        bytes.extend_from_slice(&out.annexb);
        let _ = shared.frames.send(Arc::new(Packet {
            keyframe: out.keyframe,
            bytes: bytes.into(),
        }));
    }
}

/// Applies input in order on its own thread (a tap's release may wait for its minimum hold).
fn input_loop(rx: std::sync::mpsc::Receiver<Input>, mut hid: Hid) {
    for msg in rx {
        let r = match msg {
            Input::Touch { p, x, y } => hid.touch(&p, x, y),
            Input::Button { b, d } => hid.button(&b, d),
            Input::Text { s } => hid.text(&s),
            Input::Keyframe => Ok(()),
        };
        if let Err(e) = r {
            eprintln!("agent-sim: input: {e:#}");
        }
    }
}

async fn page(State(s): State<Arc<Shared>>, Path(token): Path<String>) -> Response {
    if token != s.token {
        return StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        VIEWER,
    )
        .into_response()
}

async fn socket(
    State(s): State<Arc<Shared>>,
    Path(token): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    if token != s.token {
        return StatusCode::NOT_FOUND.into_response();
    }
    ws.on_upgrade(move |sock| viewer(sock, s))
}

async fn viewer(mut sock: WebSocket, s: Arc<Shared>) {
    let mut frames = s.frames.subscribe();
    s.viewers.fetch_add(1, Ordering::AcqRel);
    s.want_keyframe.store(true, Ordering::Release);
    let mut need_key = true;
    loop {
        tokio::select! {
            f = frames.recv() => match f {
                Ok(p) => {
                    if need_key && !p.keyframe {
                        continue;
                    }
                    need_key = false;
                    if sock.send(Message::Binary(p.bytes.clone())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    need_key = true;
                    s.want_keyframe.store(true, Ordering::Release);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            m = sock.recv() => match m {
                Some(Ok(Message::Text(t))) => match serde_json::from_str::<Input>(&t) {
                    Ok(Input::Keyframe) => {
                        need_key = true;
                        s.want_keyframe.store(true, Ordering::Release);
                    }
                    Ok(i) => { let _ = s.input.send(i); }
                    Err(_) => {}
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    s.viewers.fetch_sub(1, Ordering::AcqRel);
}

pub async fn run(opts: Options) -> Result<()> {
    let ring = opts.device.ring_path()?;
    let (frames, _) = broadcast::channel(8);
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared {
        token: opts.token.clone(),
        frames,
        viewers: AtomicUsize::new(0),
        want_keyframe: AtomicBool::new(false),
        input: input_tx,
    });
    {
        let s = shared.clone();
        let (scale, bitrate) = (opts.scale, opts.bitrate_bps);
        std::thread::Builder::new()
            .name("encode".into())
            .spawn(move || encode_loop(s, ring, scale, bitrate))?;
    }
    let hid = Hid::new(opts.device.hid_socket());
    std::thread::Builder::new()
        .name("input".into())
        .spawn(move || input_loop(input_rx, hid))?;

    let app = Router::new()
        .route("/{token}/", get(page))
        .route("/{token}/ws", get(socket))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind(opts.listen)
        .await
        .with_context(|| format!("listening on {}", opts.listen))?;
    println!(
        "agent-sim: serving {} at http://{}/{}/",
        opts.device.dir.display(),
        opts.listen,
        opts.token
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
