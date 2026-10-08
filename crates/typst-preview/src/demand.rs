use std::sync::Arc;

/// Counts connected viewers independently for each preview pipeline.
pub(crate) struct ViewerDemand {
    viewers: parking_lot::Mutex<usize>,
    changed: Box<dyn Fn(bool) + Send + Sync>,
}

impl ViewerDemand {
    pub fn new(changed: impl Fn(bool) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            viewers: parking_lot::Mutex::new(0),
            changed: Box::new(changed),
        })
    }

    pub fn connect(self: &Arc<Self>) -> ConnectedViewer {
        let mut viewers = self.viewers.lock();
        *viewers += 1;
        if *viewers == 1 {
            (self.changed)(true);
        }
        ConnectedViewer(self.clone())
    }
}

/// Releases demand even if a connection task fails or is cancelled.
pub(crate) struct ConnectedViewer(Arc<ViewerDemand>);

impl Drop for ConnectedViewer {
    fn drop(&mut self) {
        let mut viewers = self.0.viewers.lock();
        *viewers -= 1;
        if *viewers == 0 {
            (self.0.changed)(false);
        }
    }
}
