// todo: remove me
#![allow(missing_docs)]

mod actor;
mod change;
mod debug_loc;
mod demand;
mod focus;
mod outline;
pub mod protocol;

pub use crate::actor::editor::{
    CompileStatus, ControlPlaneMessage, ControlPlaneResponse, ControlPlaneRx, ControlPlaneTx,
    PanelScrollByPositionRequest,
};
pub use crate::focus::{PreviewHighlightStatus, PreviewSourceContext, PreviewTextSelection};
pub use crate::outline::Outline;

use std::sync::{Arc, OnceLock};
use std::{borrow::Cow, collections::HashMap, future::Future, path::PathBuf, pin::Pin};

use bytes::Bytes;
use futures::sink::SinkExt;
use reflexo_typst::Error;
use reflexo_typst::args::TaskWhen;
use reflexo_typst::debug_loc::{DocumentPosition, SourceSpanOffset};
use serde::{Deserialize, Serialize};
use tinymist_std::error::IgnoreLogging;
use tinymist_std::typst::TypstDocument;
use tinymist_task::ExportTarget;
use tokio::sync::{broadcast, mpsc};
use typst::{introspection::PagedPosition, syntax::Span};

use crate::actor::editor::{EditorActor, EditorActorRequest};
use crate::actor::html::HtmlRenderActor;
use crate::actor::render::RenderActorRequest;
use crate::actor::webview::WebviewActorRequest;
use crate::debug_loc::SpanInterner;

type StopFuture = Pin<Box<dyn Future<Output = ()> + Send + Sync>>;

/// Configure the preview mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum PreviewMode {
    /// Would like to preview a regular document.
    #[cfg_attr(feature = "clap", clap(name = "document"))]
    Document,

    /// Would like to preview slides.
    #[cfg_attr(feature = "clap", clap(name = "slide"))]
    Slide,
}

/// Configure the preview service.
#[derive(Debug, Clone, Default)]
pub struct PreviewConfig {
    /// Configure the preview output format.
    pub format: ExportTarget,
    /// Enable partial rendering.
    pub enable_partial_rendering: bool,
    /// Configure the refresh style of the preview.
    pub refresh_style: TaskWhen,
    /// The invert colors setting for the preview.
    pub invert_colors: String,
}

/// Gets the HTML for the frontend by a given preview mode and server to connect
pub fn frontend_html(html: &str, mode: PreviewMode, to: &str, page_title: &str) -> String {
    let mode = match mode {
        PreviewMode::Document => "Doc",
        PreviewMode::Slide => "Slide",
    };
    let page_title = escape_html_text(page_title);

    html.replace("ws://127.0.0.1:23625", to)
        .replace(
            "preview-arg:previewMode:Doc",
            format!("preview-arg:previewMode:{mode}").as_str(),
        )
        .replace("preview-arg:pageTitle:", &page_title)
}

// TODO: Drop this local copy after upstreaming the fix to reflexo's vendored
// XML escape helper, which still predates
// https://github.com/netvl/xml-rs/commit/59d629458f596611f6627357e522af4d7bcba13e.
fn escape_html_text(value: &str) -> Cow<'_, str> {
    let mut escaped = String::new();
    let mut last = 0;

    for (idx, ch) in value.char_indices() {
        let replacement = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => continue,
        };

        if escaped.is_empty() {
            escaped.reserve(value.len() + 16);
        }

        escaped.push_str(&value[last..idx]);
        escaped.push_str(replacement);
        last = idx + ch.len_utf8();
    }

    if escaped.is_empty() {
        Cow::Borrowed(value)
    } else {
        escaped.push_str(&value[last..]);
        Cow::Owned(escaped)
    }
}

/// Simply creates a previewer.
pub async fn preview(
    config: PreviewConfig,
    conn: ControlPlaneTx,
    server: Arc<impl EditorServer>,
) -> Previewer {
    PreviewBuilder::new(config).build(conn, server).await
}

/// The previewer service.
pub struct Previewer {
    stop: Option<Box<dyn FnOnce() -> StopFuture + Send + Sync>>,
    data_plane_handle: Option<tokio::task::JoinHandle<()>>,
    data_plane_resources: Option<(DataPlane, Option<mpsc::Sender<()>>, mpsc::Receiver<()>)>,
    control_plane_handle: tokio::task::JoinHandle<()>,
}

