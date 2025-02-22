use super::ref_count::ThreadedRefCount;
use super::ThreadedCc;
use crate::cc::CcDummy;
use crate::cc::CcDyn;
use crate::collect;
use crate::collect::AbstractObjectSpace;
use crate::collect::Linked;
use crate::debug;
use crate::Trace;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::cell::Cell;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;

#[repr(C)]
pub struct Header {
    next: Cell<*const Header>,
    prev: Cell<*const Header>,

    /// Vtable of (`&CcBox<T> as &dyn CcDyn`)
    ccdyn_vptr: *const (),

    /// Lock for mutating the linked list.
    linked_list_lock: Arc<Mutex<()>>,
}

/// A collection of tracked [`ThreadedCc`](type.ThreadedCc.html) objects
/// that can be garbage collected.
///
/// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html) is similar to
/// [`ObjectSpace`](struct.ObjectSpace.html) but works with multi-thread.
pub struct ThreadedObjectSpace {
    /// Linked list to the tracked objects.
    list: Pin<Box<Header>>,

    /// Whether the collector is running.
    collector_lock: Arc<RwLock<()>>,
}

// safety: accesses are protected by mutex
unsafe impl Send for ThreadedObjectSpace {}
unsafe impl Sync for ThreadedObjectSpace {}

impl AbstractObjectSpace for ThreadedObjectSpace {
    type RefCount = ThreadedRefCount;
    type Header = Header;

    fn insert(&self, header: &mut Self::Header, value: &dyn CcDyn) {
        debug_assert!(Arc::ptr_eq(
            &header.linked_list_lock,
            &self.list.linked_list_lock
        ));
        // Should be locked by `create()` already.
        debug_assert!(self.list.linked_list_lock.try_lock().is_none());
        let prev: &Header = &self.list;
        debug_assert!(!collect::is_collecting(prev));
        debug_assert!(header.next.get().is_null());
        let next = prev.next.get();
        header.prev.set(prev);
        header.next.set(next);
        unsafe {
            // safety: The linked list is maintained, and pointers are valid.
            (*next).prev.set(header);
            // safety: To access vtable pointer. Test by test_gc_header_value.
            let fat_ptr: [*mut (); 2] = mem::transmute(value);
            header.ccdyn_vptr = fat_ptr[1];
        }
        prev.next.set(header);
    }

    #[inline]
    fn remove(header: &Self::Header) {
        let _linked_list_lock = header.linked_list_lock.lock();
        let header: &Header = header;
        debug_assert!(!collect::is_collecting(header));
        debug_assert!(!header.next.get().is_null());
        debug_assert!(!header.prev.get().is_null());
        let next = header.next.get();
        let prev = header.prev.get();
        // safety: The linked list is maintained. Pointers in it are valid.
        unsafe {
            (*prev).next.set(next);
            (*next).prev.set(prev);
        }
        header.next.set(std::ptr::null_mut());
    }

    #[inline]
    fn new_ref_count(&self, tracked: bool) -> Self::RefCount {
        ThreadedRefCount::new(tracked, self.collector_lock.clone())
    }

    fn empty_header(&self) -> Self::Header {
        let linked_list_lock = self.list.linked_list_lock.clone();
        Self::Header {
            linked_list_lock,
            next: Cell::new(std::ptr::null()),
            prev: Cell::new(std::ptr::null()),
            ccdyn_vptr: CcDummy::ccdyn_vptr(),
        }
    }
}

impl Default for ThreadedObjectSpace {
    /// Constructs an empty [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html).
    fn default() -> Self {
        let linked_list_lock = Arc::new(Mutex::new(()));
        let pinned = Box::pin(Header {
            prev: Cell::new(std::ptr::null()),
            next: Cell::new(std::ptr::null()),
            ccdyn_vptr: CcDummy::ccdyn_vptr(),
            linked_list_lock,
        });
        let header: &Header = &pinned;
        header.prev.set(header);
        header.next.set(header);
        ThreadedObjectSpace {
            list: pinned,
            collector_lock: Default::default(),
        }
    }
}

impl ThreadedObjectSpace {
    /// Count objects tracked by this
    /// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html).
    pub fn count_tracked(&self) -> usize {
        let _linked_list_lock = self.list.linked_list_lock.lock();
        let list: &Header = &self.list;
        let mut count = 0;
        collect::visit_list(list, |_| count += 1);
        count
    }

    /// Collect cyclic garbage tracked by this
    /// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html).
    /// Return the number of objects collected.
    pub fn collect_cycles(&self) -> usize {
        // Wait for complex operations (drop). Block operations (drop, deref).
        let collector_lock = self.collector_lock.write();
        // Block linked list changes (create, remove).
        let linked_list_lock = self.list.linked_list_lock.lock();
        debug::log(|| ("ThreadedObjectSpace", "start collect_cycles"));
        let list: &Header = &self.list;
        let result = collect::collect_list(list, (linked_list_lock, collector_lock));
        debug::log(|| ("ThreadedObjectSpace", "end collect_cycles"));
        result
    }

    /// Constructs a new [`ThreadedCc<T>`](type.ThreadedCc.html) in this
    /// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html).
    ///
    /// The returned object should not refer to
    /// [`ThreadedCc<T>`](type.ThreadedCc.html) created by a different
    /// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html).
    /// Otherwise the collector might fail to collect cycles.
    ///
    /// The type `T` needs to be `Send + Sync`. This is because the
    /// [`ThreadedObjectSpace`](struct.ThreadedObjectSpace.html) is
    /// `Send` and `Sync`. The collector can run in a different
    /// thread to access `ThreadedCc<T>`, which needs to be
    /// `Send + Sync` to be safely accessed by the collector.
    ///
    /// ```compile_fail
    /// use gcmodule::{ThreadedObjectSpace, ThreadedCc, Cc};
    /// let cc = Cc::new(5);
    /// let space = ThreadedObjectSpace::default();
    /// # Does not compile since Cc is not Send.
    /// let tcc: ThreadedCc<Cc<_>> = space.create(cc);
    /// ```
    pub fn create<T: Trace + Send + Sync>(&self, value: T) -> ThreadedCc<T> {
        let _linked_list_lock = self.list.linked_list_lock.lock();
        ThreadedCc::new_in_space(value, self)
    }
}

impl Linked for Header {
    #[inline]
    fn next(&self) -> *const Self {
        self.next.get()
    }
    #[inline]
    fn prev(&self) -> *const Self {
        self.prev.get()
    }
    #[inline]
    fn set_prev(&self, other: *const Self) {
        self.prev.set(other)
    }
    #[inline]
    fn value(&self) -> &dyn CcDyn {
        // safety: To build trait object from self and vtable pointer.
        // Test by test_gc_header_value_consistency().
        unsafe {
            let fat_ptr: (*const (), *const ()) =
                ((self as *const Self).offset(1) as _, self.ccdyn_vptr);
            mem::transmute(fat_ptr)
        }
    }
}
