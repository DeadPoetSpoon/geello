use axum::extract::{
    Query, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::response::IntoResponse;
use axum::{Router, routing::get};
use serde::Deserialize;

use crate::server::error::ServerError;
use crate::server::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/ws/anim", get(anim_upgrade))
}

#[derive(Debug, Deserialize)]
pub struct WsAnimParams {
    pub layers: String,
    pub styles: String,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_format")]
    pub format: String,
    pub bbox: Option<String>,
}

fn default_format() -> String {
    "png".to_string()
}

/// GET /ws/anim (WebSocket upgrade)
pub async fn anim_upgrade(
    State(state): State<AppState>,
    Query(params): Query<WsAnimParams>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state, params))
}

async fn handle_ws(mut socket: WebSocket, state: AppState, params: WsAnimParams) {
    let start = std::time::Instant::now();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(t))) if t == "exit" => break,
                    Some(Ok(_)) => {
                        let elapsed = start.elapsed().as_secs() % 10;
                        // Periodically send frame (simplified: sends placeholder)
                        if let Ok(frame) = render_anim_frame(&state, &params, elapsed).await {
                            let _ = socket.send(Message::Binary(frame.into())).await;
                        }
                    }
                    Some(Err(_)) => break,
                    None => break,
                }
            }
        }
    }
}

async fn render_anim_frame(
    state: &AppState,
    params: &WsAnimParams,
    _frame: u64,
) -> Result<Vec<u8>, ServerError> {
    let geojson_data = super::wmts::load_layers_impl(state, &params.layers).await?;
    let mut option = super::wmts::load_style_impl(state, &params.styles).await?;
    option.pixel_option.width = params.width;
    option.pixel_option.height = params.height;
    option.region = super::wms::parse_bbox_impl(&params.bbox);
    super::wms::render_wms_impl(state, &geojson_data, &mut option, &params.format).await
}
