use std::ops::Index;

use crate::{
    event::{Event, EventListener},
    ThreadCtx,
};

pub(crate) struct Peers {
    peers: Vec<Peer>,
}

impl Peers {
    pub(crate) fn new() -> Peers {
        Peers {
            peers: Default::default(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.peers.len()
    }
}

#[async_trait::async_trait]
impl EventListener for Peers {
    async fn on_event(&self, event: &Event) -> eyre::Result<()> {
        match event {
            Event::Message(_) => {}
        }

        Ok(())
    }
}

impl Index<usize> for Peers {
    type Output = Peer;

    fn index(&self, index: usize) -> &Self::Output {
        &self.peers[index]
    }
}

pub(crate) struct Peer {}

impl Peer {
    pub(crate) fn send_message(&self, _message: Message) -> eyre::Result<()> {
        todo!();
    }
}

pub(crate) enum Message {
    Spawn { context: ThreadCtx },
}
