#![deny(missing_docs)]
#![cfg_attr(feature = "nightly", feature(coerce_unsized), feature(unsize))]
#![cfg_attr(all(feature = "debug", feature = "nightly"), feature(specialization))]

//! Reference cycle garbage collection inspired by
//! [cpython](https://github.com/python/cpython/).
//!
//! The type [`Cc<T>`](type.Cc.html) provides shared ownership of a value of type `T`,
//! similar to `std::rc::Rc<T>`. Unlike `Rc<T>`, [`collect_thread_cycles`](fn.collect_thread_cycles.html)
//! can be used to drop unreachable values that form circular references.
//!
//! # Examples
//!
//! ## Cloning references
//!
//! Similar to `Rc<T>`, use `clone()` to get cloned references.
//!
//! ```
//! use jrsonnet_gcmodule::Cc;
//! let foo = Cc::new(vec![1, 2, 3]);
//! let foo_cloned = foo.clone();
//!
//! // foo and foo_cloned both point to the same `vec![1, 2, 3]`.
//! assert!(std::ptr::eq(&foo[0], &foo_cloned[0]));
//! ```
//!
//! ## Collecting cycles
//!
//! Use [`collect_thread_cycles()`](fn.collect_thread_cycles.html) to collect
//! thread-local garbage.
//!
//! Use [`count_thread_tracked()`](fn.count_thread_tracked.html) to count how
//! many objects are tracked by the collector.
//!
//! ```
//! use jrsonnet_gcmodule::{Cc, Trace, TraceBox};
//! use std::cell::RefCell;
//! {
//!     type List = Cc<RefCell<Vec<TraceBox<dyn Trace>>>>;
//!     let a: List = Default::default();
//!     let b: List = Default::default();
//!     a.borrow_mut().push(TraceBox(Box::new(b.clone())));
//!     b.borrow_mut().push(TraceBox(Box::new(a.clone())));
//! }
//!
//! // a and b form circular references. The objects they point to are not
//! // dropped automatically, despite both variables run out of scope.
//!
//! assert_eq!(jrsonnet_gcmodule::count_thread_tracked(), 2);   // 2 values are tracked.
//! assert_eq!(jrsonnet_gcmodule::collect_thread_cycles(), 2);  // This will drop a and b.
//! assert_eq!(jrsonnet_gcmodule::count_thread_tracked(), 0);   // no values are tracked.
//! ```
//!
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
//!
//! ## Defining new types
//!
//! [`Cc<T>`](type.Cc.html) requires [`Trace`](trait.Trace.html) implemented
//! for `T` so the collector knows how values are referred. That can usually
//! be done by `#[derive(Trace)]`.
//!
//! ### Acyclic types
//!
//! If a type is acyclic (cannot form reference circles about [`Cc`](type.Cc.html)),
//! [`Trace::is_type_tracked()`](trait.Trace.html#method.is_type_tracked) will return `false`.
//!
//! ```
//! use jrsonnet_gcmodule::{Cc, Trace};
//!
//! #[derive(Trace)]
//! struct Foo(String);
//!
//! #[derive(Trace)]
//! struct Bar;
//!
//! assert!(!Foo::is_type_tracked()); // Acyclic
//! assert!(!Bar::is_type_tracked()); // Acyclic
//!
//! let foo = Cc::new(Foo("abc".to_string()));
//! let bar = Cc::new(Bar);
//! let foo_cloned = foo.clone(); // Share the same `"abc"` with `foo`.
//! assert_eq!(jrsonnet_gcmodule::count_thread_tracked(), 0); // The collector tracks nothing.
//! drop(foo); // The ref count of `"abc"` drops from 2 to 1.
//! drop(foo_cloned); // `"abc"` will be dropped here..
//! # drop(bar);
//! ```
//!
//! ### Container types
//!
//! Whether a container type is acyclic or not depends on its fields. Usually,
//! types without referring to trait objects or itself are considered acyclic.
//!
//! ```
//! use jrsonnet_gcmodule::{Cc, Trace, TraceBox};
//!
//! #[derive(Trace)]
//! struct Foo<T1: Trace, T2: Trace>(T1, T2, u8);
//!
//! // `a` is not tracked - types are acyclic.
//! let a = Cc::new(Foo(Foo(Cc::new(1), 2, 3), Cc::new("abc"), 10));
//! assert_eq!(jrsonnet_gcmodule::count_thread_tracked(), 0);
//!
//! // `b` is tracked because it contains a trait object.
//! let b = Cc::new(Foo(TraceBox(Box::new(1) as Box<dyn Trace>), 2, 3));
//! assert_eq!(jrsonnet_gcmodule::count_thread_tracked(), 1);
//! ```
//!
//! The `#[skip_trace]` attribute can be used to skip tracking specified fields
//! in a structure.
//!
//! ```
//! use jrsonnet_gcmodule::{Cc, Trace};
//!
//! struct AlienStruct; // Does not implement Trace
//!
//! #[derive(Trace)]
//! struct Foo {
//!     field: String,
//!
//!     #[trace(skip)]
//!     alien: AlienStruct, // Field skipped in Trace implementation.
//! }
//! ```
//!
//! ### Weak references
//!
//! Similar to `std::rc::Rc`, use [`Cc::downgrade`](struct.RawCc.html#method.downgrade)
//! to create weak references. Use [`Weak::upgrade`](struct.RawWeak.html#method.upgrade)
//! to test if the value is still alive and to access the value. For example:
//!
//! ```
//! use jrsonnet_gcmodule::{Cc, Weak};
//!
//! let value = Cc::new("foo");
//! let weak: Weak<_> = value.downgrade();
//! assert_eq!(*weak.upgrade().unwrap(), "foo");
//! drop(value);
//! assert!(weak.upgrade().is_none());  // Cannot upgrade after dropping value
//! ```
//!
//! # Technical Details
//!
//! ## Memory Layouts
//!
//! [`Cc<T>`](type.Cc.html) uses different memory layouts depending on `T`.
//!
//! ### Untracked types
//!
//! If [`<T as Trace>::is_type_tracked()`](trait.Trace.html#method.is_type_tracked)
//! returns `false`, the layout is similar to `Rc<T>`:
//!
//! ```plain,ignore
//! Shared T                    Pointer
//! +-------------------+     .-- Cc<T>
//! | ref_count: usize  | <--<
//! | weak_count: usize |     '-- Cc<T>::clone()
//! |-------------------|
//! | T (shared data)   | <--- Cc<T>::deref()
//! +-------------------+
//! ```
//!
//! ### Tracked types
//!
//! If [`<T as Trace>::is_type_tracked()`](trait.Trace.html#method.is_type_tracked)
//! returns `true`, the layout has an extra `GcHeader` that makes the value visible
//! in a thread-local linked list:
//!
//! ```plain,ignore
//! Shared T with GcHeader
//! +-------------------+
//! | gc_prev: pointer  | ---> GcHeader in a linked list.
//! | gc_next: pointer  |
//! | vptr<T>: pointer  | ---> Pointer to the `&T as &dyn Trace` virtual table.
//! |-------------------|
//! | ref_count: usize  | <--- Cc<T>
//! | weak_count: usize |
//! | ----------------- |
//! | T (shared data)   | <--- Cc<T>::deref()
//! +-------------------+
//! ```
//!
//! ## Incorrect `Trace` implementation
//!
//! While most public APIs provided by this library looks safe, incorrectly
//! implementing the [`Trace`](trait.Trace.html) trait has consequences.
//!
//! This library should cause no undefined behaviors (UB) even with incorrect
//! [`Trace`](trait.Trace.html) implementation on _debug_ build.
//!
//! Below are some consequences of a wrong [`Trace`](trait.Trace.html)
//! implementation.
//!
//! ### Memory leak
//!
//! If [`Trace::trace`](trait.Trace.html#method.trace) does not visit all
//! referred values, the collector might fail to detect cycles, and take
//! no actions on cycles. That causes memory leak.
//!
//! Note: there are other ways to cause memory leak unrelated to an incorrect
//! [`Trace`](trait.Trace.html) implementation. For example, forgetting
//! to call collect functions can cause memory leak. When using the advanced
//! [`ObjectSpace`](struct.ObjectSpace.html) APIs, objects in one space
//! referring to objects in a different space can cause memory leak.
//!
//! ### Panic
//!
//! If [`Trace::trace`](trait.Trace.html#method.trace) visits more values
//! than it should (for example, visit indirectly referred values, or visit
//! a directly referred value multiple times), the collector can detect
//! such issues and panic the thread with the message:
//!
//! ```plain,ignore
//! bug: unexpected ref-count after dropping cycles
//! This usually indicates a buggy Trace or Drop implementation.
//! ```
//!
//! ### Undefined behavior (UB)
//!
//! After the above panic (`bug: unexpected ref-count after dropping cycles`),
//! dereferencing a garbage-collected [`Cc<T>`](type.Cc.html) will trigger
//! `panic!` or UB depending on whether it's a debug build or not.
//!
//! On debug build, sanity checks are added at `Cc::<T>::deref()`.
//! It will panic if `T` was garbage-collected:
//!
//! ```plain,ignore
//! bug: accessing a dropped CcBox detected
//! ```
//!
//! In other words, no UB on debug build.
//!
//! On release build the dereference would access dropped values, which is an
//! undefined behavior. Again, the UB can only happen if the [`Trace::trace`](trait.Trace.html#method.trace)
//! is implemented wrong, and panic will happen before the UB.

