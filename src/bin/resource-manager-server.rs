use elemaudio_resources::resource::{
    normalize_audio_resource_name, AudioBuffer, Resource, ResourceManager,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

const DEFAULT_ADDR: &str = "127.0.0.1:3030";

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[archive(check_bytes)]
struct ResourceEntry {
    id: String,
    kind: String,
    bytes: usize,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[archive(check_bytes)]
struct ResourceSnapshot {
    active: Option<String>,
    resources: Vec<ResourceEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ResourceMetadata {
    duration_ms: f64,
    channels: u16,
}

#[derive(Default)]
struct PlaybackState {
    active: Option<String>,
}

#[derive(Default)]
struct AppState {
    resources: ResourceManager,
    playback: PlaybackState,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: Vec<u8>,
}

fn main() -> io::Result<()> {
    let listener = TcpListener::bind(DEFAULT_ADDR)?;
    let mut state = AppState::default();
    println!("resource-manager-server listening on http://{DEFAULT_ADDR}");

    for stream in listener.incoming() {
        let mut stream = stream?;
        match read_request(&mut stream) {
            Ok(request) => {
                if let Err(error) = handle_request(&mut stream, &mut state, request) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("request read failed: {error}"),
        }
    }

    Ok(())
}

fn handle_request(
    stream: &mut TcpStream,
    state: &mut AppState,
    request: HttpRequest,
) -> io::Result<()> {
    if request.method == "OPTIONS" {
        return respond(stream, "204 No Content", "text/plain", &[]);
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => respond_text(stream, "ok"),
        ("GET", "/api/resources") => {
            let body = serde_json::to_vec(&snapshot_json(state)).unwrap();
            respond(stream, "200 OK", "application/json", &body)
        }
        ("GET", "/api/resources.bin") => {
            let snapshot = snapshot_json(state);
            let body = rkyv::to_bytes::<_, 256>(&snapshot).unwrap();
            respond(stream, "200 OK", "application/octet-stream", body.as_ref())
        }
        ("GET", "/api/resources/export.wav") => {
            let name = query_value(&request.query, "name")
                .map(|value| normalize_audio_resource_name(&value))
                .unwrap_or_default();
            match state.resources.get(name).cloned() {
                Some(resource) => {
                    let bytes = encode_wav(&resource)?;
                    respond(stream, "200 OK", "audio/wav", &bytes)
                }
                None => respond_not_found(stream),
            }
        }
        ("GET", "/api/resources/metadata") => {
            let name = query_value(&request.query, "name")
                .map(|value| normalize_audio_resource_name(&value))
                .unwrap_or_default();
            match state.resources.get(name).cloned() {
                Some(resource) => {
                    let metadata = resource_metadata(&resource)?;
                    respond(
                        stream,
                        "200 OK",
                        "application/json",
                        &serde_json::to_vec(&metadata).unwrap(),
                    )
                }
                None => respond_not_found(stream),
            }
        }
        // Loads a WAV into the Rust resource manager using a filename-derived resource id.
        // Mono buffers are kept as-is; multichannel buffers remain multichannel so the browser demo can route them with `mc.sample(...)`.
        ("POST", "/api/resources/load") => {
            let name =
                query_value(&request.query, "name").unwrap_or_else(|| "resource".to_string());
            let buffer = decode_wav(&request.body)?;
            let base_name = normalize_audio_resource_name(&name);
            state.resources.remove_matching_prefix(&base_name);
            state
                .resources
                .insert(base_name.clone(), Resource::audio(buffer))
                .map_err(io::Error::other)?;
            respond_text(stream, "ok")
        }
        ("POST", "/api/resources/play") => {
            let name = query_value(&request.query, "name")
                .map(|value| normalize_audio_resource_name(&value))
                .unwrap_or_default();
            if state
                .resources
                .get(name.clone())
                .and_then(|r| r.as_audio())
                .is_none()
            {
                return respond(
                    stream,
                    "400 Bad Request",
                    "application/json",
                    br#"{"success":false,"error":"resource is missing or not audio"}"#,
                );
            }
            state.playback.active = Some(name);
            respond_text(stream, "ok")
        }
        ("POST", "/api/resources/stop") => {
            state.playback.active = None;
            respond_text(stream, "ok")
        }
        ("GET", "/api/resources/active") => {
            let body =
                serde_json::to_vec(&serde_json::json!({"active": state.playback.active})).unwrap();
            respond(stream, "200 OK", "application/json", &body)
        }
        ("POST", "/api/resources/delete") => {
            let name = query_value(&request.query, "name")
                .map(|value| normalize_audio_resource_name(&value))
                .unwrap_or_default();
            let removed = state.resources.remove(name.clone());
            if state.playback.active.as_deref() == Some(name.as_str()) {
                state.playback.active = None;
            }
            let body = match removed {
                Ok(_) => serde_json::json!({"success": true}),
                Err(error) => serde_json::json!({"success": false, "error": error}),
            };
            respond(
                stream,
                "200 OK",
                "application/json",
                &serde_json::to_vec(&body).unwrap(),
            )
        }
        ("POST", "/api/resources/rename") => {
            let from = query_value(&request.query, "from").unwrap_or_default();
            let to = query_value(&request.query, "to").unwrap_or_default();
            let result = state.resources.rename(from.clone(), to.clone());
            if state.playback.active.as_deref() == Some(from.as_str()) {
                state.playback.active = Some(to.clone());
            }
            let body = match result {
                Ok(_) => serde_json::json!({"success": true}),
                Err(error) => serde_json::json!({"success": false, "error": error}),
            };
            respond(
                stream,
                "200 OK",
                "application/json",
                &serde_json::to_vec(&body).unwrap(),
            )
        }
        ("POST", "/api/resources/prune") => {
            let keep = query_value(&request.query, "keep").unwrap_or_default();
            let keep: Vec<String> = keep
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| normalize_audio_resource_name(s.trim()))
                .collect();
            let pruned = state.resources.prune_except(keep.clone());
            if let Some(active) = state.playback.active.clone() {
                if !keep.iter().any(|id| id == &active) {
                    state.playback.active = None;
                }
            }
            let body = serde_json::json!({"success": true, "pruned": pruned.len()});
            respond(
                stream,
                "200 OK",
                "application/json",
                &serde_json::to_vec(&body).unwrap(),
            )
        }
        _ => respond_not_found(stream),
    }
}

fn snapshot_json(state: &AppState) -> ResourceSnapshot {
    ResourceSnapshot {
        active: state.playback.active.clone(),
        resources: state
            .resources
            .snapshot()
            .into_iter()
            .map(|(id, resource)| ResourceEntry {
                id: id.as_str().to_string(),
                kind: resource.kind().to_string(),
                bytes: resource_bytes(&resource),
            })
            .collect(),
    }
}

fn resource_bytes(resource: &Resource) -> usize {
    match resource {
        Resource::Audio(buffer) => buffer.samples.len() * std::mem::size_of::<f32>(),
        Resource::F32(data) => data.len() * std::mem::size_of::<f32>(),
        Resource::F64(data) => data.len() * std::mem::size_of::<f64>(),
        Resource::Bytes(data) => data.len(),
        Resource::Text(data) => data.len(),
        Resource::Any(_) => 0,
    }
}

fn resource_metadata(resource: &Resource) -> io::Result<ResourceMetadata> {
    let buffer = match resource {
        Resource::Audio(buffer) => buffer,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resource is not audio",
            ));
        }
    };

    let frames = buffer.frames() as f64;
    let sample_rate = buffer.sample_rate as f64;
    let duration_ms = if sample_rate > 0.0 {
        (frames / sample_rate) * 1000.0
    } else {
        0.0
    };

    Ok(ResourceMetadata {
        duration_ms,
        channels: buffer.channels,
    })
}

