//! Deterministic tests for worker completion and actor publication races.

use clap::Parser;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Default)]
struct RecordingHandler {
    published: AtomicUsize,
    completed_statuses: AtomicUsize,
}

impl CompileHandler<LspCompilerFeat, ProjectInsStateExt> for RecordingHandler {
    fn on_any_compile_reason(&self, _: &mut LspProjectCompiler) {}
    fn notify_compile(&self, _: &LspCompiledArtifact) {
        self.published.fetch_add(1, Ordering::Relaxed);
    }
    fn status(&self, _: usize, report: CompileReport) {
        if !matches!(report.status, CompileStatusEnum::Compiling) {
            self.completed_statuses.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn project() -> (tempfile::TempDir, LspProjectCompiler, Arc<RecordingHandler>) {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("main.typ");
    std::fs::write(&input, "#let value = 1").unwrap();
    let args =
        CompileOnceArgs::parse_from(["tinymist", "--ignore-system-fonts", input.to_str().unwrap()]);
    let verse = args.resolve().unwrap();
    let handler = Arc::new(RecordingHandler::default());
    let compiler = ProjectCompiler::new(
        verse,
        mpsc::unbounded_channel().0,
        CompileServerOpts {
            handler: handler.clone(),
            syntax_only: true,
            ..Default::default()
        },
    );
    (temp, compiler, handler)
}

fn queue(
    compiler: &mut LspProjectCompiler,
) -> impl FnOnce() -> CompileOutcome<LspCompilerFeat> + use<> {
    let handler = compiler.handler.clone();
    let (ticket, work) = compiler.primary.may_compile_latest(&handler).unwrap();
    compiler.primary.ext.active_compile = Some(ticket);
    compiler.primary.ext.compiling_since = Some(tinymist_std::time::now());
    work
}

#[test]
fn cancelled_queue_work_releases_slot_without_publishing() {
    let (_temp, mut compiler, handler) = project();
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let work = queue(&mut compiler);
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let discarded = work();
    assert!(discarded.artifact.is_none());
    ProjectState::do_interrupt(&mut compiler, Interrupt::CompileFinished(discarded));
    assert!(compiler.primary.ext.active_compile.is_none());
    assert!(compiler.primary.ext.compiling_since.is_none());
    assert!(compiler.primary.reason.any());
    assert_eq!(handler.published.load(Ordering::Relaxed), 0);
    assert_eq!(handler.completed_statuses.load(Ordering::Relaxed), 0);

    let latest = queue(&mut compiler)();
    ProjectState::do_interrupt(&mut compiler, Interrupt::CompileFinished(latest));
    assert_eq!(handler.published.load(Ordering::Relaxed), 1);
    assert_eq!(handler.completed_statuses.load(Ordering::Relaxed), 1);
    assert!(compiler.primary.ext.active_compile.is_none());
}

#[test]
fn edit_after_worker_finishes_prevents_stale_publication() {
    let (_temp, mut compiler, handler) = project();
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let completed = queue(&mut compiler)();
    assert!(completed.is_publishable());
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    ProjectState::do_interrupt(&mut compiler, Interrupt::CompileFinished(completed));
    assert_eq!(handler.published.load(Ordering::Relaxed), 0);
    assert!(compiler.primary.ext.active_compile.is_none());
}

#[test]
fn old_project_completion_cannot_release_replacement_worker() {
    let (_old_temp, mut old, _) = project();
    old.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let old_work = queue(&mut old);
    drop(old);
    let (_new_temp, mut new, handler) = project();
    new.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let new_work = queue(&mut new);
    let active = new.primary.ext.active_compile.clone().unwrap();
    ProjectState::do_interrupt(&mut new, Interrupt::CompileFinished(old_work()));
    assert!(new
        .primary
        .ext
        .active_compile
        .as_ref()
        .unwrap()
        .same_task(&active));
    assert_eq!(handler.published.load(Ordering::Relaxed), 0);
    ProjectState::do_interrupt(&mut new, Interrupt::CompileFinished(new_work()));
    assert!(new.primary.ext.active_compile.is_none());
    assert_eq!(handler.published.load(Ordering::Relaxed), 1);
}

#[test]
fn delayed_accepted_notification_does_not_clear_new_worker() {
    let (_temp, mut compiler, _) = project();
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let accepted = queue(&mut compiler)();
    let artifact = accepted.artifact.as_ref().unwrap().clone();
    ProjectState::do_interrupt(&mut compiler, Interrupt::CompileFinished(accepted));
    compiler.process(Interrupt::Compile(ProjectInsId::PRIMARY));
    let _new_work = queue(&mut compiler);
    let active = compiler.primary.ext.active_compile.clone().unwrap();
    ProjectState::do_interrupt(&mut compiler, Interrupt::Compiled(artifact));
    assert!(compiler
        .primary
        .ext
        .active_compile
        .as_ref()
        .unwrap()
        .same_task(&active));
    assert!(compiler.primary.ext.compiling_since.is_some());
}
