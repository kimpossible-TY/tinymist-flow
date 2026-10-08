use futures::{SinkExt, StreamExt};
use reflexo_typst::debug_loc::DocumentPosition;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tinymist_std::error::IgnoreLogging;
use tokio::sync::{broadcast, mpsc};

use super::{editor::EditorActorRequest, render::RenderActorRequest};
use crate::focus::{FocusRequest, FocusStore};
use crate::{ViewerWindowStateMessage, WsMessage, actor::editor::DocToSrcJumpResolveRequest};

type FocusConnection = (
    FocusStoreHandle,
    Arc<parking_lot::RwLock<Option<Arc<dyn crate::CompileView>>>>,
);
type FocusStoreHandle = Arc<FocusStore>;
static NEXT_VIEWER_ID: AtomicU64 = AtomicU64::new(1);

// pub type CursorPosition = DocumentPosition;
pub type SrcToDocJumpInfo = DocumentPosition;

#[derive(Debug, Clone)]
pub enum WebviewActorRequest {
    ViewportPosition(DocumentPosition),
    SrcToDocJump(Vec<SrcToDocJumpInfo>),
    // CursorPosition(CursorPosition),
}

fn position_req(
    event: &'static str,
    DocumentPosition { page_no, x, y }: DocumentPosition,
) -> String {
    format!("{event},{page_no} {x} {y}")
}

fn positions_req(event: &'static str, positions: Vec<DocumentPosition>) -> String {
    format!("{event},")
        + &positions
            .iter()
            .map(|DocumentPosition { page_no, x, y }| format!("{page_no} {x} {y}"))
            .collect::<Vec<_>>()
            .join(",")
}

pub struct WebviewActor<'a, C> {
    webview_websocket_conn: std::pin::Pin<&'a mut C>,
    svg_receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    mailbox: broadcast::Receiver<WebviewActorRequest>,

    broadcast_sender: broadcast::Sender<WebviewActorRequest>,
    editor_sender: mpsc::UnboundedSender<EditorActorRequest>,
    render_sender: broadcast::Sender<RenderActorRequest>,
    focus: Option<FocusConnection>,
    viewer_id: u64,
}

pub struct Channels {
    pub svg: (
        mpsc::UnboundedSender<Vec<u8>>,
        mpsc::UnboundedReceiver<Vec<u8>>,
    ),
}