impl Previewer {
    /// Sends stop requests to preview actors.
    pub async fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop().await;
        }
    }

    /// Joins all the previewer actors.
    ///
    /// Note: send stop request first.
    pub async fn join(mut self) {
        let data_plane_handle = self.data_plane_handle.take().expect("must bind data plane");
        let _ = tokio::join!(data_plane_handle, self.control_plane_handle);
    }

    /// Listens streams that accepting data plane messages.
    pub fn start_data_plane<
        C: futures::Sink<WsMessage, Error = reflexo_typst::Error>
            + futures::Stream<Item = Result<WsMessage, reflexo_typst::Error>>
            + Send
            + 'static,
        S: 'static,
        SFut: Future<Output = S> + Send + 'static,
    >(
        &mut self,
        mut streams: mpsc::UnboundedReceiver<SFut>,
        caster: impl Fn(S) -> Result<C, Error> + Send + Sync + Copy + 'static,
    ) {
        let idle_timeout = reflexo_typst::time::Duration::from_secs(5);
        let (conn_handler, shutdown_tx, mut shutdown_data_plane_rx) =
            self.data_plane_resources.take().unwrap();
        let recv = move |conn| {
            let h = conn_handler.clone();
            async move {
                let Some(conn) = caster(conn.await).log_error("AcceptPreviewConnection") else {
                    return;
                };
                let _viewer = h.viewer_demand.as_ref().map(|demand| demand.connect());
                // Renderers retain document snapshots and incremental rendering
                // caches. Tie their lifetime to the socket that consumes them.
                let mut render_tasks = tokio::task::JoinSet::new();
                tokio::pin!(conn);

                if h.enable_partial_rendering
                    && conn
                        .send(WsMessage::Binary("partial-rendering,true".into()))
                        .await
                        .log_error("SendPartialRendering")
                        .is_none()
                {
                    return;
                }
                if h.focus_store.is_some()
                    && conn
                        .send(WsMessage::Binary("focus-enabled,true".into()))
                        .await
                        .log_error("SendFocusEnabled")
                        .is_none()
                {
                    return;
                }
                if !h.invert_colors.is_empty()
                    && conn
                        .send(WsMessage::Binary(
                            format!("invert-colors,{}", h.invert_colors).into(),
                        ))
                        .await
                        .log_error("SendInvertColor")
                        .is_none()
                {
                    return;
                }
                let actor::webview::Channels { svg } =
                    actor::webview::WebviewActor::<'_, C>::set_up_channels();
                let webview_actor = actor::webview::WebviewActor::new(
                    conn,
                    svg.1,
                    h.webview_tx.clone(),
                    (h.webview_tx.subscribe(), h.status_rx.clone()),
                    h.editor_tx.clone(),
                    h.renderer_tx.clone(),
                    h.focus_store
                        .clone()
                        .map(|store| (store, h.doc_sender.clone())),
                );
                match h.format {
                    ExportTarget::Paged => {
                        let render_actor = actor::render::RenderActor::new(
                            h.renderer_tx.subscribe(),
                            h.doc_sender.clone(),
                            h.editor_tx.clone(),
                            svg.0,
                            h.webview_tx,
                            h.focus_store.is_some(),
                            h.change_tracker.clone(),
                        );
                        render_tasks.spawn(render_actor.run());
                        let outline_render_actor = actor::render::OutlineRenderActor::new(
                            h.renderer_tx.subscribe(),
                            h.doc_sender.clone(),
                            h.editor_tx.clone(),
                            h.span_interner,
                        );
                        render_tasks.spawn(outline_render_actor.run());
                    }
                    ExportTarget::Html => {
                        let html_render_actor = HtmlRenderActor::new(
                            h.renderer_tx.subscribe(),
                            h.doc_sender.clone(),
                            svg.0,
                        );
                        render_tasks.spawn(html_render_actor.run());
                    }
                    ExportTarget::Bundle => {
                        log::warn!("bundle export target is not supported by preview");
                    }
                }

                webview_actor.run().await;
                render_tasks.shutdown().await;
            }
        };

        let data_plane_handle = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            let mut shutdown_bell = tokio::time::interval(idle_timeout);
            loop {
                if shutdown_tx.is_some() {
                    shutdown_bell.reset();
                }
                tokio::select! {
                    Some(()) = shutdown_data_plane_rx.recv() => {
                        log::info!("Data plane server shutdown");
                        connections.shutdown().await;
                        return;
                    }
                    Some(stream) = streams.recv() => {
                        connections.spawn(recv(stream));
                    },
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        if let Err(err) = result {
                            log::warn!("Preview connection task failed: {err}");
                        }
                    }
                    _ = shutdown_bell.tick(), if connections.is_empty() && shutdown_tx.is_some() => {
                        let shutdown_tx = shutdown_tx.expect("scheduled shutdown without shutdown_tx");
                        log::info!(
                            "Data plane server has been idle for {idle_timeout:?}, shutting down."
                        );
                        let _ = shutdown_tx.send(()).await;
                        log::info!("Data plane server shutdown");
                        return;
                    }
                }
            }
        });

        self.data_plane_handle = Some(data_plane_handle);
    }
}

