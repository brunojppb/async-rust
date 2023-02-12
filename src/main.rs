use futures::{
    future::{BoxFuture, FutureExt},
    task::{waker_ref, ArcWake},
};

use std::{
    future::Future,
    sync::mpsc::{sync_channel, Receiver, SyncSender},
    sync::{Arc, Mutex},
    task::Context,
    time::Duration,
};

use async_rust::TimerFuture;

/// Task is a type of future that can reschedule itself in the queue
/// until completion, so it can be polled by the `Executor`.
struct Task {
    /// In-flight future that should be pushed to completion
    ///
    /// Even though we are using just one thread o interleave tasks
    /// a Mutex is necessary here so Rust knows that we can't mutate
    /// this value unsafely across threads without acquiring the lock.
    /// According to the async Rust book, in a production-grade Executor we
    /// would use `UnsafeCell` instead.
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    /// mechanism to reschedule itself back onto the task queue.
    task_sender: SyncSender<Arc<Task>>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // send itself back to the task channel so it will be picked up
        // by the Executor when polled again.
        let cloned = arc_self.clone();
        arc_self
            .task_sender
            // Using `send` instead would potentially lead us to a deadlock
            // where the queue is full, the executor can't process any more tasks
            // and `send` will be waiting forever here for the queue to allocate
            // space in the channel buffer.
            // Using `try_send`
            .try_send(cloned)
            .expect("Too many tasks queued");
    }
}

/// Executor that receives tasks from a channel and runs them.
struct Executor {
    ready_queue: Receiver<Arc<Task>>,
}

impl Executor {
    fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            // take a future from the channel inbox and while it's not yet completed,
            // poll it and attempt to complete. If it's still pending, put it back.
            let mut maybe_future = task.future.lock().unwrap();
            if let Some(mut future) = maybe_future.take() {
                // create a local scope Waker from the task itself
                // so we can extract the the future's context to poll it.
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&waker);
                if future.as_mut().poll(context).is_pending() {
                    // Future isn't done it. put it back so it can be poll again
                    // at a later point by the executor.
                    *maybe_future = Some(future);
                }
            }
        }
    }
}

/// Spawns new futures onto the task channel
struct Spawner {
    task_sender: SyncSender<Arc<Task>>,
}

impl Spawner {
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = future.boxed();
        let task = Arc::new(Task {
            future: Mutex::new(Some(future)),
            task_sender: self.task_sender.clone(),
        });
        self.task_sender
            .try_send(task)
            .expect("Too many tasks queued");
    }
}

fn new_executor_and_spanwer() -> (Executor, Spawner) {
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (task_sender, ready_queue) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { ready_queue }, Spawner { task_sender })
}

fn main() {
    let (executor, spanwer) = new_executor_and_spanwer();

    spanwer.spawn(async {
        TimerFuture::new("heavy_stuff", Duration::new(2, 0)).await;
    });

    spanwer.spawn(async {
        TimerFuture::new("slow_socket", Duration::new(2, 0)).await;
    });

    drop(spanwer);

    executor.run();
}