extern crate self as jrsonnet_gcmodule;

mod cc;
mod cc_impls;
mod collect;
#[cfg(test)]
mod debug;
mod ref_count;
#[cfg(feature = "sync")]
mod sync;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod trace;
mod trace_impls;

pub use trace_impls::TraceBox;

pub use cc::{Cc, RawCc, RawWeak, Weak};
pub use collect::{
    collect_thread_cycles, count_thread_tracked, with_thread_object_space, ObjectSpace,
};
pub use trace::{Trace, Tracer};

#[cfg(feature = "sync")]
pub use sync::{collect::ThreadedObjectSpace, ThreadedCc, ThreadedCcRef};

/// Derive [`Trace`](trait.Trace.html) implementation for a structure.
///
/// # Examples
///
/// ```
/// use jrsonnet_gcmodule::{Cc, Trace};
///
/// #[derive(Trace)]
/// struct S1(u32, String);
///
/// #[derive(Trace)]
/// struct S2<T1: Trace, T2: Trace>(T1, T2, u8);
///
/// #[derive(Trace)]
/// struct S3<T: Trace> {
///     a: S1,
///     b: Option<S2<T, u8>>,
///
///     #[trace(skip)]
///     c: AlienStruct,  // c is not tracked by the collector.
/// }
///
/// struct AlienStruct;
/// ```
#[cfg(feature = "derive")]
pub use jrsonnet_gcmodule_derive::Trace;

