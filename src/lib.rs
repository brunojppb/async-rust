use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    thread,
    time::Duration,
};

use rand::Rng;

pub struct TimerFuture {
    shared_state: Arc<Mutex<SharedState>>,
}

struct SharedState {
    completed: bool,
    waker: Option<Waker>,
}

impl Future for TimerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared_state = self.shared_state.lock().unwrap();
        if shared_state.completed {
            // This future is completed. No need to reference the waker anymore.
            // This future should be removed from the executor queue.
            Poll::Ready(())
        } else {
            // The concrete Waker comes from the execution context when Poll-ing it
            // So we store it here in order to call it once we are ready to make progress.
            // We always need to clone it so we keep the up2date reference from this context,
            // otherwise different threads could have a stale pointers to the waker.
            shared_state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl TimerFuture {
    pub fn new(duration: Duration) -> Self {
        let shared_state = Arc::new(Mutex::new(SharedState {
            completed: false,
            waker: None,
        }));

        // Make a copy of the shared state pointer so we can move
        // its reference to the thread scope which will own it.
        let thread_shared_state = shared_state.clone();
        thread::spawn(move || {
            let mut time_spent = duration.clone();
            loop {
                println!("🏋🏽 Doing something heavy on a thread...");
                thread::sleep(duration);
                let mut rng = rand::prelude::thread_rng();
                let is_done = rng.gen_bool(1.0 / 5.0);
                if is_done {
                    println!("✅ Done doing something heavy. took {:?}", &time_spent);
                    let mut shared_state = thread_shared_state.lock().unwrap();
                    shared_state.completed = true;
                    if let Some(waker) = shared_state.waker.take() {
                        waker.wake();
                        break;
                    }
                } else {
                    println!("😴 Still not done. Work will continue later...");
                    time_spent += duration;
                }
            }
        });

        TimerFuture { shared_state }
    }
}
