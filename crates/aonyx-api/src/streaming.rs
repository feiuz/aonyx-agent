//! Streaming turn endpoints: a bidirectional **WebSocket** and a one-shot
//! **SSE** stream. Both share [`drive_turn`], which runs the injected
//! [`ApiAgent`](crate::ApiAgent) streaming, persists the new log, and emits
//! the terminal `Done`/`Error` frame.

use std::convert::Infallible;

use aonyx_core::{Message, Role};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::agent::{last_assistant_text, StreamFrame};
use crate::error::ApiError;
use crate::sessions::SendMessageRequest;
use crate::state::ApiState;

/// Client → server WebSocket message.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    /// Send a user turn.
    User {
        /// The user message text.
        content: String,
    },
    /// Request cancellation of the running turn (best-effort).
    Cancel,
}

/// `GET /v1/sessions/:id/stream` — upgrade to a bidirectional WebSocket.
pub async fn ws_stream(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, id))
}

async fn handle_socket(mut socket: WebSocket, state: ApiState, id: Uuid) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            WsMessage::Text(t) => t,
            WsMessage::Close(_) => break,
            _ => continue,
        };
        let content = match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::User { content }) => content,
            Ok(ClientMsg::Cancel) => continue,
            Err(e) => {
                let frame = StreamFrame::Error {
                    message: format!("bad client message: {e}"),
                };
                if send_frame(&mut socket, &frame).await.is_err() {
                    break;
                }
                continue;
            }
        };
        if content.trim().is_empty() {
            let frame = StreamFrame::Error {
                message: "empty content".into(),
            };
            if send_frame(&mut socket, &frame).await.is_err() {
                break;
            }
            continue;
        }
        if run_turn_to_socket(&mut socket, &state, id, content)
            .await
            .is_err()
        {
            break; // socket closed mid-turn
        }
    }
}

async fn run_turn_to_socket(
    socket: &mut WebSocket,
    state: &ApiState,
    id: Uuid,
    content: String,
) -> Result<(), ()> {
    let record = match state.sessions.get(id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            let frame = StreamFrame::Error {
                message: format!("no session {id}"),
            };
            return send_frame(socket, &frame).await;
        }
        Err(e) => {
            let frame = StreamFrame::Error {
                message: e.to_string(),
            };
            return send_frame(socket, &frame).await;
        }
    };

    let mut history = record.messages;
    history.push(Message::new(Role::User, content));
    let turns = record.turns + 1;

    let (tx, mut rx) = mpsc::channel::<StreamFrame>(128);
    let st = state.clone();
    let task = tokio::spawn(async move { drive_turn(st, id, history, turns, tx).await });

    while let Some(frame) = rx.recv().await {
        if send_frame(socket, &frame).await.is_err() {
            task.abort();
            return Err(());
        }
    }
    let _ = task.await;
    Ok(())
}

/// Shared turn driver for WS + SSE: run the injected agent streaming, persist
/// the new log, then emit the terminal `Done` (or `Error`). Owns `tx`, so the
/// receiver stream ends when this returns.
async fn drive_turn(
    state: ApiState,
    id: Uuid,
    history: Vec<Message>,
    turns: u32,
    tx: mpsc::Sender<StreamFrame>,
) {
    match state.agent.run_turn_streaming(history, tx.clone()).await {
        Ok(log) => {
            let reply = last_assistant_text(&log);
            if let Err(e) = state.sessions.update(id, log, turns).await {
                let _ = tx
                    .send(StreamFrame::Error {
                        message: e.to_string(),
                    })
                    .await;
                return;
            }
            let _ = tx.send(StreamFrame::Done { reply, turns }).await;
        }
        Err(e) => {
            let _ = tx
                .send(StreamFrame::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

async fn send_frame(socket: &mut WebSocket, frame: &StreamFrame) -> Result<(), ()> {
    let json = serde_json::to_string(frame).unwrap_or_default();
    socket.send(WsMessage::Text(json)).await.map_err(|_| ())
}

/// `POST /v1/sessions/:id/messages/stream` — run one turn, streaming frames
/// as Server-Sent Events. Validation/lookup errors are returned as normal
/// HTTP errors before the stream starts.
pub async fn sse_message(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Response {
    if req.content.trim().is_empty() {
        return ApiError::BadRequest("empty message content".into()).into_response();
    }
    let record = match state.sessions.get(id).await {
        Ok(Some(r)) => r,
        Ok(None) => return ApiError::NotFound(format!("no session {id}")).into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };

    let mut history = record.messages;
    history.push(req.into_message());
    let turns = record.turns + 1;

    let (tx, rx) = mpsc::channel::<StreamFrame>(128);
    let st = state.clone();
    tokio::spawn(async move { drive_turn(st, id, history, turns, tx).await });

    let stream = ReceiverStream::new(rx).map(|frame| {
        let ev = Event::default()
            .json_data(&frame)
            .unwrap_or_else(|_| Event::default().data("{}"));
        Ok::<Event, Infallible>(ev)
    });
    Sse::new(stream).into_response()
}