#[cfg(not(test))]
mod debug {
    #[cfg(any(feature = "debug", test))]
    thread_local!(pub(crate) static NEXT_DEBUG_NAME: std::cell::Cell<usize> = Default::default());
    #[cfg(any(feature = "debug", test))]
    thread_local!(pub(crate) static GC_DROPPING: std::cell::Cell<bool> = Cell::new(false));
    pub(crate) fn log<S1: ToString, S2: ToString>(func: impl Fn() -> (S1, S2)) {
        if cfg!(feature = "debug") {
            let (name, message) = func();
            eprintln!("[gc] {} {}", name.to_string(), message.to_string());
        }
    }
}

/// Whether the `debug` feature is enabled.
pub const DEBUG_ENABLED: bool = cfg!(feature = "debug");

/// Jrsonnet golang bindings require that it is possible to move jsonnet
/// VM between OS threads, and this is not possible due to usage of
/// `thread_local`. Instead, there is two methods added, one should be
/// called at the end of current thread work, and one that should be
/// used when using other thread.
// It won't be able to preserve debug state.
#[cfg(not(any(test, feature = "debug")))]
pub mod interop {
    use std::mem;
    use std::pin::Pin;

    use crate::collect::{new_gc_list, OwnedGcHeader, THREAD_OBJECT_SPACE};