fn decode_wav(bytes: &[u8]) -> io::Result<AudioBuffer> {
    let mss = MediaSourceStream::new(
        Box::new(io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("wav");
    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing default track"))?;
    if track.codec_params.codec == CODEC_TYPE_NULL {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported codec",
        ));
    }
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing sample rate"))?;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    error.to_string(),
                ))
            }
        };
        let decoded = decoder
            .decode(&packet)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        samples.extend_from_slice(buffer.samples());
    }
    AudioBuffer::new(samples, sample_rate, channels as u16)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn encode_wav(resource: &Resource) -> io::Result<Vec<u8>> {
    let (samples, sample_rate, channels) = match resource {
        Resource::Audio(buffer) => (
            buffer.samples.as_ref().to_vec(),
            buffer.sample_rate,
            buffer.channels,
        ),
        Resource::F32(samples) => (samples.as_ref().to_vec(), 44_100, 1),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resource is not audio",
            ))
        }
    };
    let data_bytes = samples.len() * 2;
    let mut bytes = Vec::with_capacity(44 + data_bytes);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36u32 + data_bytes as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in samples {
        bytes
            .extend_from_slice(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    Ok(bytes)
}

fn read_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let request_line = request_line.trim_end();
    if request_line.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty request",
        ));
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("");
    let (path, query) = split_target(target);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(HttpRequest {
        method,
        path,
        query,
        body,
    })
}

fn split_target(target: &str) -> (String, HashMap<String, String>) {
    let mut query = HashMap::new();
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    for pair in query_string
        .split('&')
        .filter(|segment| !segment.is_empty())
    {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(key.to_string(), value.to_string());
    }
    (path.to_string(), query)
}

fn query_value(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query.get(key).cloned()
}

fn respond_text(stream: &mut TcpStream, text: &str) -> io::Result<()> {
    respond(stream, "200 OK", "text/plain", text.as_bytes())
}

fn respond_not_found(stream: &mut TcpStream) -> io::Result<()> {
    let body = serde_json::json!({"success": false, "error": "not found"});
    respond(
        stream,
        "404 Not Found",
        "application/json",
        &serde_json::to_vec(&body).unwrap(),
    )
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)?;
    stream.flush()
}