type MpScChannel<T> = (mpsc::UnboundedSender<T>, mpsc::UnboundedReceiver<T>);
type BroadcastChannel<T> = (broadcast::Sender<T>, broadcast::Receiver<T>);

pub struct PreviewBuilder {
    config: PreviewConfig,
    viewer_demand: Option<Arc<demand::ViewerDemand>>,
    focus_store: Option<Arc<focus::FocusStore>>,
    change_tracker: Option<Arc<change::ChangeTracker>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    renderer_mailbox: BroadcastChannel<RenderActorRequest>,
    editor_conn: MpScChannel<EditorActorRequest>,
    webview_conn: BroadcastChannel<WebviewActorRequest>,
    compile_status: (
        tokio::sync::watch::Sender<Option<CompileStatus>>,
        tokio::sync::watch::Receiver<Option<CompileStatus>>,
    ),
    doc_sender: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,

    compile_watcher: OnceLock<Arc<CompileWatcher>>,
}

impl PreviewBuilder {
    pub fn new(config: PreviewConfig) -> Self {
        Self {
            config,
            viewer_demand: None,
            focus_store: None,
            change_tracker: None,
            shutdown_tx: None,
            renderer_mailbox: broadcast::channel(1024),
            editor_conn: mpsc::unbounded_channel(),
            webview_conn: broadcast::channel(32),
            compile_status: tokio::sync::watch::channel(None),
            doc_sender: Arc::new(parking_lot::RwLock::new(None)),
            compile_watcher: OnceLock::new(),
        }
    }

    pub fn with_shutdown_tx(mut self, shutdown_tx: mpsc::Sender<()>) -> Self {
        self.shutdown_tx = Some(shutdown_tx);
        self
    }

    /// Notify the compiler when the first viewer connects or the last one leaves.
    /// The callback must return promptly; it is serialized across connections.
    /// Sibling pipelines have independent demand and need their own callback.
    pub fn with_viewer_demand(mut self, changed: impl Fn(bool) + Send + Sync + 'static) -> Self {
        self.viewer_demand = Some(demand::ViewerDemand::new(changed));
        self
    }

    /// Persist explicit preview selections for a local assistant to read.
    pub fn with_focus_file(mut self, path: PathBuf) -> std::io::Result<Self> {
        self.focus_store = Some(Arc::new(focus::FocusStore::new(path)?));
        Ok(self)
    }

    /// Remember visual edits for new viewers and across service restarts.
    pub fn with_change_file(
        mut self,
        path: PathBuf,
        project: String,
        variant: &str,
    ) -> std::io::Result<Self> {
        self.change_tracker = Some(Arc::new(change::ChangeTracker::new(
            path, project, variant,
        )?));
        Ok(self)
    }

    /// Create an independent rendering pipeline sharing the ordered focus store.
    pub fn sibling(&self, config: PreviewConfig) -> Self {
        let mut sibling = Self::new(config);
        sibling.focus_store = self.focus_store.clone();
        sibling.change_tracker = self
            .change_tracker
            .as_ref()
            .map(|tracker| Arc::new(tracker.sibling("dark")));
        sibling
    }

    pub fn compile_watcher(&self, task_id: String) -> &Arc<CompileWatcher> {
        self.compile_watcher.get_or_init(|| {
            Arc::new(CompileWatcher {
                task_id,
                when: self.config.refresh_style.clone(),
                doc_sender: self.doc_sender.clone(),
                editor_tx: self.editor_conn.0.clone(),
                render_tx: self.renderer_mailbox.0.clone(),
                status_tx: self.compile_status.0.clone(),
                change_tracker: self.change_tracker.clone(),
            })
        })
    }

