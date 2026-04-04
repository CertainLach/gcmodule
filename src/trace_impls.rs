use std::borrow::{Borrow, BorrowMut};
use std::ops::{Deref, DerefMut};

use crate::Acyclic;
use crate::trace::{Trace, Tracer};

/// Mark types as acyclic. Opt-out the cycle collector.
///
/// See [`Trace::is_type_tracked`](trait.Trace.html#method.is_type_tracked) for details.
/// In general, types including trait objects (directly or indirectly) should
/// not be acyclic.
///
/// ## Examples
///
/// ```
/// use jrsonnet_gcmodule::trace_acyclic;
///
/// struct X(u32);
/// struct Y(String);
/// struct Z<T>(fn (T));
///
/// trace_acyclic!(X);
/// trace_acyclic!(Y);
/// trace_acyclic!(<T> Z<T>);
/// ```
#[macro_export]
macro_rules! trace_acyclic {
    ( <$( $g:ident ),*> $( $t: tt )* ) => {
        impl<$( $g: 'static + $crate::Acyclic ),*> $crate::Trace for $($t)* {
            #[inline]
            fn is_type_tracked() -> bool where Self: Sized { false }
        }
        unsafe impl<$( $g: 'static + $crate::Acyclic ),*> $crate::Acyclic for $($t)* {}
    };
    ( $( $t: ty ),* ) => {
        $( trace_acyclic!(<> $t); )*
    };
}

/// Implement [`Trace`](trait.Trace.html) for simple container types.
///
/// ## Examples
///
/// ```
/// use jrsonnet_gcmodule::Trace;
/// use jrsonnet_gcmodule::trace_fields;
///
/// struct X<T1, T2> { a: T1, b: T2 };
/// struct Y<T>(Box<T>);
/// struct Z(Box<dyn Trace>);
///
/// trace_fields!(
///     X<T1, T2> { a: T1, b: T2 }
///     Y<T> { 0: T }
///     Z { 0 }
/// );
/// ```
#[macro_export]
macro_rules! trace_fields {
    ( $( $type:ty { $( $field:tt $(: $tp:ident )? ),* } )* ) => {
        $(
            impl< $( $( $tp: $crate::Trace )? ),* > $crate::Trace for $type {
                fn trace(&self, tracer: &mut $crate::Tracer) {
                    let _ = tracer;
                    $( (&self . $field ).trace(tracer); )*
                }
                #[inline]
                fn is_type_tracked() -> bool {
                    $( $( if $tp::is_type_tracked() { return true } )? )*
                    false
                }
            }
            unsafe impl< $( $( $tp: $crate::Acyclic )? ),* > $crate::Acyclic for $type {}
        )*
    };
}

trace_acyclic!(
    bool, char, f32, f64, i16, i32, i64, i8, isize, u16, u32, u64, u8, usize
);
trace_acyclic!(());
trace_acyclic!(String, &'static str);

mod tuples {
    trace_fields!(
        (A,) { 0: A }
        (A, B,) { 0: A, 1: B }
        (A, B, C,) { 0: A, 1: B, 2: C }
        (A, B, C, D,) { 0: A, 1: B, 2: C, 3: D }
        (A, B, C, D, E,) { 0: A, 1: B, 2: C, 3: D, 4: E }
        (A, B, C, D, E, F,) { 0: A, 1: B, 2: C, 3: D, 4: E, 5: F }
        (A, B, C, D, E, F, G,) { 0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G }
    );
}

mod borrow {
    use super::*;
    use std::borrow::Cow;

    impl<T: ToOwned + ?Sized> Trace for Cow<'static, T>
    where
        T::Owned: Trace,
    {
        fn trace(&self, tracer: &mut Tracer) {
            if let Cow::Owned(v) = self {
                v.trace(tracer)
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::Owned::is_type_tracked()
        }
    }
    unsafe impl<T: ToOwned + ?Sized> Acyclic for Cow<'static, T> where T::Owned: Acyclic {}
}

mod boxed {
    use super::*;
    impl<T: Trace> Trace for Box<T> {
        fn trace(&self, tracer: &mut Tracer) {
            self.as_ref().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for Box<T> {}
}

mod cell {
    use super::*;
    use std::cell::{Cell, OnceCell, RefCell};

    impl<T: Copy + Trace> Trace for Cell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            self.get().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic + Copy> Acyclic for Cell<T> {}

    impl<T: Trace> Trace for RefCell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // If the RefCell is currently borrowed we
            // assume there's an outstanding reference to this
            // cycle so it's ok if we don't trace through it.
            // If the borrow gets leaked somehow then we're going
            // to leak the cycle.
            if let Ok(x) = self.try_borrow() {
                x.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for RefCell<T> {}

    impl<T: Trace> Trace for OnceCell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            if let Some(x) = self.get() {
                x.trace(tracer)
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for OnceCell<T> {}
}

#[cfg(feature = "im-rc")]
mod im_collections {
    use super::*;
    use im_rc::{HashMap, HashSet, OrdMap, OrdSet, Vector};
    impl<T: Trace + Clone> Trace for Vector<T> {
        fn trace(&self, tracer: &mut Tracer) {
            let _ = tracer;
            for v in self.iter() {
                v.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool
        where
            Self: Sized,
        {
            T::is_type_tracked()
        }
    }
    impl<T: Trace + Clone, H: 'static> Trace for HashSet<T, H> {
        fn trace(&self, tracer: &mut Tracer) {
            let _ = tracer;
            for v in self.iter() {
                v.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool
        where
            Self: Sized,
        {
            T::is_type_tracked()
        }
    }
    impl<T: Trace + Clone + Ord> Trace for OrdSet<T> {
        fn trace(&self, tracer: &mut Tracer) {
            let _ = tracer;
            for v in self.iter() {
                v.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool
        where
            Self: Sized,
        {
            T::is_type_tracked()
        }
    }
    impl<K: Trace + Clone, V: Trace + Clone> Trace for HashMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            let _ = tracer;
            for (k, v) in self.iter() {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool
        where
            Self: Sized,
        {
            K::is_type_tracked() || V::is_type_tracked()
        }
    }
    impl<K: Trace + Clone + Ord, V: Trace + Clone> Trace for OrdMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            let _ = tracer;
            for (k, v) in self.iter() {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool
        where
            Self: Sized,
        {
            K::is_type_tracked() || V::is_type_tracked()
        }
    }
}

mod collections {
    use super::*;
    use std::collections::{BTreeMap, HashMap, HashSet, LinkedList, VecDeque};
    use std::hash::Hash;

    impl<K: Trace, V: Trace> Trace for BTreeMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            for (k, v) in self {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            K::is_type_tracked() || V::is_type_tracked()
        }
    }
    unsafe impl<K: Acyclic, V: Acyclic> Acyclic for BTreeMap<K, V> {}

    impl<K: Eq + Hash + Trace, H: 'static> Trace for HashSet<K, H> {
        fn trace(&self, tracer: &mut Tracer) {
            for k in self {
                k.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            K::is_type_tracked()
        }
    }
    unsafe impl<K: Acyclic + Hash + Eq, H: 'static> Acyclic for HashSet<K, H> {}

    impl<K: Eq + Hash + Trace, V: Trace, H: 'static> Trace for HashMap<K, V, H> {
        fn trace(&self, tracer: &mut Tracer) {
            for (k, v) in self {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            K::is_type_tracked() || V::is_type_tracked()
        }
    }
    unsafe impl<K: Acyclic + Hash + Eq, V: Acyclic, H: 'static> Acyclic for HashMap<K, V, H> {}

    impl<T: Trace> Trace for LinkedList<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for LinkedList<T> {}

    impl<T: Trace> Trace for VecDeque<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for VecDeque<T> {}
}

mod rustc_hash {}

mod vec {
    use super::*;
    impl<T: Trace> Trace for Vec<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for Vec<T> {}
}

mod slice {
    use crate::{Acyclic, Trace, Tracer};

    impl<T: Trace> Trace for [T] {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }
    }
    unsafe impl<T: Acyclic> Acyclic for [T] {}
}

// See https://github.com/rust-lang/rust/issues/56105#issuecomment-465709105
#[allow(unknown_lints)]
#[allow(coherence_leak_check)]
mod func {
    trace_acyclic!(<X> fn() -> X);

    trace_acyclic!(<A, X> fn(&A) -> X);
    trace_acyclic!(<A, X> fn(A) -> X);

    trace_acyclic!(<A, B, X> fn(&A, &B) -> X);
    trace_acyclic!(<A, B, X> fn(A, &B) -> X);
    trace_acyclic!(<A, B, X> fn(&A, B) -> X);
    trace_acyclic!(<A, B, X> fn(A, B) -> X);

    trace_acyclic!(<A, B, C, X> fn(&A, &B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, &B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, &B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, &B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, B, C) -> X);

    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, C, D) -> X);

    trace_acyclic!(<A, B, C, D, E, X> fn(A, B, C, D, E) -> X);
    trace_acyclic!(<A, B, C, D, E, F, X> fn(A, B, C, D, E, F) -> X);
}

mod ffi {
    use std::ffi;

    trace_acyclic!(ffi::CString, ffi::NulError, ffi::OsString);
}

mod net {
    use std::net;

    trace_acyclic!(
        net::AddrParseError,
        net::Ipv4Addr,
        net::Ipv6Addr,
        net::SocketAddrV4,
        net::SocketAddrV6,
        net::TcpListener,
        net::TcpStream,
        net::UdpSocket
    );
}

mod option {
    use super::*;

    impl<T: Trace> Trace for Option<T> {
        fn trace(&self, tracer: &mut Tracer) {
            if let Some(ref t) = *self {
                t.trace(tracer);
            }
        }

        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic> Acyclic for Option<T> {}
}

mod path {
    use std::path;

    trace_acyclic!(path::PathBuf);
}

mod process {
    use std::process;

    trace_acyclic!(
        process::Child,
        process::ChildStderr,
        process::ChildStdin,
        process::ChildStdout,
        process::Command,
        process::ExitStatus,
        process::Output,
        process::Stdio
    );
}

mod rc {
    use std::rc::{Rc, Weak};

    use crate::Acyclic;

    trace_acyclic!(<T> Rc<T> where T: Acyclic + ?Sized);
    trace_acyclic!(<T> Weak<T>);
}

mod result {
    use super::*;

    impl<T: Trace, U: Trace> Trace for Result<T, U> {
        fn trace(&self, tracer: &mut Tracer) {
            match *self {
                Ok(ref t) => t.trace(tracer),
                Err(ref u) => u.trace(tracer),
            }
        }

        fn is_type_tracked() -> bool {
            T::is_type_tracked() || U::is_type_tracked()
        }
    }
    unsafe impl<T: Acyclic, U: Acyclic> Acyclic for Result<T, U> {}
}

mod sync {
    use super::*;
    use std::sync;

    // See comment in Mutex for why this is acyclic.
    trace_acyclic!(<T> sync::Arc<T>);

    impl<T: Trace> Trace for sync::Mutex<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // For single-thread collector (ObjectSpace):
            // Locking is optional. See RefCell.
            //
            // For multi-thread collector (ThreadedObjectSpace):
            // `ThreadedCcRef` is expected to be the only way to access a `T`
            // stored in `ThreadedCc<T>`. `ThreadedCcRef` takes a lock so
            // collector does not run. When the collector runs, `ThreadedCcRef`
            // are dropped so locks are released.
            // A special is when `T` is `Arc<Mutex<M>>`. It allows mutating `M`
            // without going through `ThreadedCcRef`. This is handled by marking
            // `Arc` as acyclic. The collector only cares about `trace`, and
            // `trace` result for an `Arc` cannot be changed by another thread,
            // even if `M` is mutable.
            if let Ok(x) = self.try_lock() {
                x.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }

    impl<T: Trace> Trace for sync::RwLock<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // See Mutex for why locking is optional.
            //
            // If read or write locks are already taken, that indicates
            // outstanding references that keeps the objects alive.
            if let Ok(x) = self.try_write() {
                x.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }
    }
}

mod thread {
    use std::thread;

    trace_acyclic!(<T> thread::JoinHandle<T>);
    trace_acyclic!(<T> thread::LocalKey<T>);
    trace_acyclic!(thread::Thread);
}

mod phantom {
    use super::*;
    use std::marker::PhantomData;
    impl<T: 'static> Trace for PhantomData<T> {
        fn trace(&self, _tracer: &mut Tracer) {}

        #[inline]
        fn is_type_tracked() -> bool {
            false
        }
    }
    unsafe impl<T: 'static> Acyclic for PhantomData<T> {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cc;
    use std::cell::{Cell, RefCell};

    #[test]
    fn test_is_type_tracked() {
        assert!(!u8::is_type_tracked());
        assert!(!<f32 as Trace>::is_type_tracked());
        assert!(!String::is_type_tracked());
        assert!(!Option::<u32>::is_type_tracked());
        assert!(!Vec::<u8>::is_type_tracked());
        assert!(!<(bool, f64)>::is_type_tracked());
        assert!(!Cell::<u32>::is_type_tracked());
        assert!(!RefCell::<String>::is_type_tracked());
        assert!(TraceBox::<dyn Trace>::is_type_tracked());
        assert!(RefCell::<TraceBox::<dyn Trace>>::is_type_tracked());
        assert!(RefCell::<Vec::<TraceBox::<dyn Trace>>>::is_type_tracked());
        assert!(Vec::<RefCell::<TraceBox::<dyn Trace>>>::is_type_tracked());
        assert!(!Cc::<u8>::is_type_tracked());
        assert!(!Vec::<Cc::<u8>>::is_type_tracked());

        assert!(!<fn(u8) -> u8>::is_type_tracked());
        assert!(!<fn(&u8) -> u8>::is_type_tracked());
    }

    #[test]
    fn test_is_cyclic_type_tracked() {
        type C2 = RefCell<Option<Cc<Box<S2>>>>;
        struct S2(C2);
        impl Trace for S2 {
            fn trace(&self, t: &mut Tracer) {
                self.0.trace(t);
            }
            fn is_type_tracked() -> bool {
                // C2::is_type_tracked() can cause an infinite loop.
                true
            }
        }

        assert!(S2::is_type_tracked());
    }
}

/// Box, that assumes inner type is tracked
///
/// Exists because plain `Box<T>` only accepts sized `T`
#[derive(Debug, Clone)]
pub struct TraceBox<T: ?Sized>(pub Box<T>);

impl<T: ?Sized + Trace> Trace for TraceBox<T> {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.0.trace(tracer);
    }

    fn is_type_tracked() -> bool {
        true
    }
}
unsafe impl<T: ?Sized> Acyclic for TraceBox<T> where T: Acyclic {}

impl<T: ?Sized> From<Box<T>> for TraceBox<T> {
    fn from(inner: Box<T>) -> Self {
        Self(inner)
    }
}

impl<T: ?Sized> Deref for TraceBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Trace + ?Sized> DerefMut for TraceBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: ?Sized> Borrow<T> for TraceBox<T> {
    fn borrow(&self) -> &T {
        &self.0
    }
}

impl<T: ?Sized> BorrowMut<T> for TraceBox<T> {
    fn borrow_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: ?Sized> AsRef<T> for TraceBox<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: ?Sized> AsMut<T> for TraceBox<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
