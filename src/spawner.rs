use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use eyre::OptionExt;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    rand::Rand,
    remote::{Message, Peers},
    ProcessCtx, ThreadCtx, ThreadResult, ThreadState, Word,
};

pub(crate) struct Spawner {
    rand: Rand,
    spawn_count: AtomicU64,
    threads: Mutex<HashMap<Word, JoinHandle<eyre::Result<ThreadResult>>>>,
    peers: Arc<Peers>,
}

impl Spawner {
    pub(crate) fn new(rand: Rand, peers: Arc<Peers>) -> Spawner {
        Spawner {
            rand,
            spawn_count: Default::default(),
            threads: Default::default(),
            peers,
        }
    }

    pub(crate) async fn spawn(
        &self,
        process: &Arc<ProcessCtx>,
        state: ThreadState,
    ) -> eyre::Result<Word> {
        let thread_id = self
            .rand
            .get("thread_id")
            .get(self.spawn_count.fetch_add(1, Ordering::Relaxed).to_string())
            .word();

        let context = ThreadCtx {
            id: thread_id,
            proc: Arc::clone(process),
            state,
        };

        let locations = self.peers.len() + 1;
        match (thread_id as usize) % locations {
            0 => {
                self.threads
                    .lock()
                    .await
                    .insert(thread_id, spawn_execute(context));
            }
            peer => self.peers[peer - 1].send_message(Message::Spawn { context })?,
        }

        Ok(thread_id)
    }

    pub(crate) async fn join(&self, tid: Word) -> eyre::Result<ThreadResult> {
        let handle = {
            let mut threads = self.threads.lock().await;
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