    /// Type-erased gc object list
    pub enum GcState {}

    type UnerasedState = OwnedGcHeader;

    /// Dump current interned string pool, to be restored by
    /// `reenter_thread`
    ///
    /// # Safety
    ///
    /// Current thread gc becomes broken after this call, you should not use gc after this
    /// call, and before `reenter_thread` call.
    pub unsafe fn exit_thread() -> *mut GcState {
        let object_list: UnerasedState = THREAD_OBJECT_SPACE
            .with(|space| mem::replace(&mut *space.list.borrow_mut(), new_gc_list()));
        Box::into_raw(Box::new(object_list)).cast()
    }

    /// Reenter thread, using state dumped by `exit_thread`.
    ///
    /// # Safety
    ///
    /// `state` should be acquired from `exit_thread`, it is not allowed
    /// to reuse state to reenter multiple threads.
    pub unsafe fn reenter_thread(state: *mut GcState) {
        let ptr: *mut UnerasedState = state.cast();
        // SAFETY: ptr is an unique state per method safety requirements.
        let ptr: Box<UnerasedState> = unsafe { Box::from_raw(ptr) };
        let ptr: UnerasedState = *ptr;
        THREAD_OBJECT_SPACE.with(|space| {
            let _ = mem::replace(&mut *space.list.borrow_mut(), ptr);
        });
    }
}

#[cfg(test)]
mod dyn_cc {
    use crate::Trace;
    use std::fmt::Debug;

    use crate::cc_dyn;

    #[derive(Debug, Trace)]
    struct Test {
        a: String,
    }

    trait DebugAndTrace: Debug + Trace {}
    impl<T> DebugAndTrace for T where T: Debug + Trace {}
    cc_dyn!(
        #[derive(Debug)]
        CcDebugAndTrace,
        DebugAndTrace
    );

    trait BorrowTy<O> {
        fn borrow_ty(&self) -> &O;
    }
    cc_dyn!(CcBorrowTy<O>, BorrowTy<O>);
    cc_dyn!(
        CcBorrowTyAlsoDebug<O: Debug>,
        BorrowTy<O>
    );

    #[derive(Trace)]
    struct Borrowable<T: Trace> {
        v: T,
    }
    impl<T: Trace> BorrowTy<T> for Borrowable<T> {
        fn borrow_ty(&self) -> &T {
            &self.v
        }
    }

    #[test]
    fn test_dyn() {
        let test = Test {
            a: "hello".to_owned(),
        };

        let dynccgeneric = CcBorrowTy::new(Borrowable { v: 1u32 });
        let v = dynccgeneric.0.borrow_ty();
        assert_eq!(*v, 1);

        let dynccgeneric = CcBorrowTyAlsoDebug::new(Borrowable { v: 1u32 });
        let v = dynccgeneric.0.borrow_ty();
        assert_eq!(*v, 1);

        let dyncc = CcDebugAndTrace::new(test);
        assert_eq!(
            format!("{dyncc:?}"),
            "CcDebugAndTrace(Cc(Test { a: \"hello\" }))"
        );
        let dyncc_is_trace = dyncc;
        assert_eq!(
            format!("{dyncc_is_trace:?}"),
            "CcDebugAndTrace(Cc(Test { a: \"hello\" }))"
        );
        let dyncc_is_trace_as_dyn = CcDebugAndTrace::new(dyncc_is_trace);
        assert_eq!(
            format!("{dyncc_is_trace_as_dyn:?}"),
            "CcDebugAndTrace(Cc(CcDebugAndTrace(Cc(Test { a: \"hello\" }))))"
        );
    }
}
