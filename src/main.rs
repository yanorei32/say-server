use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;

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
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::TcpListener,
    process::Command,
};

static CHARACTER_LIST: OnceCell<Vec<Character>> = OnceCell::new();
static TEMPORARY_DIR: OnceCell<PathBuf> = OnceCell::new();

#[derive(Debug, Clone, Deserialize)]
struct Request {
    name: String,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct Character {
    name: String,
    lang: String,
    sample: String,
}

#[derive(Debug, Parser)]
struct Cli {
    #[clap(env, long, short, default_value = "0.0.0.0:3000")]
    listen: SocketAddr,

    #[clap(env, long, short, default_value = "/tmp/")]
    temp_dir: PathBuf,
}

async fn run_synthesis(name: &str, text: &str) -> Vec<u8> {
    let start_at = Instant::now();
    let session = uuid::Uuid::new_v4();

    let mut temp_wav = TEMPORARY_DIR.get().unwrap().clone();
    temp_wav.push(format!("say-server_{}.wav", session));

    let temp_wav = temp_wav.into_os_string();
    let temp_wav = temp_wav
        .into_string()
        .expect("The temp_wav path can be convert to string");

    let mut temp_txt = TEMPORARY_DIR.get().unwrap().clone();
    temp_txt.push(format!("say-server_{}.txt", session));

    let temp_txt = temp_txt.into_os_string();
    let temp_txt = temp_txt
        .into_string()
        .expect("The temp_txt path can be convert to string");

    {
        let f = File::create_new(&temp_txt).await.expect("Create TXT file");
        let mut f = BufWriter::new(f);
        f.write_all(text.as_bytes()).await.expect("Write TXT file");
        f.flush().await.expect("Flush TXT file");
    }

    Command::new("say")
        .args([
            "-v",
            name,
            "-o",
            &temp_wav,
            "--file-format=WAVE",
            "--data-format=LEI24@22050",
            "-f",
            &temp_txt,
        ])
        .spawn()
        .expect("Failed to start say command")
        .wait()
        .await
        .expect("Failed to run say command");

    let buffer = {
        let wav = File::open(&temp_wav).await.expect("Open WAVE file");
        let mut wav = BufReader::new(wav);

        let mut buffer = vec![];
        wav.read_to_end(&mut buffer).await.expect("Read WAVE file");
        buffer
    };

    tokio::fs::remove_file(&temp_wav)
        .await
        .expect("Remove WAVE file");

    tokio::fs::remove_file(&temp_txt)
        .await
        .expect("Remove TXT file");

    tracing::info!("Synthesis: {:?}", Instant::now() - start_at);

    buffer
}

async fn synthesis(extract::Json(request): extract::Json<Request>) -> Response {
    if !CHARACTER_LIST
        .get()
        .unwrap()
        .iter()
        .any(|c| &c.name == &request.name)
    {
        return (StatusCode::BAD_REQUEST, "INVALID CHARACTER NAME").into_response();
    }

    let output = run_synthesis(&request.name, &request.text).await;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "audio/wav")],
        output,
    )
        .into_response()
}

async fn characters() -> Json<Vec<Character>> {
    Json(CHARACTER_LIST.get().unwrap().clone())
}

async fn root() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn init_character_list() {
    let output = Command::new("say")
        .args(["-v", "?"])
        .output()
        .await
        .expect("Failed to run say command");

    let output = String::from_utf8(output.stdout).expect("Failed to parse say command output");

    let characters: Vec<_> = output
        .split('\n')
        .flat_map(|line| {
            if line.is_empty() {
                return None;
            }

            let (character, sample) = line.split_once('#').expect("Sample Text");
            let (name, lang) = character.trim().rsplit_once(' ').expect("Character Info");

            let sample = sample.trim().to_string();
            let name = name.trim().to_string();
            let lang = lang.trim().to_string();

            Some(Character { name, sample, lang })
        })
        .collect();

    CHARACTER_LIST.set(characters).unwrap();
}

#[tokio::main]
async fn main() {
    init_character_list().await;

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    TEMPORARY_DIR.set(cli.temp_dir).unwrap();

    let listener = TcpListener::bind(&cli.listen)
        .await
        .expect("Failed to bind");

    let app = Router::new()
        .route("/", get(root))
        .route("/api/characters", get(characters))
        .route("/api/synthesis", post(synthesis));

    tracing::info!("Listening on {}", &cli.listen);

    axum::serve(listener, app).await.unwrap();
}
