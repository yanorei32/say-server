use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::{
    Router, extract,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
};
use clap::Parser;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot},
};

mod ffi;
mod worker;

use ffi::*;

static VOICES: OnceCell<Vec<Voice>> = OnceCell::new();
pub static TEMPORARY_DIR: OnceCell<PathBuf> = OnceCell::new();
static WORKER_POOL: OnceCell<Mutex<HashMap<String, mpsc::Sender<RequestContext>>>> =
    OnceCell::new();

#[derive(Debug, Clone, Deserialize)]
struct Request {
    voice_id: String,
    text: String,
}

#[derive(Debug)]
pub struct RequestContext {
    text: String,
    writeback: oneshot::Sender<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
struct Voice {
    name: String,
    locale_identifier: String,
    demo_text: String,
    id: String,
}

#[derive(Debug, Parser)]
struct Cli {
    #[clap(env, long, short, default_value = "0.0.0.0:3000")]
    listen: SocketAddr,

    #[clap(env, long, short, default_value = "/tmp/")]
    temp_dir: PathBuf,
}

async fn get_worker(voice_id: &str) -> mpsc::Sender<RequestContext> {
    let mut pool_lock = WORKER_POOL.get().unwrap().lock().await;

    if let Some(sender) = pool_lock.get(voice_id) {
        return sender.clone();
    }

    let (tx, rx) = mpsc::channel(16);

    std::thread::Builder::new()
        .name(format!("worker-{voice_id}"))
        .spawn({
            let voice_id = voice_id.to_string();
            move || {
                worker::run(&voice_id, rx);
            }
        })
        .unwrap();

    pool_lock.insert(voice_id.to_string(), tx.clone());

    tx
}

async fn synthesis(extract::Json(request): extract::Json<Request>) -> Response {
    if !VOICES
        .get()
        .unwrap()
        .iter()
        .any(|c| c.id == request.voice_id)
    {
        return (StatusCode::BAD_REQUEST, "INVALID VOICE ID").into_response();
    }

    let worker = get_worker(&request.voice_id).await;

    let (tx, rx) = oneshot::channel();

    worker
        .send(RequestContext {
            text: request.text,
            writeback: tx,
        })
        .await
        .unwrap();

    let output = rx.await.unwrap();

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "audio/wav")],
        output,
    )
        .into_response()
}

async fn voices() -> Json<Vec<Voice>> {
    Json(VOICES.get().unwrap().clone())
}

async fn root() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn init_voices() {
    let voices = unsafe { CopySpeechSynthesisVoicesForMode(std::ptr::null()) };
    if voices.is_null() {
        panic!("Failed to copy speech synthesis voices");
    }

    let count = unsafe { CFArrayGetCount(voices) };

    let mut voices_ = Vec::new();

    for i in 0..count {
        let voice_id = unsafe { CFArrayGetValueAtIndex(voices, i) as CFStringRef };

        let mut spec = VoiceSpec {
            creator: 0,
            id: 0,
            instance: 0,
        };

        if unsafe { MakeVoiceSpecForIdentifierString(voice_id, &mut spec) } != 1 {
            continue;
        }

        let mut info: CFDictionaryRef = std::ptr::null();

        unsafe { GetVoiceInfo(&spec, K_SPEECH_VOICE_ATTR_SELECTOR, &mut info) };

        if info.is_null() {
            continue;
        }

        let name =
            unsafe { CFDictionaryGetValue(info, kSpeechVoiceName as *const _) as CFStringRef };

        let locale_identifier = unsafe {
            CFDictionaryGetValue(info, kSpeechVoiceLocaleIdentifier as *const _) as CFStringRef
        };

        let demo_text =
            unsafe { CFDictionaryGetValue(info, kSpeechVoiceDemoText as *const _) as CFStringRef };

        let name = unsafe { cfstring_to_string(name) };
        let locale_identifier = unsafe { cfstring_to_string(locale_identifier) };
        let id = unsafe { cfstring_to_string(voice_id) };
        let demo_text = unsafe { cfstring_to_string(demo_text) };

        voices_.push(Voice {
            name,
            id,
            locale_identifier,
            demo_text,
        });

        unsafe { CFRelease(info as *const _) };
    }

    unsafe { CFRelease(voices as *const _) };

    VOICES.set(voices_).unwrap();
}

#[tokio::main]
async fn main() {
    init_voices().await;

    tracing_subscriber::fmt()
        .with_thread_names(true)
        .with_target(false)
        .init();

    let cli = Cli::parse();

    TEMPORARY_DIR.set(cli.temp_dir).unwrap();
    WORKER_POOL.set(Mutex::new(HashMap::new())).unwrap();

    let listener = TcpListener::bind(&cli.listen)
        .await
        .expect("Failed to bind");

    let app = Router::new()
        .route("/", get(root))
        .route("/api/voices", get(voices))
        .route("/api/synthesis", post(synthesis));

    tracing::info!("Listening on {}", &cli.listen);

    axum::serve(listener, app).await.unwrap();
}
