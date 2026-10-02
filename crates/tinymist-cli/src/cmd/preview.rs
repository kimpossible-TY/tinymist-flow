use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use hyper_tungstenite::tungstenite::Message;
use tinymist::{
    PREVIEW_COMPAT_LOG_TARGET,
    project::ProjectPreviewState,
    tool::{
        preview::{
            PreviewCliArgs, ProjectPreviewHandler, bind_streams, make_http_server,
            make_theme_http_server,
        },
        project::{ProjectOpts, StartProjectResult, start_project},
    },
};
use tinymist_assets::TYPST_PREVIEW_HTML;
use tinymist_preview::{
    ControlPlaneMessage, ControlPlaneTx, PreviewBuilder, PreviewConfig, frontend_html,
};
use tinymist_project::{EntryReader, WorldProvider};
use tinymist_std::error::prelude::*;
use tinymist_task::ExportTarget;
use tokio::sync::mpsc;

use crate::utils::exit_on_ctrl_c;

/// Entry point of the preview tool.
pub async fn preview_main(mut args: PreviewCliArgs) -> Result<()> {
    log::info!("Arguments: {args:#?}");
    let handle = tokio::runtime::Handle::current();

    let config = args.preview.config(&PreviewConfig::default());
    #[cfg(feature = "open")]
    let open_in_browser = args.open_in_browser(true);
    let static_file_host =
        if args.static_file_host == args.data_plane_host || !args.static_file_host.is_empty() {
            Some(args.static_file_host)
        } else {
            None
        };

    exit_on_ctrl_c();

    let preview_target = args.preview.format;
    if matches!(preview_target, ExportTarget::Bundle) {
        bail!("bundle export target is not supported by preview");
    }
    if args.follow_system_theme && !matches!(preview_target, ExportTarget::Paged) {
        bail!("--follow-system-theme requires paged output");
    }
    let dark_inputs = if args.follow_system_theme {
        set_theme_input(&mut args.compile.inputs, "light");
        let mut dark_compile = args.compile.clone();
        set_theme_input(&mut dark_compile.inputs, "dark");
        dark_compile.resolve_inputs()
    } else {
        None
    };
    let verse = args.compile.resolve()?;
    let previewer = PreviewBuilder::new(config.clone());
    let previewer = match std::env::var_os("TINYMIST_PREVIEW_FOCUS_FILE") {
        Some(path) => {
            if !matches!(preview_target, ExportTarget::Paged) {
                bail!("preview focus sharing requires paged output");
            }
            previewer
                .with_focus_file(path.into())
                .map_err(anyhow::Error::from)?
        }
        None => previewer,
    };
    let previewer = match std::env::var_os("TINYMIST_PREVIEW_CHANGE_FILE") {
        Some(path) => {
            if !matches!(preview_target, ExportTarget::Paged) {
                bail!("preview change restoration requires paged output");
            }
            let entry = verse
                .entry_file()
                .and_then(|path| path.to_err().ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("preview change restoration requires a local entry")
                })?;
            let root = verse.entry_state().root().map(|root| root.to_path_buf());
            let project = serde_json::to_string(&(root, entry)).map_err(anyhow::Error::from)?;
            previewer
                .with_change_file(
                    path.into(),
                    project,
                    if args.follow_system_theme {
                        "light"
                    } else {
                        "default"
                    },
                )
                .map_err(anyhow::Error::from)?
        }
        None => previewer,
    };
    let dark_builder = dark_inputs.as_ref().map(|_| previewer.sibling(config));

    let (service, handle, dark_handle) = {
        let preview_state = ProjectPreviewState::default();
        let opts = ProjectOpts {
            handle: Some(handle),
            preview: preview_state.clone(),
            export_target: preview_target,
            ..ProjectOpts::default()
        };

        let StartProjectResult {
            mut service,
            intr_tx,
            mut editor_rx,
        } = start_project(verse, Some(opts), |compiler, intr, next| {
            next(compiler, intr)
        });

        // Consume editor_rx
        tokio::spawn(async move { while editor_rx.recv().await.is_some() {} });

        let id = service.compiler.primary.id.clone();
        let registered =
            preview_state.register(&id, previewer.compile_watcher(args.task_id.clone()));
        if !registered {
            tinymist_std::bail!("failed to register preview");
        }

        let dark_handle = if let Some(inputs) = dark_inputs {
            let entry = service.compiler.primary.verse.entry_state();
            let dark_id = service
                .compiler
                .restart_dedicate("preview-theme-dark", entry)?;
            let project = service
                .compiler
                .projects()
                .find(|p| p.id == dark_id)
                .unwrap();
            project
                .verse
                .increment_revision(|verse| verse.set_inputs(inputs));
            if !preview_state.register(
                &dark_id,
                dark_builder
                    .as_ref()
                    .unwrap()
                    .compile_watcher(format!("{}-dark", args.task_id)),
            ) {
                bail!("failed to register dark preview");
            }
            Some(Arc::new(ProjectPreviewHandler {
                project_id: dark_id,
                client: Box::new(intr_tx.clone()),
            }))
        } else {
            None
        };

        let handle: Arc<ProjectPreviewHandler> = Arc::new(ProjectPreviewHandler {
            project_id: id,
            client: Box::new(intr_tx),
        });

        (service, handle, dark_handle)
    };

    let (lsp_tx, mut lsp_rx) = ControlPlaneTx::new(true);

    let control_plane_server_handle = tokio::spawn(async move {
        let (control_sock_tx, mut control_sock_rx) = mpsc::unbounded_channel();

        let srv =
            make_http_server(String::default(), args.control_plane_host, control_sock_tx).await;
        log::info!(
            target: PREVIEW_COMPAT_LOG_TARGET,
            "Control panel server listening on: {}",
            srv.addr
        );

        let control_websocket = control_sock_rx.recv().await.unwrap();
        let ws = control_websocket.await.unwrap();

        tokio::pin!(ws);

        loop {
            tokio::select! {
                Some(resp) = lsp_rx.resp_rx.recv() => {
                    let r = ws
                        .send(Message::text(serde_json::to_string(&resp).unwrap()))
                        .await;
                    let Err(err) = r else {
                        continue;
                    };

                    log::warn!("failed to send response to editor {err:?}");
                    break;

                }
                msg = ws.next() => {
                    let msg = match msg {
                        Some(Ok(Message::Text(msg))) => Some(msg),
                        Some(Ok(Message::Binary(..))) =>{
                            log::error!("unsupported binary message");
                            break;
                        }
                        Some(Ok(Message::Ping(..))) =>{
                            log::error!("unsupported ping message");
                            break;
                        }
                        Some(Ok(Message::Pong(..))) =>{
                            log::error!("unsupported pong message");
                            break;
                        }
                        Some(Ok(Message::Close(..))) =>{
                            log::error!("unsupported close message");
                            break;
                        }
                        Some(Ok(Message::Frame(..))) =>{
                            log::error!("unsupported frame message");
                            break;
                        }
                        Some(Err(e)) => {
                            log::error!("failed to receive message: {e}");
                            break;
                        }
                        _ => None,
                    };

                    if let Some(msg) = msg {
                        let Ok(msg) = serde_json::from_str::<ControlPlaneMessage>(&msg) else {
                            log::warn!("failed to parse control plane request: {msg:?}");
                            break;
                        };

                        lsp_rx.ctl_tx.send(msg).unwrap();
                    } else {
                        // todo: inform the editor that the connection is closed.
                        break;
                    }
                }

            }
        }

        let _ = srv.shutdown_tx.send(());
        let _ = srv.join.await;
    });

    let (websocket_tx, websocket_rx) = mpsc::unbounded_channel();
    let mut previewer = previewer.build(lsp_tx, handle.clone()).await;
    let dark_websocket_tx = if let (Some(builder), Some(handle)) = (dark_builder, dark_handle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (control_tx, mut control_rx) = ControlPlaneTx::new(true);
        // The second pipeline has no editor connection, but its response channel
        // must remain open while source edits update both variants.
        tokio::spawn(async move {
            while control_rx.resp_rx.recv().await.is_some() {}
            drop(control_rx);
        });
        let mut dark_previewer = builder.build(control_tx, handle).await;
        bind_streams(&mut dark_previewer, rx);
        tokio::spawn(dark_previewer.join());
        Some(tx)
    } else {
        None
    };
    tokio::spawn(service.run());

    bind_streams(&mut previewer, websocket_rx);

    let page_title = tinymist::tool::preview::resolve_page_title(
        args.preview.page_title.as_deref(),
        args.compile.input.as_deref(),
    );
    let frontend_html = frontend_html(
        TYPST_PREVIEW_HTML,
        args.preview.preview_mode,
        "/",
        &page_title,
    )
    .replace(
        "preview-arg:systemTheme:false",
        if args.follow_system_theme {
            "preview-arg:systemTheme:true"
        } else {
            "preview-arg:systemTheme:false"
        },
    );

    let static_server = if let Some(static_file_host) = static_file_host {
        log::warn!(
            "--static-file-host is deprecated, which will be removed in the future. Use --data-plane-host instead."
        );
        let html = frontend_html.clone();
        Some(
            make_theme_http_server(
                html,
                static_file_host,
                websocket_tx.clone(),
                dark_websocket_tx.clone(),
            )
            .await,
        )
    } else {
        None
    };

    let srv = make_theme_http_server(
        frontend_html,
        args.data_plane_host,
        websocket_tx,
        dark_websocket_tx,
    )
    .await;
    log::info!(
        target: PREVIEW_COMPAT_LOG_TARGET,
        "Data plane server listening on: {}",
        srv.addr
    );

    let static_server_addr = static_server.as_ref().map(|s| s.addr).unwrap_or(srv.addr);
    log::info!(
        target: PREVIEW_COMPAT_LOG_TARGET,
        "Static file server listening on: {static_server_addr}"
    );

    #[cfg(feature = "open")]
    if open_in_browser {
        open::that_detached(format!("http://{static_server_addr}"))
            .log_error("failed to open browser for preview");
    }

    let _ = tokio::join!(previewer.join(), srv.join, control_plane_server_handle);
    // Assert that the static server's lifetime is longer than the previewer.
    let _s = static_server;

    Ok(())
}

fn set_theme_input(inputs: &mut Vec<(String, String)>, theme: &str) {
    inputs.retain(|(key, _)| key != "theme");
    inputs.push(("theme".into(), theme.into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn themes_are_opt_in_and_preserve_other_inputs() {
        assert!(!PreviewCliArgs::parse_from(["preview", "main.typ"]).follow_system_theme);
        assert!(
            PreviewCliArgs::parse_from(["preview", "main.typ", "--follow-system-theme"])
                .follow_system_theme
        );
        let mut inputs = vec![
            ("other".into(), "value".into()),
            ("theme".into(), "old".into()),
        ];
        set_theme_input(&mut inputs, "light");
        let mut dark = inputs.clone();
        set_theme_input(&mut dark, "dark");
        assert_eq!(
            inputs,
            [
                ("other".into(), "value".into()),
                ("theme".into(), "light".into())
            ]
        );
        assert_eq!(
            dark,
            [
                ("other".into(), "value".into()),
                ("theme".into(), "dark".into())
            ]
        );
    }
}
