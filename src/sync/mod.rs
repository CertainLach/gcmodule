//! ## Multi-thread support
//!
//! The main type [`Cc`](type.cc.html) works fine in a single-thread environment.
//!
//! There are also [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html)
//! and [`ThreadedCc`](type.ThreadedCc.html) for multi-thread usecases. Beware
//! they take more memory, are slower, and a bit harder to use.
//!
//! ```
//! use jrsonnet_gcmodule::{ThreadedObjectSpace, ThreadedCc, Trace, TraceBox};
//! use std::sync::Mutex;
//!
//! type List = ThreadedCc<Mutex<Vec<TraceBox<dyn Trace + Send + Sync>>>>;
//! let space = ThreadedObjectSpace::default();
//! {
//!     let list1: List = space.create(Mutex::new(Default::default()));
//!     let list2: List = space.create(Mutex::new(Default::default()));
//!     let thread = std::thread::spawn(move || {
//!         list1.borrow().lock().unwrap().push(TraceBox(Box::new(list2.clone())));
//!         list2.borrow().lock().unwrap().push(TraceBox(Box::new(list1.clone())));
//!     });
//!     thread.join().unwrap();
//! }
//! assert_eq!(space.count_tracked(), 2);
//! assert_eq!(space.collect_cycles(), 2);
//! assert_eq!(space.count_tracked(), 0);
//! ```

pub(crate) mod collect;
mod ref_count;

#[cfg(test)]
mod tests;

use crate::Trace;
use crate::Tracer;
use crate::cc::RawCc;
use crate::ref_count::RefCount;
use collect::ThreadedObjectSpace;
use parking_lot::RawRwLock;
use parking_lot::lock_api::RwLockReadGuard;
use std::marker::PhantomData;
use std::ops::Deref;

/// A multi-thread reference-counting pointer that integrates with cyclic
/// garbage collection.
///
/// [`ThreadedCc`](type.ThreadedCc.html) is similar to [`Cc`](type.Cc.html).
/// It works with multi-thread but is significantly slower than
/// [`Cc`](type.Cc.html).
///
/// To construct a [`ThreadedCc`](type.ThreadedCc.html), use
/// [`ThreadedObjectSpace::create`](struct.ThreadedObjectSpace.html#method.create).
pub type ThreadedCc<T> = RawCc<T, ThreadedObjectSpace>;

/// Wraps a borrowed reference to [`ThreadedCc`](type.ThreadedCc.html).
///
/// The wrapper automatically takes a lock that prevents the collector from
/// running. This ensures that when the collector is running, there are no
/// borrowed references of [`ThreadedCc`](type.ThreadedCc.html). Therefore
/// [`ThreadedCc`](type.ThreadedCc.html)s can be seen as temporarily immutable,
/// even if they might have interior mutability. The collector relies on this
/// for correctness.
pub struct ThreadedCcRef<'a, T: ?Sized> {
    // Prevent the collector from running when a reference is present.
    locked: RwLockReadGuard<'a, RawRwLock, ()>,

    // Provide access to the parent `Acc`.
    parent: &'a ThreadedCc<T>,

    // !Send + !Sync.
    _phantom: PhantomData<*mut ()>,
}

// safety: similar to `std::sync::Arc`
unsafe impl<T: Send + Sync + ?Sized> Send for ThreadedCc<T> {}
unsafe impl<T: Send + Sync + ?Sized> Sync for ThreadedCc<T> {}

impl<T: ?Sized> ThreadedCc<T> {
    /// Immutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned value exits scope.
    #[must_use]
    pub fn borrow(&self) -> ThreadedCcRef<'_, T> {
        ThreadedCcRef {
            locked: self.inner().ref_count.locked(),
            parent: self,
            _phantom: PhantomData,
        }
    }
}

impl<T: ?Sized> Deref for ThreadedCcRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Only a marker to show the lock is here
        #[allow(clippy::let_underscore_lock)]
        let _ = &self.locked;
        self.parent.inner().deref()
    }
}

impl<T: Trace> Trace for ThreadedCc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer);
    }

    #[inline]
    fn is_type_tracked() -> bool {
        T::is_type_tracked()
    }
}

impl Trace for ThreadedCc<dyn Trace> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer);
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }
}

impl Trace for ThreadedCc<dyn Trace + Send> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer);
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }
}

impl Trace for ThreadedCc<dyn Trace + Send + Sync> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer);
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }
}