    pub async fn build<T: EditorServer>(self, conn: ControlPlaneTx, server: Arc<T>) -> Previewer {
        let PreviewBuilder {
            config,
            viewer_demand,
            focus_store,
            change_tracker,
            shutdown_tx,
            renderer_mailbox,
            editor_conn: (editor_tx, editor_rx),
            webview_conn: (webview_tx, _),
            compile_status: (_, status_rx),
            doc_sender,
            ..
        } = self;

        // Shared resource
        let span_interner = SpanInterner::new();
        let (shutdown_data_plane_tx, shutdown_data_plane_rx) = mpsc::channel(1);

        // Spawns the editor actor
        let editor_actor = EditorActor::new(
            server,
            editor_rx,
            conn,
            renderer_mailbox.0.clone(),
            webview_tx.clone(),
            span_interner.clone(),
        );
        let control_plane_handle = tokio::spawn(editor_actor.run());
        log::info!("Previewer: editor actor spawned");

        // Delayed data plane binding
        let data_plane = DataPlane {
            format: config.format,
            viewer_demand,
            focus_store,
            change_tracker,
            span_interner: span_interner.clone(),
            webview_tx: webview_tx.clone(),
            editor_tx: editor_tx.clone(),
            invert_colors: config.invert_colors,
            renderer_tx: renderer_mailbox.0.clone(),
            enable_partial_rendering: config.enable_partial_rendering,
            doc_sender,
            status_rx,
        };

        Previewer {
            control_plane_handle,
            data_plane_handle: None,
            data_plane_resources: Some((data_plane, shutdown_tx, shutdown_data_plane_rx)),
            stop: Some(Box::new(move || {
                Box::pin(async move {
                    let _ = shutdown_data_plane_tx.send(()).await;
                    let _ = editor_tx.send(EditorActorRequest::Shutdown);
                })
            })),
        }
    }
}

#[derive(Debug)]
pub enum WsMessage {
    /// A text WebSocket message
    Text(String),
    /// A binary WebSocket message
    Binary(Bytes),
    /// A ping message
    Ping(Bytes),
    /// A pong message
    Pong(Bytes),
}

