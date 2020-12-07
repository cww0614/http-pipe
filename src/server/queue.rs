use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use futures::future;

use crate::common::Packet;

pub struct Queue {
    q: Arc<Mutex<VecDeque<Option<Packet>>>>,
    capacity: usize,

    write_wakers: Arc<Mutex<VecDeque<Waker>>>,
    read_wakers: Arc<Mutex<VecDeque<Waker>>>,
}

impl Queue {
    pub fn new(capacity: usize) -> Self {
        Queue {
            capacity,
            q: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),

            write_wakers: Arc::new(Mutex::new(VecDeque::new())),
            read_wakers: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn register_reader(&self, waker: Waker) {
        self.read_wakers.lock().unwrap().push_back(waker);
    }

    fn register_writer(&self, waker: Waker) {
        self.write_wakers.lock().unwrap().push_back(waker);
    }

    fn wakeup_reader(&self) {
        if let Some(w) = self.read_wakers.lock().unwrap().pop_front() {
            w.wake();
        }
    }

    fn wakeup_writer(&self) {
        if let Some(w) = self.write_wakers.lock().unwrap().pop_front() {
            w.wake();
        }
    }

    fn poll_push(&self, cx: &mut Context, e: Packet) -> Poll<()> {
        let mut q = self.q.lock().unwrap();
        if q.len() < self.capacity {
            q.push_back(Some(e));

            self.wakeup_reader();

            Poll::Ready(())
        } else {
            self.register_writer(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_get(&self, cx: &mut Context, index: usize) -> Poll<Option<Packet>> {
        let q = self.q.lock().unwrap();
        if q.is_empty() {
            self.register_reader(cx.waker().clone());
            return Poll::Pending;
        }

        let first_index = q[0].as_ref().unwrap().index;

        if index < first_index {
            return Poll::Ready(None);
        }

        if first_index <= index && first_index + q.len() > index {
            return Poll::Ready(q[index - first_index].clone());
        }

        self.register_reader(cx.waker().clone());
        Poll::Pending
    }

    pub async fn push(&self, e: Packet) {
        future::poll_fn(|cx| self.poll_push(cx, e.clone())).await
    }

    pub async fn get(&self, index: usize) -> Option<Packet> {
        future::poll_fn(|cx| self.poll_get(cx, index)).await
    }

    pub fn remove(&self, index: usize) {
        let mut q = self.q.lock().unwrap();

        if q.is_empty() || q[0].as_ref().unwrap().index > index {
            return;
        }

        let first_index = q[0].as_ref().unwrap().index;

        q[index - first_index] = None;

        while let Some(None) = q.front() {
            q.pop_front().unwrap();
        }

        self.wakeup_writer();
    }
}