impl<'a, C> WebviewActor<'a, C>
where
    C: futures::Sink<WsMessage, Error = reflexo_typst::Error>
        + futures::Stream<Item = Result<WsMessage, reflexo_typst::Error>>,
{
    pub fn set_up_channels() -> Channels {
        Channels {
            svg: mpsc::unbounded_channel(),
        }
    }
    pub fn new(
        websocket_conn: std::pin::Pin<&'a mut C>,
        svg_receiver: mpsc::UnboundedReceiver<Vec<u8>>,
        broadcast_sender: broadcast::Sender<WebviewActorRequest>,
        mailbox: broadcast::Receiver<WebviewActorRequest>,
        editor_sender: mpsc::UnboundedSender<EditorActorRequest>,
        render_sender: broadcast::Sender<RenderActorRequest>,
        focus: Option<FocusConnection>,
    ) -> Self {
        Self {
            webview_websocket_conn: websocket_conn,
            svg_receiver,
            mailbox,
            broadcast_sender,
            editor_sender,
            render_sender,
            focus,
            viewer_id: NEXT_VIEWER_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    async fn record_focus(&mut self, payload: &str) {
        let Some((store, view)) = self.focus.as_ref() else {
            return;
        };
        let request = if payload.len() <= 48 * 1024 {
            serde_json::from_str::<FocusRequest>(payload)
                .ok()
                .filter(FocusRequest::is_valid)
        } else {
            None
        };
        let response = if let Some(request) = request {
            let store = store.clone();
            let view = view.read().clone();
            let viewer_id = self.viewer_id;
            match tokio::task::spawn_blocking(move || store.record(request, view, viewer_id)).await
            {
                Ok(Ok(snapshot)) => serde_json::json!({
                    "status": snapshot.status,
                    "sequence": snapshot.sequence,
                    "page": snapshot.position.map(|p| p.page_no),
                    "filepath": snapshot.source.as_ref().map(|s| &s.filepath),
                    "line": snapshot.source.as_ref().map(|s| s.line),
                    "text_selected": snapshot.selection.is_some(),
                }),
                result => {
                    log::warn!("could not persist preview focus: {result:?}");
                    serde_json::json!({"status": "error"})
                }
            }
        } else {
            serde_json::json!({"status": "invalid"})
        };
        self.webview_websocket_conn
            .send(WsMessage::Binary(format!("focus,{response}").into()))
            .await
            .log_error("SendPreviewFocus");
    }

    async fn highlight_selection(&mut self, payload: &str) {
        let request = (payload.len() <= 48 * 1024)
            .then(|| serde_json::from_str::<FocusRequest>(payload).ok())
            .flatten()
            .filter(|request| request.is_valid() && request.selection.is_some());
        let response = match (self.focus.as_ref(), request) {
            (Some((_, view)), Some(request)) => {
                let view = view.read().clone();
                match view {
                    Some(view) if request.revision.as_ref() == Some(&view.revision()) => {
                        match tokio::task::spawn_blocking(move || {
                            view.highlight_preview_selection(
                                &request.position,
                                &request.selection.unwrap(),
                            )
                        })
                        .await
                        {
                            Ok(status) => serde_json::json!({"status": status}),
                            Err(error) => {
                                log::warn!("could not highlight preview selection: {error}");
                                serde_json::json!({"status": "error"})
                            }
                        }
                    }
                    _ => serde_json::json!({"status": "stale"}),
                }
            }
            (None, _) => serde_json::json!({"status": "unsupported"}),
            _ => serde_json::json!({"status": "invalid"}),
        };
        self.webview_websocket_conn
            .send(WsMessage::Binary(format!("highlight,{response}").into()))
            .await
            .log_error("SendPreviewHighlight");
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                Ok(msg) = self.mailbox.recv() => {
                    log::trace!("WebviewActor: received message from mailbox: {msg:?}");
                    match msg {
                        WebviewActorRequest::SrcToDocJump(jump_info) => {
                            let msg = positions_req("jump", jump_info);
                            self.webview_websocket_conn.send(WsMessage::Binary(msg.into()))
                              .await.log_error("WebViewActor");
                        }
                        WebviewActorRequest::ViewportPosition(jump_info) => {
                            let msg = position_req("viewport", jump_info);
                            self.webview_websocket_conn.send(WsMessage::Binary(msg.into()))
                              .await.log_error("WebViewActor");
                        }
                    }
                }
                Some(svg) = self.svg_receiver.recv() => {
                    log::trace!("WebviewActor: received svg from renderer");
                    let _scope = typst_timing::TimingScope::new("webview_actor_send_svg");
                    self.webview_websocket_conn.send(WsMessage::Binary(svg.into()))
                    .await.log_error("WebViewActor");
                }
                msg = self.webview_websocket_conn.next() => {
                    let Some(msg) = msg else {
                        break;
                    };
                    log::trace!("WebviewActor: received message from websocket: {msg:?}");
                    let Ok(msg) = msg else {
                        log::info!("WebviewActor: no more messages from websocket: {}", msg.unwrap_err());
                      break;
                    };
                    let msg = match msg {
                        WsMessage::Text(msg) => msg,
                        WsMessage::Ping(msg) => {
                            let _ = self.webview_websocket_conn.send(WsMessage::Pong(msg)).await;
                            continue;
                        },
                        WsMessage::Pong(..) => {
                            continue;
                        },
                        _ =>  {
                            log::info!("WebviewActor: received non-text message from websocket: {msg:?}");
                            let _ = self.webview_websocket_conn.send(WsMessage::Text(format!("Webview Actor: error, received non-text message: {msg:?}")))
                            .await;
                            break;
                        }
                    };
                    if msg == "current" {
                        self.render_sender.send(RenderActorRequest::RenderFullLatest).log_error("WebViewActor");
                    } else if msg.starts_with("srclocation") {
                        let location = msg.split(' ').nth(1).unwrap();
                        self.editor_sender.send(EditorActorRequest::DocToSrcJumpResolve(
                            DocToSrcJumpResolveRequest {
                                span: location.trim().to_owned(),
                            },
                        )).log_error("WebViewActor");
                    } else if msg.starts_with("outline-sync") {
                        let location = msg.split(',').nth(1).unwrap();
                        let location = location.split(' ').collect::<Vec::<&str>>();
                        let page_no = location[0].parse().unwrap();
                        let x = location.get(1).map(|s| s.parse().unwrap()).unwrap_or(0.);
                        let y = location.get(2).map(|s| s.parse().unwrap()).unwrap_or(0.);
                        let pos = DocumentPosition { page_no, x, y };

                        self.broadcast_sender.send(WebviewActorRequest::ViewportPosition(pos)).log_error("WebViewActor");
                    } else if let Some(path) = msg.strip_prefix("src-point ") {
                        if self.focus.is_some() {
                            self.record_focus(path).await;
                        } else if let Ok(path) = serde_json::from_str(path) {
                            self.render_sender.send(RenderActorRequest::WebviewResolveFrameLoc(path)).log_error("WebViewActor");
                        }
                    } else if let Some(selection) = msg.strip_prefix("src-highlight ") {
                        self.highlight_selection(selection).await;
                    } else if let Some(state) = msg.strip_prefix("viewer-window-state ") {
                        if let Ok(state) = serde_json::from_str::<ViewerWindowStateMessage>(state) {
                            self.editor_sender.send(EditorActorRequest::ViewerWindowState(state)).log_error("WebViewActor");
                        };
                    } else {
                        let err = self.webview_websocket_conn.send(WsMessage::Text(format!("error, received unknown message: {msg}"))).await;
                        log::info!("WebviewActor: received unknown message from websocket: {msg} {err:?}");
                        break;
                    }
                }
                else => {
                    break;
                }
            }
        }
        log::info!("WebviewActor: exiting");
    }
}
