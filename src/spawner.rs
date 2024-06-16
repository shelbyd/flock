use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use eyre::OptionExt;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{ProcessCtx, ThreadCtx, ThreadResult, ThreadState, Word};

#[async_trait::async_trait]
pub(crate) trait Spawner: Send + Sync + 'static {
    async fn spawn(&self, process: &Arc<ProcessCtx>, state: ThreadState) -> eyre::Result<Word>;
    async fn join(&self, tid: Word) -> eyre::Result<ThreadResult>;
}

#[derive(Default)]
pub(crate) struct LocalSpawner {
    next_child_id: AtomicU64,
    local_threads: Mutex<HashMap<Word, JoinHandle<eyre::Result<ThreadResult>>>>,
}

impl LocalSpawner {
    pub(crate) fn new() -> LocalSpawner {
        LocalSpawner::default()
    }
}

#[async_trait::async_trait]
impl Spawner for LocalSpawner {
    async fn spawn(&self, process: &Arc<ProcessCtx>, state: ThreadState) -> eyre::Result<Word> {
        let thread_id = self.next_child_id.fetch_add(1, Ordering::Relaxed) as Word;

        let thread_ctx = ThreadCtx {
            id: thread_id,
            proc: Arc::clone(process),
            state,
        };

        self.local_threads
            .lock()
            .await
            .insert(thread_id, spawn_execute(thread_ctx));
        Ok(thread_id)
    }

    async fn join(&self, tid: Word) -> eyre::Result<ThreadResult> {
        let handle = {
            let mut threads = self.local_threads.lock().await;
            threads
                .remove(&tid)
                .ok_or_eyre(format!("Joined unknown thread: {tid}"))?
        };
        Ok(handle.await??)
    }
}

fn spawn_execute(ctx: ThreadCtx) -> JoinHandle<Result<ThreadResult, eyre::Error>> {
    tokio::task::spawn(async move { ctx.execute().await })
}