pub type SourceLocation = reflexo_typst::debug_loc::SourceLocation;

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Poll};

    use futures::{Sink, Stream};
    use reflexo_vec2svg::IncrSvgDocServer;
    use tinymist_std::error::prelude::*;
    use tinymist_std::typst::{TypstDocument, TypstPagedDocument};
    use tokio::sync::mpsc;

    use super::{WsMessage, escape_html_text, protocol};

    struct TestEditor;

    impl super::EditorServer for TestEditor {}

    #[derive(Debug)]
    struct TestSocket {
        incoming:
            futures::channel::mpsc::UnboundedReceiver<Result<WsMessage, reflexo_typst::Error>>,
        outgoing: Option<mpsc::UnboundedSender<WsMessage>>,
    }

    impl Stream for TestSocket {
        type Item = Result<WsMessage, reflexo_typst::Error>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.incoming).poll_next(cx)
        }
    }

    impl Sink<WsMessage> for TestSocket {
        type Error = reflexo_typst::Error;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, message: WsMessage) -> Result<(), Self::Error> {
            if let Some(outgoing) = &self.outgoing {
                let _ = outgoing.send(message);
            }
            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    struct FailingSocket {
        socket: TestSocket,
        sent: usize,
        fail_after: usize,
        failures: Arc<AtomicUsize>,
    }

    impl Stream for FailingSocket {
        type Item = Result<WsMessage, reflexo_typst::Error>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.socket).poll_next(cx)
        }
    }

    impl Sink<WsMessage> for FailingSocket {
        type Error = reflexo_typst::Error;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            if self.sent >= self.fail_after {
                self.failures.fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Err(error_once!("test disconnected socket")))
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn start_send(mut self: Pin<&mut Self>, message: WsMessage) -> Result<(), Self::Error> {
            self.sent += 1;
            Pin::new(&mut self.socket).start_send(message)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.socket).poll_flush(cx)
        }

        fn poll_close(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.socket).poll_close(cx)
        }
    }

    async fn demand_event(events: &mut mpsc::UnboundedReceiver<bool>) -> bool {
        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .expect("viewer demand should be updated")
            .expect("demand observer should remain alive")
    }

    async fn renderer_count(
        renderer: &tokio::sync::broadcast::Sender<super::RenderActorRequest>,
        expected: usize,
    ) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while renderer.receiver_count() != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("connection-owned renderers should be started or released");
    }

    #[tokio::test]
    async fn compiler_status_reaches_late_viewers_and_remains_pipeline_local() {
        let builder = super::PreviewBuilder::new(super::PreviewConfig::default());
        let sibling = builder.sibling(super::PreviewConfig::default());
        let watcher = builder.compile_watcher("status-test".into()).clone();
        watcher.status(super::CompileStatus::CompileError);
        assert!(sibling.compile_status.1.borrow().is_none());
        let (control, _control_rx) = super::ControlPlaneTx::new(false);
        let mut previewer = builder.build(control, Arc::new(TestEditor)).await;
        let (streams_tx, streams_rx) = mpsc::unbounded_channel();
        previewer.start_data_plane(streams_rx, |socket| socket);
        let (incoming_tx, incoming) = futures::channel::mpsc::unbounded();
        let (outgoing, mut messages) = mpsc::unbounded_channel();
        streams_tx
            .send(futures::future::ready(Ok(TestSocket {
                incoming,
                outgoing: Some(outgoing),
            })))
            .unwrap();
        for expected in ["error", "compiling", "success"] {
            if expected == "compiling" {
                watcher.status(super::CompileStatus::Compiling);
            } else if expected == "success" {
                watcher.status(super::CompileStatus::CompileSuccess);
            }
            let message = tokio::time::timeout(std::time::Duration::from_secs(2), messages.recv())
                .await
                .unwrap()
                .unwrap();
            let WsMessage::Binary(bytes) = message else {
                panic!("expected a binary status")
            };
            assert_eq!(
                bytes.as_ref(),
                format!("compile-status,{expected}").as_bytes()
            );
        }
        drop(incoming_tx);
        previewer.stop().await;
        previewer.join().await;
    }

    #[tokio::test]
    async fn send_failure_releases_connection_without_polling_failed_sink_again() {
        for (fail_after, trigger) in [
            (0, "startup"),
            (1, "startup"),
            (2, "startup"),
            (3, "compile"),
            (3, "ping"),
            (3, "highlight"),
            (3, "viewport"),
        ] {
            let (demand_tx, mut demand_rx) = mpsc::unbounded_channel();
            let builder = super::PreviewBuilder::new(super::PreviewConfig {
                enable_partial_rendering: true,
                invert_colors: "auto".into(),
                ..super::PreviewConfig::default()
            })
            .with_viewer_demand(move |active| {
                demand_tx.send(active).unwrap();
            });
            let watcher = builder.compile_watcher("failed-send-test".into()).clone();
            watcher.status(super::CompileStatus::CompileError);
            let renderer = builder.renderer_mailbox.0.clone();
            let webview = builder.webview_conn.0.clone();
            let (control, _control_rx) = super::ControlPlaneTx::new(false);
            let mut previewer = builder.build(control, Arc::new(TestEditor)).await;
            let (streams_tx, streams_rx) = mpsc::unbounded_channel();
            previewer.start_data_plane(streams_rx, |socket| socket);
            let (incoming_tx, incoming) = futures::channel::mpsc::unbounded();
            let (outgoing, mut messages) = mpsc::unbounded_channel();
            let failures = Arc::new(AtomicUsize::new(0));
            streams_tx
                .send(futures::future::ready(Ok(FailingSocket {
                    socket: TestSocket {
                        incoming,
                        outgoing: Some(outgoing),
                    },
                    sent: 0,
                    fail_after,
                    failures: failures.clone(),
                })))
                .unwrap();
            assert!(demand_event(&mut demand_rx).await);
            if fail_after == 3 {
                for _ in 0..3 {
                    tokio::time::timeout(std::time::Duration::from_secs(2), messages.recv())
                        .await
                        .unwrap()
                        .unwrap();
                }
                match trigger {
                    "compile" => watcher.status(super::CompileStatus::Compiling),
                    "ping" => incoming_tx
                        .unbounded_send(Ok(WsMessage::Ping(vec![].into())))
                        .unwrap(),
                    "highlight" => incoming_tx
                        .unbounded_send(Ok(WsMessage::Text("src-highlight {}".into())))
                        .unwrap(),
                    "viewport" => {
                        webview
                            .send(
                                super::actor::webview::WebviewActorRequest::ViewportPosition(
                                    super::DocumentPosition {
                                        page_no: 1,
                                        x: 0.,
                                        y: 0.,
                                    },
                                ),
                            )
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            assert!(!demand_event(&mut demand_rx).await);
            renderer_count(&renderer, 0).await;
            assert_eq!(failures.load(Ordering::SeqCst), 1, "{trigger}");
            previewer.stop().await;
            previewer.join().await;
        }
    }

    #[tokio::test]
    async fn demand_tracks_live_viewers_and_releases_renderers_on_disconnect() {
        let (demand_tx, mut demand_rx) = mpsc::unbounded_channel();
        let builder = super::PreviewBuilder::new(super::PreviewConfig::default())
            .with_viewer_demand(move |active| {
                demand_tx.send(active).unwrap();
            });
        // Theme siblings must never activate each other's compiler.
        assert!(
            builder
                .sibling(super::PreviewConfig::default())
                .viewer_demand
                .is_none()
        );
        let renderer = builder.renderer_mailbox.0.clone();
        // The library test does not own the process. Standalone control mode
        // deliberately exits the entire process when its editor actor stops.
        let (control, _control_rx) = super::ControlPlaneTx::new(false);
        let mut previewer = builder.build(control, Arc::new(TestEditor)).await;
        let (streams_tx, streams_rx) = mpsc::unbounded_channel();
        previewer.start_data_plane(streams_rx, |socket| socket);
        assert!(demand_rx.try_recv().is_err());

        // A failed HTTP upgrade must never request a compilation or leak a viewer.
        streams_tx
            .send(futures::future::ready(Err(error_once!(
                "test upgrade failure"
            ))))
            .unwrap();
        let (first, incoming) = futures::channel::mpsc::unbounded();
        streams_tx
            .send(futures::future::ready(Ok(TestSocket {
                incoming,
                outgoing: None,
            })))
            .unwrap();
        assert!(demand_event(&mut demand_rx).await);
        renderer_count(&renderer, 2).await;

        let (second, incoming) = futures::channel::mpsc::unbounded();
        streams_tx
            .send(futures::future::ready(Ok(TestSocket {
                incoming,
                outgoing: None,
            })))
            .unwrap();
        renderer_count(&renderer, 4).await;
        assert!(demand_rx.try_recv().is_err());

        // EOF must release this connection even while broadcast channels stay open.
        drop(first);
        renderer_count(&renderer, 2).await;
        assert!(demand_rx.try_recv().is_err());
        drop(second);
        assert!(!demand_event(&mut demand_rx).await);
        renderer_count(&renderer, 0).await;

        let (reconnected, incoming) = futures::channel::mpsc::unbounded();
        streams_tx
            .send(futures::future::ready(Ok(TestSocket {
                incoming,
                outgoing: None,
            })))
            .unwrap();
        assert!(demand_event(&mut demand_rx).await);
        reconnected
            .unbounded_send(Ok(WsMessage::Text("invalid viewer message".into())))
            .unwrap();
        assert!(!demand_event(&mut demand_rx).await);
        renderer_count(&renderer, 0).await;

        // Service shutdown also releases demand for sockets that are still open.
        let (_last, incoming) = futures::channel::mpsc::unbounded();
        streams_tx
            .send(futures::future::ready(Ok(TestSocket {
                incoming,
                outgoing: None,
            })))
            .unwrap();
        assert!(demand_event(&mut demand_rx).await);
        previewer.stop().await;
        previewer.join().await;
        assert!(!demand_event(&mut demand_rx).await);
        renderer_count(&renderer, 0).await;
    }

    #[test]
    fn escapes_html_text_without_breaking_multibyte_code_points() {
        assert_eq!(escape_html_text("☃<>&"), "☃&lt;&gt;&amp;");
        assert_eq!(escape_html_text("plain text"), "plain text");
    }

    #[test]
    fn sibling_shares_focus_store_but_not_compile_state() {
        let path = std::env::temp_dir().join(format!(
            "tinymist-sibling-focus-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let builder = super::PreviewBuilder::new(super::PreviewConfig::default())
            .with_focus_file(path.clone())
            .unwrap();
        let sibling = builder.sibling(super::PreviewConfig::default());
        assert!(Arc::ptr_eq(
            builder.focus_store.as_ref().unwrap(),
            sibling.focus_store.as_ref().unwrap(),
        ));
        assert!(!Arc::ptr_eq(&builder.doc_sender, &sibling.doc_sender));
        assert!(!Arc::ptr_eq(
            builder.compile_watcher("light".into()),
            sibling.compile_watcher("dark".into()),
        ));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn full_current_event_uses_new_prefix_after_incremental_render() {
        tinymist_tests::run_with_sources(
            "#set page(width: 1pt, height: 1pt, margin: 0pt)",
            |verse, _| {
                let world = verse.snapshot();
                let doc = typst::compile::<TypstPagedDocument>(&world)
                    .output
                    .expect("short preview fixture should compile");
                let document = TypstDocument::Paged(Arc::new(doc));
                let mut renderer = IncrSvgDocServer::default();

                let first = renderer.pack_delta(&document);
                assert!(
                    first.starts_with(protocol::DIFF_V1_PREFIX),
                    "initial preview update should be diff-v1"
                );

                let current = protocol::full_current_frame_from_delta(&first)
                    .expect("full current can be built from an initial incremental frame");
                assert!(
                    current.starts_with(protocol::NEW_PREFIX),
                    "full current preview update should use the new, prefix"
                );
                assert!(
                    current.len() > protocol::NEW_PREFIX.len(),
                    "full current frame should include a payload"
                );
            },
        );
    }
}

pub enum Location {
    Src(SourceLocation),
}

pub trait EditorServer: Send + Sync + 'static {
    fn update_memory_files(
        &self,
        _files: MemoryFiles,
        _reset_shadow: bool,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Ok(()) }
    }

    fn remove_memory_files(
        &self,
        _files: MemoryFilesShort,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Ok(()) }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocToSrcJumpInfo {
    pub filepath: String,
    pub start: Option<(usize, usize)>, // row, column
    pub end: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeCursorPositionRequest {
    filepath: PathBuf,
    line: u32,
    /// fixme: character is 0-based, UTF-16 code unit.
    /// We treat it as UTF-8 now.
    character: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolveSourceLocRequest {
    pub filepath: PathBuf,
    pub line: u32,
    /// fixme: character is 0-based, UTF-16 code unit.
    /// We treat it as UTF-8 now.
    pub character: u32,
}

impl ResolveSourceLocRequest {
    pub fn to_byte_offset(&self, src: &typst::syntax::Source) -> Option<usize> {
        src.lines()
            .line_column_to_byte(self.line as usize, self.character as usize)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryFiles {
    pub files: HashMap<PathBuf, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryFilesShort {
    pub files: Vec<PathBuf>,
    // mtime: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerWindowState {
    pub inner_width: u32,
    pub inner_height: u32,
    #[serde(default)]
    pub outer_x: Option<i32>,
    #[serde(default)]
    pub outer_y: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerWindowStateMessage {
    pub schema_version: u32,
    pub window: ViewerWindowState,
}

pub trait CompileView: Send + Sync {
    /// Identify the compiler snapshot used by rendered document coordinates.
    fn revision(&self) -> String {
        String::new()
    }

    /// Resolve an explicit preview selection against this source snapshot.
    fn preview_source_context(&self, _pos: &DocumentPosition) -> Option<PreviewSourceContext> {
        None
    }

    /// Stable opaque workspace/entry identity for browser-local review marks.
    fn preview_document_id(&self) -> Option<String> {
        None
    }

    /// Apply a real source highlight to a verified literal preview text range.
    fn highlight_preview_selection(
        &self,
        _start: &DocumentPosition,
        _selection: &PreviewTextSelection,
    ) -> PreviewHighlightStatus {
        PreviewHighlightStatus::Unsupported
    }

    /// Allow a concrete preview view to compare its source snapshot with the
    /// preceding one without exposing compiler-specific types to this crate.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get the compiled document.
    fn doc(&self) -> Option<TypstDocument>;
    /// Get the compile status.
    fn status(&self) -> CompileStatus;

    /// Check if the view is by OnSaved event.
    fn is_on_saved(&self) -> bool;
    /// Check if the view is by entry update.
    fn is_by_entry_update(&self) -> bool;

    /// Resolve the source span offset.
    fn resolve_source_span(&self, _by: Location) -> Option<SourceSpanOffset> {
        None
    }

    /// Resolve a physical location in the document.
    fn resolve_frame_loc(
        &self,
        _pos: &DocumentPosition,
    ) -> Option<(SourceSpanOffset, SourceSpanOffset)> {
        None
    }

    /// Resolve the document position.
    fn resolve_document_position(&self, _by: Location) -> Vec<PagedPosition> {
        vec![]
    }

    /// Resolve the source edit that produced this compilation in the new
    /// document. A visual page comparison remains the fallback.
    fn changed_document_positions(&self, _previous: &dyn CompileView) -> Vec<PagedPosition> {
        vec![]
    }

    /// Resolve the span with an optional offset.
    fn resolve_span(&self, _s: Span, _offset: Option<usize>) -> Option<DocToSrcJumpInfo> {
        None
    }
}

pub struct CompileWatcher {
    task_id: String,
    when: TaskWhen,
    doc_sender: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
    editor_tx: mpsc::UnboundedSender<EditorActorRequest>,
    render_tx: broadcast::Sender<RenderActorRequest>,
    status_tx: tokio::sync::watch::Sender<Option<CompileStatus>>,
    change_tracker: Option<Arc<change::ChangeTracker>>,
}

impl CompileWatcher {
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn status(&self, status: CompileStatus) {
        self.status_tx.send_replace(Some(status));
        let _ = self
            .editor_tx
            .send(EditorActorRequest::CompileStatus(status));
    }

    pub fn notify_compile(&self, view: Arc<dyn CompileView>) {
        log::info!(
            "Preview({:?}): received notification: signal({:?}, {:?}), when {:?}",
            self.task_id,
            view.is_by_entry_update(),
            view.is_on_saved(),
            self.when
        );
        if !view.is_by_entry_update()
            && (matches!(self.when, TaskWhen::OnSave) && !view.is_on_saved())
        {
            return;
        }

        let status = view.status();
        match status {
            CompileStatus::CompileSuccess => {
                // it is ok to ignore the error here
                {
                    let mut document = self.doc_sender.write();
                    if let Some(tracker) = &self.change_tracker {
                        tracker.observe(&view, document.as_ref());
                    }
                    *document = Some(view);
                }

                // todo: is it right that ignore zero broadcast receiver?
                let _ = self.render_tx.send(RenderActorRequest::RenderIncremental);
                self.status(CompileStatus::CompileSuccess);
            }
            CompileStatus::Compiling | CompileStatus::CompileError => {
                self.status(status);
            }
        }
    }
}

#[derive(Clone)]
struct DataPlane {
    format: ExportTarget,
    viewer_demand: Option<Arc<demand::ViewerDemand>>,
    focus_store: Option<Arc<focus::FocusStore>>,
    change_tracker: Option<Arc<change::ChangeTracker>>,
    span_interner: SpanInterner,
    webview_tx: broadcast::Sender<WebviewActorRequest>,
    editor_tx: mpsc::UnboundedSender<EditorActorRequest>,
    enable_partial_rendering: bool,
    invert_colors: String,
    renderer_tx: broadcast::Sender<RenderActorRequest>,
    doc_sender: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
    status_rx: tokio::sync::watch::Receiver<Option<CompileStatus>>,
}

/// The invert colors for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewInvertColors {
    /// Inverts all elements.
    Enum(PreviewInvertColor),
    /// Inverts colors per element kinds.
    Object(PreviewInvertColorObject),
}

impl Default for PreviewInvertColors {
    fn default() -> Self {
        PreviewInvertColors::Enum(PreviewInvertColor::Never)
    }
}

impl serde::Serialize for PreviewInvertColors {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PreviewInvertColors::Enum(color) => color.serialize(serializer),
            PreviewInvertColors::Object(obj) => obj.serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for PreviewInvertColors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = PreviewInvertColors;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an object with image and rest fields")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PreviewInvertColors::Enum(Deserialize::deserialize(
                    serde::de::value::StrDeserializer::new(v),
                )?))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                Ok(PreviewInvertColors::Object(Deserialize::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// The ways of inverting colors in the preview.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PreviewInvertColor {
    /// Never inverts colors.
    #[default]
    Never,
    /// Inverts colors automatically based on the color theme.
    Auto,
    /// Always inverts colors.
    Always,
}

/// The invert colors for the preview, which can be applied to images and other
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInvertColorObject {
    /// The invert color mode about images.
    #[serde(default)]
    pub image: PreviewInvertColor,
    /// The invert color mode about rest elements.
    #[serde(default)]
    pub rest: PreviewInvertColor,
}
