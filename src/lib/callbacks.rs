use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use anyhow::Result;
use futures::future::BoxFuture;
use indexmap::IndexMap;

#[derive(Clone)]
pub struct Callbacks<T> {
    callbacks: Arc<Mutex<IndexMap<usize, Callback<T>>>>,
}

impl<T> Default for Callbacks<T> {
    fn default() -> Self {
        Self {
            callbacks: Arc::new(Mutex::new(IndexMap::new())),
        }
    }
}

impl<T> std::fmt::Debug for Callbacks<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Callback").finish()
    }
}

impl<T> Callbacks<T> {
    pub fn add_callback<F, Fut>(&self, callback: F) -> usize
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let callback_id = COUNTER.fetch_add(1, Ordering::Relaxed);

        let mut callbacks = self.callbacks.lock().unwrap();
        callbacks.insert(callback_id, Callback::new(callback));

        callback_id
    }

    pub fn remove_callback(&self, id: usize) {
        let mut callbacks = self.callbacks.lock().unwrap();
        callbacks.shift_remove(&id);
    }

    pub fn call_all(&self, msg: T) -> Vec<BoxFuture<'static, Result<()>>>
    where
        T: Clone,
    {
        let callbacks = self.callbacks.lock().unwrap();
        callbacks
            .values()
            .map(|callback| callback.call(msg.clone()))
            .collect()
    }
}

#[derive(Clone)]
struct Callback<T> {
    callback: Arc<dyn Fn(T) -> BoxFuture<'static, Result<()>> + Send + Sync>,
}

impl<T> Callback<T> {
    fn new<F, Fut>(callback: F) -> Self
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            callback: Arc::new(move |msg: T| Box::pin(callback(msg))),
        }
    }

    fn call(&self, msg: T) -> BoxFuture<'static, Result<()>> {
        (self.callback)(msg)
    }
}

pub trait MessageCallback<T>: Send + Sync + 'static {
    fn into_boxed(self) -> Box<dyn Fn(T) -> BoxFuture<'static, Result<()>> + Send + Sync>;
}

impl<F, T, Fut> MessageCallback<T> for F
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn into_boxed(self) -> Box<dyn Fn(T) -> BoxFuture<'static, Result<()>> + Send + Sync> {
        Box::new(move |msg: T| Box::pin(self(msg)))
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::*;

    #[tokio::test]
    async fn test_callbacks() {
        let callbacks = Callbacks::<String>::default();

        let (tx1, rx1) = oneshot::channel();
        let tx1 = Arc::new(Mutex::new(Some(tx1)));
        let id1 = callbacks.add_callback(move |msg: String| {
            let tx1 = tx1.clone();
            async move {
                if msg == "test" {
                    if let Some(sender) = tx1.lock().unwrap().take() {
                        sender.send(()).unwrap();
                    }
                }
                Ok(())
            }
        });

        let (tx2, rx2) = oneshot::channel();
        let tx2 = Arc::new(Mutex::new(Some(tx2)));
        let id2 = callbacks.add_callback(move |msg: String| {
            let tx2 = tx2.clone();
            async move {
                if msg == "test" {
                    if let Some(sender) = tx2.lock().unwrap().take() {
                        sender.send(()).unwrap();
                    }
                }
                Ok(())
            }
        });

        let (tx3, rx3) = oneshot::channel();
        let tx3 = Arc::new(Mutex::new(Some(tx3)));
        let id3 = callbacks.add_callback(move |msg: String| {
            let tx3 = tx3.clone();
            async move {
                if msg == "test" {
                    if let Some(sender) = tx3.lock().unwrap().take() {
                        sender.send(()).unwrap();
                    }
                }
                Ok(())
            }
        });

        // Remove the secondly added callback
        callbacks.remove_callback(id2);

        // Add a fourth callback
        let (tx4, rx4) = oneshot::channel();
        let tx4 = Arc::new(Mutex::new(Some(tx4)));
        let id4 = callbacks.add_callback(move |msg: String| {
            let tx4 = tx4.clone();
            async move {
                if msg == "test" {
                    if let Some(sender) = tx4.lock().unwrap().take() {
                        sender.send(()).unwrap();
                    }
                }
                Ok(())
            }
        });

        // Remove the third callback
        callbacks.remove_callback(id3);

        // Certify that only the callback functions 1 and 4 are called
        let futures = callbacks.call_all("test".to_string());
        futures::future::join_all(futures).await;

        assert!(rx1.await.is_ok(), "Callback 1 should be called");
        assert!(rx2.await.is_err(), "Callback 2 should NOT be called");
        assert!(rx3.await.is_err(), "Callback 3 should NOT be called");
        assert!(rx4.await.is_ok(), "Callback 4 should be called");

        // Remove remaining callbacks
        callbacks.remove_callback(id1);
        callbacks.remove_callback(id4);
    }
}
