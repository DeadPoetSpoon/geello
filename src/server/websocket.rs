//! Bounded WebSocket control/state machine for one published map.
use super::*;
use axum::extract::ws::{Message, WebSocket};
use axum::{
    Router,
    extract::{Path, State, WebSocketUpgrade},
    http::Uri,
    response::Response,
    routing::get,
};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

const MAX_CONTROL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewState {
    crs: String,
    bbox: [f64; 4],
    width: u32,
    height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Playback {
    time: f64,
    speed: f64,
    playing: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    format: String,
    fps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jpeg_quality: Option<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionState {
    view: ViewState,
    variant: String,
    playback: Playback,
    output: Output,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Control {
    #[serde(rename = "init")]
    Init {
        request_id: String,
        view: ViewState,
        variant: Variant,
        playback: Option<Playback>,
        output: Option<Output>,
    },
    #[serde(rename = "update")]
    Update {
        request_id: String,
        view: Option<ViewState>,
        variant: Option<Variant>,
        playback: Option<Playback>,
        output: Option<Output>,
    },
    #[serde(rename = "retry")]
    Retry { request_id: String },
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    id: String,
}
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ServerControl<'a> {
    #[serde(rename = "hello")]
    Hello {
        protocol: u32,
        backend: &'a str,
        formats: [&'static str; 2],
        max_fps: u32,
        max_control_bytes: usize,
        default_fps: u32,
    },
    #[serde(rename = "ack")]
    Ack {
        request_id: &'a str,
        revision: u64,
        state: &'a SessionState,
    },
    #[serde(rename = "frame")]
    Frame {
        frame_id: u64,
        revision: u64,
        time: f64,
        render_revision: &'a str,
        content_type: &'static str,
        bytes: usize,
        width: u32,
        height: u32,
        backend: &'a str,
        incomplete: bool,
    },
    #[serde(rename = "event")]
    Event { code: &'static str },
    #[serde(rename = "error")]
    Error {
        request_id: Option<&'a str>,
        code: &'static str,
        message: &'static str,
    },
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new().route("/ws/maps/{id}", get(upgrade))
}
async fn upgrade(
    State(s): State<Arc<ServerState>>,
    Path((id,)): Path<(String,)>,
    uri: Uri,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if uri.query().is_some_and(|query| !query.is_empty()) {
        return Err(ServerError::invalid(
            "WebSocket route does not accept query parameters",
        ));
    }
    let pin = s
        .catalog
        .pin(&ResourceKey::new(ResourceKind::Map, id.clone())?)?;
    let activity = s.activities.enter()?;
    Ok(ws
        .max_frame_size(MAX_CONTROL_BYTES)
        .max_message_size(MAX_CONTROL_BYTES)
        .on_upgrade(move |socket| async move {
            let _activity = activity;
            session(s, id, pin, socket).await;
        }))
}
async fn session(s: Arc<ServerState>, id: String, _pin: ResourcePin, socket: WebSocket) {
    drop(_pin);
    let (tx, rx) = socket.split();
    tokio::select! {
        _ = run_session(s.clone(), id, tx, rx) => {},
        _ = s.activities.closed() => {},
    }
}

struct RenderedFrame {
    result: Result<Arc<EncodedResponse>>,
    state: SessionState,
    revision: u64,
    request_id: Option<String>,
}
type RenderJob = std::pin::Pin<Box<dyn std::future::Future<Output = RenderedFrame> + Send>>;
fn render_job(
    s: Arc<ServerState>,
    id: String,
    revision: u64,
    state: SessionState,
    request_id: Option<String>,
) -> RenderJob {
    Box::pin(async move {
        let result = async {
            let view = crate::View {
                crs: super::runtime::parse_crs(&state.view.crs)?,
                bounds: crate::Bounds {
                    min_x: state.view.bbox[0],
                    min_y: state.view.bbox[1],
                    max_x: state.view.bbox[2],
                    max_y: state.view.bbox[3],
                },
                width: state.view.width,
                height: state.view.height,
                aspect: crate::AspectPolicy::Stretch,
            };
            let format = if state.output.format == "jpeg" {
                crate::encoding::ImageFormat::Jpeg {
                    quality: state.output.jpeg_quality.unwrap_or(90),
                    background: [255, 255, 255],
                }
            } else {
                crate::encoding::ImageFormat::Png
            };
            s.render_animation_format(&id, view, state.playback.time, format)
                .await
        }
        .await;
        RenderedFrame {
            result,
            state,
            revision,
            request_id,
        }
    })
}
async fn run_session<T, R, E>(s: Arc<ServerState>, id: String, mut tx: T, mut rx: R)
where
    T: Sink<Message> + Unpin,
    R: Stream<Item = std::result::Result<Message, E>> + Unpin,
{
    let mut changes = s.catalog.subscribe();
    let key = ResourceKey::new(ResourceKind::Map, &id).ok();
    let mut graph = key.as_ref().and_then(|key| s.catalog.pin(key).ok());
    let send_timeout = Duration::from_millis(s.config.profile.operation_timeout_ms);
    let hello = serde_json::to_string(&ServerControl::Hello {
        protocol: 1,
        backend: &s.backend_name,
        formats: ["png", "jpeg"],
        max_fps: s.config.profile.max_animation_fps,
        max_control_bytes: MAX_CONTROL_BYTES,
        default_fps: 30.min(s.config.profile.max_animation_fps),
    })
    .unwrap();
    if send_message(&mut tx, Message::Text(hello.into()), send_timeout)
        .await
        .is_err()
    {
        return;
    }
    let init_deadline = Instant::now() + send_timeout;
    let mut state: Option<SessionState> = None;
    let mut revision = 0u64;
    let mut frame_id = 0u64;
    let mut clock_anchor = Instant::now();
    let mut clock_time = 0.0;
    let mut suspended = false;
    let mut pending: Option<RenderJob> = None;
    let mut next_tick = Instant::now();
    loop {
        let playing = !suspended && state.as_ref().is_some_and(|v| v.playback.playing);
        let period = state
            .as_ref()
            .map(|v| Duration::from_secs_f64(1.0 / v.output.fps as f64))
            .unwrap_or(Duration::from_secs(1));
        tokio::select! {
            biased;
            _ = s.activities.closed() => break,
            _ = tokio::time::sleep_until(init_deadline), if state.is_none() => {
                let _ = send_error(&mut tx,send_timeout,None,"init-timeout","Initialization deadline expired").await;
                break;
            }
            changed = changes.changed() => {
                if changed.is_err() { break; }
                let Some(key) = key.as_ref() else { continue };
                let next = s.catalog.pin(key);
                let Ok(next) = next else {
                    if graph.is_some() {
                        let event = serde_json::to_string(&ServerControl::Event { code: "resource-unpublished" }).unwrap();
                        let _ = send_message(&mut tx, Message::Text(event.into()), send_timeout).await;
                        let _ = send_message(&mut tx, Message::Close(None), send_timeout).await;
                        break;
                    }
                    continue;
                };
                let different = graph.as_ref().is_some_and(|old| old.records.len() != next.records.len()
                    || old.records.iter().any(|(key, record)| next.records.get(key).is_none_or(|next| record.version != next.version)));
                graph = Some(next);
                if !different { continue; }
                pending = None;
                let event = serde_json::to_string(&ServerControl::Event { code: "resource-changed" }).unwrap();
                if send_message(&mut tx, Message::Text(event.into()), send_timeout).await.is_err() { break; }
                if !suspended && let Some(current) = state.as_mut() {
                    current.playback.time = advance_time(clock_time, clock_anchor, &current.playback);
                    pending = Some(render_job(s.clone(),id.clone(),revision,current.clone(),None));
                    next_tick=Instant::now()+period;
                }
            }
            message = rx.next() => {
                let Some(Ok(message)) = message else { break };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Ping(payload) => {
                        if send_message(&mut tx,Message::Pong(payload),send_timeout).await.is_err() {break;}
                        continue;
                    }
                    Message::Pong(_) => continue,
                    Message::Close(_) => break,
                    Message::Binary(_) => {
                        if send_error(&mut tx,send_timeout,None,"binary-control","Client control must be JSON text").await.is_err() {break;}
                        continue;
                    }
                };
                if text.len()>MAX_CONTROL_BYTES {
                    if send_error(&mut tx,send_timeout,None,"message-too-large","Control message exceeds limit").await.is_err() {break;}
                    continue;
                }
                let control=match serde_json::from_str::<Control>(&text) {
                    Ok(control)=>control,
                    Err(_)=>{
                        if send_error(&mut tx,send_timeout,None,"invalid-json","Control message is invalid").await.is_err() {break;}
                        continue;
                    }
                };
                let current=state.clone().map(|mut current| {
                    current.playback.time=advance_time(clock_time,clock_anchor,&current.playback);
                    current
                });
                let mut next_revision=revision;
                let (request_id,next)=match apply(control,current,&mut next_revision) {
                    Ok(next)=>next,
                    Err((request,code,message))=>{
                        if send_error(&mut tx,send_timeout,request.as_deref(),code,message).await.is_err() {break;}
                        continue;
                    }
                };
                if next.view.width>s.config.profile.max_dimension || next.view.height>s.config.profile.max_dimension || next.output.fps>s.config.profile.max_animation_fps {
                    // apply validated the shared protocol ceiling; deployment
                    // limits are checked before committing the connection state.
                    if send_error(&mut tx,send_timeout,Some(&request_id),"invalid-update","Canvas exceeds deployment profile").await.is_err() {break;}
                    continue;
                }
                revision=next_revision;
                clock_time=next.playback.time;
                clock_anchor=Instant::now();
                let ack=serde_json::to_string(&ServerControl::Ack {request_id:&request_id,revision,state:&next}).unwrap();
                if send_message(&mut tx,Message::Text(ack.into()),send_timeout).await.is_err() {break;}
                // One pending future: dropping its predecessor cancels queued
                // work cooperatively. Already submitted work keeps its permits.
                pending=Some(render_job(s.clone(),id.clone(),revision,next.clone(),Some(request_id)));
                next_tick=Instant::now()+Duration::from_secs_f64(1.0/next.output.fps as f64);
                state=Some(next);
                suspended=false;
            }
            rendered = async { match pending.as_mut() {Some(job)=>job.await,None=>std::future::pending().await} } => {
                pending=None;
                let outcome=send_rendered_frame(&mut tx,send_timeout,&mut frame_id,rendered).await;
                if matches!(outcome,FrameOutcome::Closed) {break;}
                suspended=suspension_after(&outcome,suspended);
            }
            _ = tokio::time::sleep_until(next_tick), if playing && pending.is_none() => {
                let Some(current)=state.as_mut() else {continue};
                current.playback.time=advance_time(clock_time,clock_anchor,&current.playback);
                pending=Some(render_job(s.clone(),id.clone(),revision,current.clone(),None));
                next_tick=Instant::now()+period;
            }
        }
    }
}
enum FrameOutcome {
    Sent,
    Failed,
    Closed,
}
async fn send_rendered_frame<T: Sink<Message> + Unpin>(
    tx: &mut T,
    timeout: Duration,
    frame_id: &mut u64,
    rendered: RenderedFrame,
) -> FrameOutcome {
    match rendered.result {
        Ok(image) => {
            let Some(next) = frame_id.checked_add(1) else {
                return FrameOutcome::Closed;
            };
            *frame_id = next;
            let meta = serde_json::to_string(&ServerControl::Frame {
                frame_id: *frame_id,
                revision: rendered.revision,
                time: rendered.state.playback.time,
                render_revision: &image.render_revision,
                content_type: if image.content_type == "image/jpeg" {
                    "image/jpeg"
                } else {
                    "image/png"
                },
                bytes: image.bytes.len(),
                width: rendered.state.view.width,
                height: rendered.state.view.height,
                backend: &image.backend,
                incomplete: image.incomplete,
            })
            .unwrap();
            if send_message(tx, Message::Text(meta.into()), timeout)
                .await
                .is_err()
            {
                return FrameOutcome::Closed;
            }
            if send_message(tx, Message::Binary(image.bytes.clone().into()), timeout)
                .await
                .is_err()
            {
                FrameOutcome::Closed
            } else {
                FrameOutcome::Sent
            }
        }
        Err(_) => {
            if send_error(
                tx,
                timeout,
                rendered.request_id.as_deref(),
                "render-failed",
                "Frame rendering failed",
            )
            .await
            .is_err()
            {
                FrameOutcome::Closed
            } else {
                FrameOutcome::Failed
            }
        }
    }
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|c| c.is_ascii_graphic() && c != '"')
}
fn valid_request_id(value: &str) -> bool {
    value.len() <= 128 && valid_id(value)
}
fn valid_crs(value: &str) -> bool {
    matches!(value, "OGC:CRS84" | "EPSG:4326" | "EPSG:3857")
}
fn advance_time(base: f64, anchor: Instant, playback: &Playback) -> f64 {
    if playback.playing {
        let time = base + anchor.elapsed().as_secs_f64() * playback.speed;
        if time.is_finite() { time } else { f64::MAX }
    } else {
        base
    }
}
fn validate_state(state: &SessionState) -> std::result::Result<(), &'static str> {
    if !valid_id(&state.variant) || state.variant != "default" {
        return Err("variant is not published");
    }
    if !state.playback.time.is_finite()
        || state.playback.time < 0.
        || !state.playback.speed.is_finite()
        || state.playback.speed <= 0.
    {
        return Err("invalid playback");
    }
    if !matches!(state.output.format.as_str(), "png" | "jpeg")
        || state.output.fps == 0
        || state.output.fps > 1000
    {
        return Err("invalid output");
    }
    if (state.output.format == "jpeg"
        && !state
            .output
            .jpeg_quality
            .is_some_and(|q| (1..=100).contains(&q)))
        || (state.output.format == "png" && state.output.jpeg_quality.is_some())
    {
        return Err("invalid JPEG quality");
    }
    if !valid_crs(&state.view.crs) {
        return Err("invalid view CRS");
    }
    if state.view.width == 0
        || state.view.height == 0
        || state.view.width > u16::MAX as u32
        || state.view.height > u16::MAX as u32
        || state.view.bbox[0] >= state.view.bbox[2]
        || state.view.bbox[1] >= state.view.bbox[3]
        || state.view.bbox.iter().any(|v| !v.is_finite())
    {
        return Err("invalid view");
    }
    Ok(())
}
fn apply(
    control: Control,
    current: Option<SessionState>,
    revision: &mut u64,
) -> std::result::Result<(String, SessionState), (Option<String>, &'static str, &'static str)> {
    match control {
        Control::Init {
            request_id,
            view,
            variant,
            playback,
            output,
        } => {
            if !valid_request_id(&request_id) {
                return Err((
                    Some(request_id),
                    "invalid-request-id",
                    "Request ID is empty or exceeds 128 bytes",
                ));
            }
            if current.is_some() {
                return Err((
                    Some(request_id),
                    "already-initialized",
                    "Connection is already initialized",
                ));
            }
            let state = SessionState {
                view,
                variant: variant.id,
                playback: playback.unwrap_or(Playback {
                    time: 0.,
                    speed: 1.,
                    playing: false,
                }),
                output: output.unwrap_or(Output {
                    format: "png".into(),
                    fps: 30,
                    jpeg_quality: None,
                }),
            };
            validate_state(&state).map_err(|m| (Some(request_id.clone()), "invalid-init", m))?;
            *revision = 1;
            Ok((request_id, state))
        }
        Control::Update {
            request_id,
            view,
            variant,
            playback,
            output,
        } => {
            if !valid_request_id(&request_id) {
                return Err((
                    Some(request_id),
                    "invalid-request-id",
                    "Request ID is empty or exceeds 128 bytes",
                ));
            }
            if view.is_none()
                && variant.is_none()
                && playback.is_none()
                && output.is_none()
                && current.is_some()
            {
                return Err((
                    Some(request_id),
                    "empty-update",
                    "Update requires at least one complete state group",
                ));
            }
            let mut state = current.ok_or((
                Some(request_id.clone()),
                "not-initialized",
                "Connection is not initialized",
            ))?;
            if let Some(v) = view {
                state.view = v
            }
            if let Some(v) = variant {
                state.variant = v.id
            }
            if let Some(v) = playback {
                state.playback = v
            }
            if let Some(v) = output {
                state.output = v
            }
            validate_state(&state).map_err(|m| (Some(request_id.clone()), "invalid-update", m))?;
            *revision = revision.checked_add(1).ok_or((
                Some(request_id.clone()),
                "revision-limit",
                "Connection revision exhausted",
            ))?;
            Ok((request_id, state))
        }
        Control::Retry { request_id } => {
            if !valid_request_id(&request_id) {
                return Err((
                    Some(request_id),
                    "invalid-request-id",
                    "Request ID is empty or exceeds 128 bytes",
                ));
            }
            let state = current.ok_or((
                Some(request_id.clone()),
                "not-initialized",
                "Connection is not initialized",
            ))?;
            Ok((request_id, state))
        }
    }
}
async fn send_error<T: Sink<Message> + Unpin>(
    tx: &mut T,
    timeout: Duration,
    request_id: Option<&str>,
    code: &'static str,
    message: &'static str,
) -> std::result::Result<(), ()> {
    let text = serde_json::to_string(&ServerControl::Error {
        request_id,
        code,
        message,
    })
    .map_err(|_| ())?;
    send_message(tx, Message::Text(text.into()), timeout).await
}

/// A timed-out write leaves framing uncertain. Callers must drop the transport
/// instead of retrying or sending another application message on that sink.
async fn send_message<T: Sink<Message> + Unpin>(
    tx: &mut T,
    message: Message,
    timeout: Duration,
) -> std::result::Result<(), ()> {
    tokio::time::timeout(timeout, tx.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

fn suspension_after(outcome: &FrameOutcome, previous: bool) -> bool {
    match outcome {
        FrameOutcome::Sent => false,
        FrameOutcome::Failed => true,
        FrameOutcome::Closed => previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> ViewState {
        ViewState {
            crs: "OGC:CRS84".into(),
            bbox: [0., 0., 1., 1.],
            width: 64,
            height: 64,
        }
    }
    #[test]
    fn state_machine_requires_init_and_preserves_state_on_invalid_update() {
        let mut revision = 0;
        let update = Control::Update {
            request_id: "u".into(),
            view: None,
            variant: None,
            playback: None,
            output: None,
        };
        assert_eq!(
            apply(update, None, &mut revision).unwrap_err().1,
            "not-initialized"
        );
        let init = Control::Init {
            request_id: "i".into(),
            view: view(),
            variant: Variant {
                id: "default".into(),
            },
            playback: None,
            output: None,
        };
        let (_, state) = apply(init, None, &mut revision).unwrap();
        assert_eq!(revision, 1);
        let bad = Control::Update {
            request_id: "bad".into(),
            view: Some(ViewState { width: 0, ..view() }),
            variant: None,
            playback: None,
            output: None,
        };
        assert_eq!(
            apply(bad, Some(state.clone()), &mut revision)
                .unwrap_err()
                .1,
            "invalid-update"
        );
        assert_eq!(revision, 1);
        let repeat = Control::Init {
            request_id: "again".into(),
            view: view(),
            variant: Variant {
                id: "default".into(),
            },
            playback: None,
            output: None,
        };
        assert_eq!(
            apply(repeat, Some(state), &mut revision).unwrap_err().1,
            "already-initialized"
        );
    }
    #[test]
    fn controls_reject_nonfinite_playback_and_nondefault_output() {
        let mut revision = 0;
        let bad = Control::Init {
            request_id: "i".into(),
            view: view(),
            variant: Variant {
                id: "default".into(),
            },
            playback: Some(Playback {
                time: f64::NAN,
                speed: 1.,
                playing: false,
            }),
            output: Some(Output {
                format: "jpeg".into(),
                fps: 30,
                jpeg_quality: None,
            }),
        };
        assert_eq!(
            apply(bad, None, &mut revision).unwrap_err().1,
            "invalid-init"
        );
    }
    #[test]
    fn controls_reject_invalid_request_ids_and_crs() {
        let mut revision = 0;
        let bad_id = Control::Init {
            request_id: "".into(),
            view: view(),
            variant: Variant {
                id: "default".into(),
            },
            playback: None,
            output: None,
        };
        assert_eq!(
            apply(bad_id, None, &mut revision).unwrap_err().1,
            "invalid-request-id"
        );
        let bad_crs = Control::Init {
            request_id: "ok".into(),
            view: ViewState {
                crs: "EPSG:999999".into(),
                ..view()
            },
            variant: Variant {
                id: "default".into(),
            },
            playback: None,
            output: None,
        };
        assert_eq!(
            apply(bad_crs, None, &mut revision).unwrap_err().1,
            "invalid-init"
        );
    }
    #[test]
    fn playing_clock_is_monotonic_and_pause_holds_time() {
        let anchor = Instant::now();
        let playback = Playback {
            time: 2.0,
            speed: 3.0,
            playing: true,
        };
        std::thread::sleep(std::time::Duration::from_millis(2));
        let first = advance_time(2.0, anchor, &playback);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = advance_time(2.0, anchor, &playback);
        assert!(first >= 2.0);
        assert!(second >= first);
        let paused = Playback {
            playing: false,
            ..playback
        };
        assert_eq!(advance_time(2.0, anchor, &paused), 2.0);
    }
    #[test]
    fn failed_retry_remains_suspended_and_successful_redraw_resumes() {
        let suspended = suspension_after(&FrameOutcome::Failed, false);
        assert!(suspended);
        assert!(suspension_after(&FrameOutcome::Failed, suspended));
        assert!(!suspension_after(&FrameOutcome::Sent, suspended));
    }

    #[tokio::test]
    #[cfg(feature = "render-cpu")]
    async fn tcp_upgrade_pairs_frames_preserves_invalid_update_and_releases_pin() {
        use std::io::{Read, Write};
        use tower::ServiceExt;
        let root = tempfile::tempdir().unwrap();
        let mut config =
            ServerConfig::load(std::path::Path::new("examples/server.experimental.toml")).unwrap();
        config.backend = BackendSelection::Cpu;
        config.profile.response_memory_bytes = 8 << 20;
        config.profile.response_disk_bytes = 64 << 20;
        config.data_directory = root.path().join("data");
        let s = ServerState::open(config).await.unwrap();
        let key = ResourceKey::new(ResourceKind::Map, "test:tcp").unwrap();
        s.catalog
            .publish(
                ResourceRecord {
                    key: key.clone(),
                    ownership: Ownership::External,
                    version: "v1".into(),
                    published: true,
                    source: SourceSpec {
                        path: root.path().join("map.toml"),
                        format: "toml".into(),
                        encoding: None,
                        crs: None,
                        sample_kind: None,
                    },
                    dependencies: vec![],
                    data_version: "d1".into(),
                    derived_version: None,
                    derived_path: None,
                    managed_directory: None,
                    class: None,
                    effective_crs: None,
                    definition: Some(
                        "schema_version=1\nid='test:tcp'\noutput_crs=['OGC:CRS84']\nitems=[]\n"
                            .into(),
                    ),
                    type_bindings: vec![],
                },
                &Precondition::Absent,
            )
            .unwrap();
        let app = super::super::api::router(s.clone());
        let slots = s
            .requests
            .clone()
            .acquire_many_owned(s.requests.available_permits() as u32)
            .await
            .unwrap();
        let capacity = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health/ready")
                    .header("x-request-id", "at-capacity")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            capacity.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(capacity.headers()["x-request-id"], "at-capacity");
        drop(slots);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        tokio::task::spawn_blocking(move || {
            let mut socket = std::net::TcpStream::connect(address).unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            socket.set_write_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            write!(socket, "GET /ws/maps/test:tcp HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nX-Request-Id: tcp-upgrade\r\n\r\n").unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
                assert!(headers.len() < 8192);
            }
            let headers = String::from_utf8(headers).unwrap().to_ascii_lowercase();
            assert!(headers.starts_with("http/1.1 101"));
            assert!(headers.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="));
            assert!(headers.contains("x-request-id: tcp-upgrade\r\n"));
            fn receive(socket: &mut std::net::TcpStream) -> (u8, Vec<u8>) {
                let mut header = [0; 2];
                socket.read_exact(&mut header).unwrap();
                assert_eq!(header[0] & 0x80, 0x80, "test expects unfragmented messages");
                assert_eq!(header[1] & 0x80, 0);
                let length = match header[1] & 127 {
                    126 => { let mut bytes = [0; 2]; socket.read_exact(&mut bytes).unwrap(); u16::from_be_bytes(bytes) as u64 },
                    127 => { let mut bytes = [0; 8]; socket.read_exact(&mut bytes).unwrap(); u64::from_be_bytes(bytes) },
                    n => n as u64,
                };
                assert!(length <= 65536);
                let mut bytes = vec![0; length as usize];
                socket.read_exact(&mut bytes).unwrap();
                (header[0] & 15, bytes)
            }
            fn control(socket: &mut std::net::TcpStream) -> serde_json::Value {
                let (opcode, bytes) = receive(socket);
                assert_eq!(opcode, 1);
                serde_json::from_slice(&bytes).unwrap()
            }
            fn send(socket: &mut std::net::TcpStream, opcode: u8, bytes: &[u8]) {
                let mask = [1, 2, 3, 4];
                let mut frame = vec![0x80 | opcode];
                if bytes.len() < 126 { frame.push(0x80 | bytes.len() as u8); }
                else { frame.push(0x80 | 126); frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes()); }
                frame.extend_from_slice(&mask);
                frame.extend(bytes.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
                socket.write_all(&frame).unwrap();
            }
            assert_eq!(control(&mut socket)["type"], "hello");
            send(&mut socket, 1, br#"{"type":"init","request_id":"init","view":{"crs":"OGC:CRS84","bbox":[0,0,1,1],"width":16,"height":16},"variant":{"id":"default"},"playback":{"time":5,"speed":1,"playing":false}}"#);
            assert_eq!(control(&mut socket)["revision"], 1);
            let first = control(&mut socket);
            assert_eq!(first["type"], "frame");
            assert_eq!(first["frame_id"], 1);
            assert_eq!(first["time"], 5.0);
            let (opcode, bytes) = receive(&mut socket);
            assert_eq!(opcode, 2);
            assert_eq!(bytes.len() as u64, first["bytes"].as_u64().unwrap());
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            send(&mut socket, 1, br#"{"type":"update","request_id":"bad","view":{"crs":"OGC:CRS84","bbox":[0,0,1,1],"width":0,"height":16},"playback":{"time":99,"speed":1,"playing":false}}"#);
            assert_eq!(control(&mut socket)["code"], "invalid-update");
            send(&mut socket, 1, br#"{"type":"retry","request_id":"retry"}"#);
            let ack = control(&mut socket);
            assert_eq!(ack["revision"], 1);
            assert_eq!(ack["state"]["playback"]["time"], 5.0);
            assert_eq!(ack["state"]["view"]["width"], 16);
            let second = control(&mut socket);
            assert_eq!(second["frame_id"], 2);
            assert_eq!(second["render_revision"], first["render_revision"]);
            assert_eq!(receive(&mut socket).0, 2);
            send(&mut socket, 9, b"check");
            assert_eq!(receive(&mut socket), (10, b"check".to_vec()));
            send(&mut socket, 8, &[]);
        }).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while s.catalog.stats().pinned_versions != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap();
        s.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "render-cpu")]
    async fn session_suspends_after_failed_init_and_failed_retry_then_redraws_on_update() {
        let root = tempfile::tempdir().unwrap();
        let mut config =
            ServerConfig::load(std::path::Path::new("examples/server.experimental.toml")).unwrap();
        config.backend = BackendSelection::Cpu;
        config.profile.response_memory_bytes = 8 << 20;
        config.profile.response_disk_bytes = 64 << 20;
        config.data_directory = root.path().join("data");
        config.profile.operation_timeout_ms = 100;
        let s = ServerState::open(config).await.unwrap();
        let (controls, input) = tokio::sync::mpsc::channel::<Message>(8);
        let (output, mut messages) = tokio::sync::mpsc::channel::<Message>(8);
        let rx = Box::pin(futures_util::stream::unfold(input, |mut input| async {
            input
                .recv()
                .await
                .map(|message| (Ok::<_, ()>(message), input))
        }));
        let tx = Box::pin(futures_util::sink::unfold(
            output,
            |output, message| async move {
                output.send(message).await.map_err(|_| ())?;
                Ok::<_, ()>(output)
            },
        ));
        let server = s.clone();
        let task = tokio::spawn(async move { run_session(server, "test:ws".into(), tx, rx).await });
        async fn control(messages: &mut tokio::sync::mpsc::Receiver<Message>) -> serde_json::Value {
            let Message::Text(text) = tokio::time::timeout(Duration::from_secs(5), messages.recv())
                .await
                .unwrap()
                .unwrap()
            else {
                panic!("expected JSON")
            };
            serde_json::from_str(&text).unwrap()
        }
        assert_eq!(control(&mut messages).await["type"], "hello");
        controls.send(Message::Text(serde_json::json!({"type":"init","request_id":"init","view":{"crs":"OGC:CRS84","bbox":[0,0,1,1],"width":16,"height":16},"variant":{"id":"default"},"playback":{"time":0,"speed":1,"playing":true},"output":{"format":"png","fps":60}}).to_string().into())).await.unwrap();
        assert_eq!(control(&mut messages).await["type"], "ack");
        assert_eq!(control(&mut messages).await["code"], "render-failed");
        assert!(
            tokio::time::timeout(Duration::from_millis(80), messages.recv())
                .await
                .is_err()
        );
        controls
            .send(Message::Text(
                r#"{"type":"retry","request_id":"retry"}"#.into(),
            ))
            .await
            .unwrap();
        let ack = control(&mut messages).await;
        assert_eq!(ack["request_id"], "retry");
        assert_eq!(ack["revision"], 1);
        assert!(ack["state"]["playback"]["time"].as_f64().unwrap() > 0.);
        assert_eq!(control(&mut messages).await["code"], "render-failed");
        assert!(
            tokio::time::timeout(Duration::from_millis(80), messages.recv())
                .await
                .is_err()
        );
        // Repair publication while suspended, then explicitly redraw current state.
        s.catalog
            .publish(
                ResourceRecord {
                    key: ResourceKey::new(ResourceKind::Map, "test:ws").unwrap(),
                    ownership: Ownership::External,
                    version: "v1".into(),
                    published: true,
                    source: SourceSpec {
                        path: root.path().join("map.toml"),
                        format: "toml".into(),
                        encoding: None,
                        crs: None,
                        sample_kind: None,
                    },
                    dependencies: vec![],
                    data_version: "d1".into(),
                    derived_version: None,
                    derived_path: None,
                    managed_directory: None,
                    class: None,
                    effective_crs: None,
                    definition: Some(
                        "schema_version=1\nid='test:ws'\noutput_crs=['OGC:CRS84']\nitems=[]\n"
                            .into(),
                    ),
                    type_bindings: vec![],
                },
                &Precondition::Absent,
            )
            .unwrap();
        controls.send(Message::Text(r#"{"type":"update","request_id":"repair","playback":{"time":5,"speed":1,"playing":false}}"#.into())).await.unwrap();
        assert_eq!(control(&mut messages).await["revision"], 2);
        let frame = control(&mut messages).await;
        assert_eq!(frame["type"], "frame");
        assert_eq!(frame["time"], 5.0);
        let original_revision = frame["render_revision"].clone();
        let Some(Message::Binary(bytes)) = messages.recv().await else {
            panic!("expected PNG after metadata")
        };
        assert_eq!(bytes.len() as u64, frame["bytes"].as_u64().unwrap());
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        controls.send(Message::Close(None)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        // Reconnect supplies full client state to the current published graph.
        let key = ResourceKey::new(ResourceKind::Map, "test:ws").unwrap();
        let mut replacement = s.catalog.get(&key).unwrap();
        replacement.version = "v2".into();
        s.catalog
            .publish(replacement, &Precondition::Version("v1".into()))
            .unwrap();
        let (controls, input) = tokio::sync::mpsc::channel::<Message>(8);
        let (output, mut messages) = tokio::sync::mpsc::channel::<Message>(8);
        let rx = Box::pin(futures_util::stream::unfold(input, |mut input| async {
            input
                .recv()
                .await
                .map(|message| (Ok::<_, ()>(message), input))
        }));
        let tx = Box::pin(futures_util::sink::unfold(
            output,
            |output, message| async move {
                output.send(message).await.map_err(|_| ())?;
                Ok::<_, ()>(output)
            },
        ));
        let server = s.clone();
        let task = tokio::spawn(async move { run_session(server, "test:ws".into(), tx, rx).await });
        assert_eq!(control(&mut messages).await["type"], "hello");
        controls.send(Message::Text(serde_json::json!({"type":"init","request_id":"reconnect","view":{"crs":"OGC:CRS84","bbox":[0,0,1,1],"width":16,"height":16},"variant":{"id":"default"},"playback":{"time":5,"speed":1,"playing":false},"output":{"format":"png","fps":30}}).to_string().into())).await.unwrap();
        let ack = control(&mut messages).await;
        assert_eq!(ack["revision"], 1);
        assert_eq!(ack["state"]["playback"]["time"], 5.0);
        let frame = control(&mut messages).await;
        assert_ne!(frame["render_revision"], original_revision);
        assert_eq!(frame["time"], 5.0);
        assert!(matches!(messages.recv().await, Some(Message::Binary(_))));
        // Queue a burst without yielding. Every accepted control is acknowledged
        // in receive order; only the latest unstarted render may be delivered.
        for time in [6, 7, 8] {
            controls.try_send(Message::Text(serde_json::json!({"type":"update","request_id":format!("seek-{time}"),"playback":{"time":time,"speed":1,"playing":false}}).to_string().into())).unwrap();
        }
        for revision in [2, 3, 4] {
            assert_eq!(control(&mut messages).await["revision"], revision);
        }
        let latest = control(&mut messages).await;
        assert_eq!(latest["type"], "frame");
        assert_eq!(latest["revision"], 4);
        assert_eq!(latest["time"], 8.0);
        assert!(matches!(messages.recv().await, Some(Message::Binary(_))));
        let previous_revision = frame["render_revision"].clone();
        let mut replacement = s.catalog.get(&key).unwrap();
        replacement.version = "v3".into();
        s.catalog
            .publish(replacement, &Precondition::Version("v2".into()))
            .unwrap();
        assert_eq!(control(&mut messages).await["code"], "resource-changed");
        let refreshed = control(&mut messages).await;
        assert_eq!(refreshed["type"], "frame");
        assert_eq!(refreshed["revision"], 4);
        assert_eq!(refreshed["time"], 8.0);
        assert_ne!(refreshed["render_revision"], previous_revision);
        assert!(matches!(messages.recv().await, Some(Message::Binary(_))));
        controls.send(Message::Text(serde_json::json!({"type":"update","request_id":"jpeg","output":{"format":"jpeg","jpeg_quality":85,"fps":30}}).to_string().into())).await.unwrap();
        assert_eq!(control(&mut messages).await["revision"], 5);
        let jpeg = control(&mut messages).await;
        assert_eq!(jpeg["content_type"], "image/jpeg");
        let Some(Message::Binary(bytes)) = messages.recv().await else {
            panic!("expected JPEG")
        };
        assert_eq!(&bytes[..2], b"\xff\xd8");
        assert_eq!(bytes.len() as u64, jpeg["bytes"].as_u64().unwrap());
        s.catalog
            .unpublish(&key, &Precondition::Version("v3".into()))
            .unwrap();
        assert_eq!(control(&mut messages).await["code"], "resource-unpublished");
        assert!(matches!(messages.recv().await, Some(Message::Close(_))));
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        // A transport stuck on hello is dropped within the configured send
        // deadline. Dropping the sink releases everything retained by the write.
        let mut replacement = s.catalog.get(&key).unwrap();
        let version = replacement.version.clone();
        replacement.version = "v4".into();
        s.catalog
            .publish(replacement, &Precondition::Version(version))
            .unwrap();
        let pin = s.catalog.pin(&key).unwrap();
        let tx = Box::pin(futures_util::sink::unfold(pin, |pin, _| async move {
            std::future::pending::<()>().await;
            Ok::<_, ()>(pin)
        }));
        let rx = futures_util::stream::pending::<std::result::Result<Message, ()>>();
        tokio::time::timeout(
            Duration::from_secs(5),
            run_session(s.clone(), "test:ws".into(), tx, rx),
        )
        .await
        .unwrap();
        assert_eq!(s.catalog.stats().pinned_versions, 0);
        s.shutdown().await.unwrap();
    }
}
