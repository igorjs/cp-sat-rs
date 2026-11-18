// Copyright 2010-2025 Google LLC
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Ported from ortools/sat/model.h at commit c931f5f7. Changed: translated from C++ to Rust.

//! The container every solver component is fetched from.
//!
//! The model owns each component in a heap box that never moves and hands out a `Handle`,
//! a pointer that derefs to `&T`. A component keeps handles to other components as
//! upstream keeps raw pointers, and calls them while it is itself running. No `&mut` of a
//! component is ever made: a method takes `&self` and keeps mutable state in per-field
//! `Cell` or `DebugRefCell`. Two rules keep this sound, and a debug build checks both:
//!
//! - Use a handle only while its component is alive. Components drop in reverse order of
//!   creation, so a `Drop` impl may use the handles a component got in `new`, never one
//!   it got later.
//! - Drop a `DebugRefCell` borrow before any call that may come back to the same field.
//!
//! A release build does not check them, and breaking one is undefined behaviour.
//!
//! A model is not `Send`. A worker thread builds its own model and keeps it for life. An
//! object that workers share is an `Arc<T>` with `T: Send + Sync`, made by the thread that
//! starts the workers, and each worker registers a clone in its model with `register`.

use std::any::TypeId;
#[cfg(debug_assertions)]
use std::cell::Cell;
use std::cell::{RefCell, UnsafeCell};
use std::collections::HashMap;
use std::marker::PhantomPinned;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::NonNull;
#[cfg(debug_assertions)]
use std::rc::{Rc, Weak};
use std::sync::Arc;

/// A type that `Model` can build. It is the `T(Model* model)` constructor, or `T()` when
/// `new` ignores the model.
pub trait Component: Sized + 'static {
    fn new(model: &Model) -> Self;

    /// Runs once the object has its final address, before the model records it. Upstream
    /// constructors that pass `this` to another component, such as
    /// `model->GetOrCreate<SatSolver>()->AddPropagator(this)` (`sat/integer.h:558`), do
    /// that here.
    fn after_create(_this: &Handle<Self>, _model: &Model) {}
}

/// A pointer to a component, or to a field of one, as the raw pointer members upstream.
pub struct Handle<T: ?Sized> {
    ptr: NonNull<T>,
    #[cfg(debug_assertions)]
    alive: Weak<()>,
}

impl<T: ?Sized> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle {
            ptr: self.ptr,
            #[cfg(debug_assertions)]
            alive: self.alive.clone(),
        }
    }
}

impl<T: ?Sized> Deref for Handle<T> {
    type Target = T;

    fn deref(&self) -> &T {
        #[cfg(debug_assertions)]
        assert!(
            self.alive.strong_count() > 0,
            "a handle was used after its component was dropped"
        );
        // SAFETY: `ptr` points into an allocation that the model frees only when it drops
        // the component, and the rule of the module says the component is still alive.
        // Only shared references are ever made from it, so handles may alias.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ?Sized> Handle<T> {
    /// A handle to a field of the component, as `&modified_vars_` upstream
    /// (`sat/integer.cc:2512`). It lives as long as the component does.
    pub fn field<U: ?Sized>(&self, field: for<'a> fn(&'a T) -> &'a U) -> Handle<U> {
        Handle {
            ptr: NonNull::from(field(&**self)),
            #[cfg(debug_assertions)]
            alive: self.alive.clone(),
        }
    }

    #[doc(hidden)]
    pub fn as_non_null(&self) -> NonNull<T> {
        self.ptr
    }

    /// Used by `handle_as_dyn!`.
    ///
    /// # Safety
    ///
    /// `ptr` must be `self.as_non_null()` coerced to a trait object.
    #[doc(hidden)]
    pub unsafe fn with_coerced_ptr<U: ?Sized>(&self, ptr: NonNull<U>) -> Handle<U> {
        Handle {
            ptr,
            #[cfg(debug_assertions)]
            alive: self.alive.clone(),
        }
    }
}

/// Turns a `Handle<T>` into a handle of a trait object `T` implements, as `Rc` coerces.
/// Stable Rust lets no user type coerce, so a typed `let` does the coercion here.
#[macro_export]
macro_rules! handle_as_dyn {
    ($handle:expr, $target:ty) => {{
        let handle = &$handle;
        let ptr: ::std::ptr::NonNull<$target> = handle.as_non_null();
        // SAFETY: the typed `let` above compiles only for an unsized coercion of the same
        // pointer.
        unsafe { handle.with_coerced_ptr(ptr) }
    }};
}

/// A `RefCell` whose borrow flag exists in debug builds only. It holds the mutable state
/// of a component that is not `Copy`.
///
/// Drop a borrow before any call that may come back to the same field. A debug build
/// panics when two borrows overlap; in a release build the overlap is undefined behaviour.
#[derive(Default)]
pub struct DebugRefCell<T> {
    value: UnsafeCell<T>,
    #[cfg(debug_assertions)]
    flag: Cell<isize>,
}

pub struct DebugRef<'a, T> {
    cell: &'a DebugRefCell<T>,
}

pub struct DebugRefMut<'a, T> {
    cell: &'a DebugRefCell<T>,
}

impl<T> DebugRefCell<T> {
    pub fn new(value: T) -> Self {
        DebugRefCell {
            value: UnsafeCell::new(value),
            #[cfg(debug_assertions)]
            flag: Cell::new(0),
        }
    }

    pub fn borrow(&self) -> DebugRef<'_, T> {
        #[cfg(debug_assertions)]
        {
            assert!(
                self.flag.get() >= 0,
                "DebugRefCell already mutably borrowed"
            );
            self.flag.set(self.flag.get() + 1);
        }
        DebugRef { cell: self }
    }

    pub fn borrow_mut(&self) -> DebugRefMut<'_, T> {
        #[cfg(debug_assertions)]
        {
            assert!(self.flag.get() == 0, "DebugRefCell already borrowed");
            self.flag.set(-1);
        }
        DebugRefMut { cell: self }
    }
}

impl<T> Deref for DebugRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: by the rule of `DebugRefCell`, no `DebugRefMut` of the cell is alive.
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> Deref for DebugRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: by the rule of `DebugRefCell`, this is the only borrow of the cell.
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> DerefMut for DebugRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: by the rule of `DebugRefCell`, this is the only borrow of the cell.
        unsafe { &mut *self.cell.value.get() }
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for DebugRef<'_, T> {
    fn drop(&mut self) {
        self.cell.flag.set(self.cell.flag.get() - 1);
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for DebugRefMut<'_, T> {
    fn drop(&mut self) {
        self.cell.flag.set(0);
    }
}

/// An object the model frees when it is dropped.
struct Owned {
    ptr: NonNull<()>,
    free: unsafe fn(NonNull<()>),
    #[cfg(debug_assertions)]
    alive: Rc<()>,
}

/// A singleton: where it is, and in a debug build whether it is alive.
#[derive(Clone)]
struct Singleton {
    ptr: NonNull<()>,
    #[cfg(debug_assertions)]
    alive: Weak<()>,
}

/// # Safety
///
/// `ptr` comes from `Box::<T>::leak` and is freed once.
unsafe fn free_box<T>(ptr: NonNull<()>) {
    // SAFETY: by the contract of the function.
    drop(unsafe { Box::from_raw(ptr.cast::<T>().as_ptr()) });
}

/// # Safety
///
/// `ptr` comes from `Arc::<T>::into_raw` and is freed once.
unsafe fn free_arc<T>(ptr: NonNull<()>) {
    // SAFETY: by the contract of the function.
    drop(unsafe { Arc::from_raw(ptr.cast::<T>().as_ptr()) });
}

/// Owns everything related to a particular optimization model.
///
/// Components point into the model, so it never moves: it is built in a pinned box.
pub struct Model {
    name: String,
    // Map of the type to a "singleton" of that type.
    singletons: RefCell<HashMap<TypeId, Singleton>>,
    // The list of items to delete.
    cleanup_list: RefCell<Vec<Owned>>,
    #[cfg(debug_assertions)]
    alive: Rc<()>,
    _pinned: PhantomPinned,
}

impl Model {
    /// The upstream default constructor. A model has no `Default` impl, since it is never
    /// built by value.
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Pin<Box<Model>> {
        Model::new("")
    }

    /// Names the model for debugging or logging.
    pub fn new(name: impl Into<String>) -> Pin<Box<Model>> {
        Box::pin(Model {
            name: name.into(),
            singletons: RefCell::default(),
            cleanup_list: RefCell::default(),
            #[cfg(debug_assertions)]
            alive: Rc::new(()),
            _pinned: PhantomPinned,
        })
    }

    /// A handle to the model, for a component that keeps `Model* model_` as a member.
    pub fn handle(&self) -> Handle<Model> {
        Handle {
            // Every `Model` is built pinned in a box, so `self` does not move.
            ptr: NonNull::from(self),
            #[cfg(debug_assertions)]
            alive: Rc::downgrade(&self.alive),
        }
    }

    /// `model.Add(f)`: calls `f` with the model.
    pub fn add<T>(&self, f: impl FnOnce(&Model) -> T) -> T {
        f(self)
    }

    /// Similar to `add` but this is const.
    pub fn get<T>(&self, f: impl FnOnce(&Model) -> T) -> T {
        f(self)
    }

    /// Returns an object of type `T` that is unique to this model (like a "local"
    /// singleton). This returns an already created instance or creates a new one if needed.
    ///
    /// IMPORTANT: the `Component::new` functions shouldn't form a cycle between each other,
    /// otherwise this will crash the program.
    pub fn get_or_create<T: Component>(&self) -> Handle<T> {
        if let Some(found) = self.get_singleton::<T>() {
            return found;
        }

        // New element.
        let (new_t, owned) = self.construct::<T>();
        self.singletons
            .borrow_mut()
            .insert(TypeId::of::<T>(), singleton_of(&new_t));
        self.cleanup_list.borrow_mut().push(owned);
        new_t
    }

    /// Like `get_or_create` but does not create the object if it does not exist.
    pub fn get_singleton<T: 'static>(&self) -> Option<Handle<T>> {
        let found = self.singletons.borrow().get(&TypeId::of::<T>()).cloned()?;
        Some(Handle {
            ptr: found.ptr.cast::<T>(),
            #[cfg(debug_assertions)]
            alive: found.alive,
        })
    }

    /// Same as `get_singleton`; upstream returns a mutable pointer here.
    pub fn mutable<T: 'static>(&self) -> Option<Handle<T>> {
        self.get_singleton::<T>()
    }

    /// Gives ownership of an object to this model. It is destroyed when the model is.
    pub fn take_ownership<T: 'static>(&self, t: T) -> Handle<T> {
        let ptr = NonNull::from(Box::leak(Box::new(t)));
        let (handle, owned) = self.own(ptr, free_box::<T>);
        self.cleanup_list.borrow_mut().push(owned);
        handle
    }

    /// Returns a non-singleton object owned by the model and created with
    /// `Component::new`, as `get_or_create` creates one.
    pub fn create<T: Component>(&self) -> Handle<T> {
        let (new_t, owned) = self.construct::<T>();
        self.cleanup_list.borrow_mut().push(owned);
        new_t
    }

    /// Registers a class that will be "singleton" in the model. The model keeps a clone of
    /// the `Arc`, so the object lives at least as long as the model; it is destroyed when
    /// its last `Arc` is.
    ///
    /// It is an error to call this on an already registered class.
    pub fn register<T: 'static>(&self, non_owned_class: Arc<T>) {
        let type_id = TypeId::of::<T>();
        assert!(
            !self.singletons.borrow().contains_key(&type_id),
            "the type is already registered in the model"
        );
        let ptr =
            NonNull::new(Arc::into_raw(non_owned_class).cast_mut()).expect("an Arc is never null");
        let (handle, owned) = self.own(ptr, free_arc::<T>);
        self.singletons
            .borrow_mut()
            .insert(type_id, singleton_of(&handle));
        self.cleanup_list.borrow_mut().push(owned);
    }

    pub fn unregister<T: 'static>(&self) {
        self.singletons.borrow_mut().remove(&TypeId::of::<T>());
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Runs the construction steps that `get_or_create` and `create` share. The caller
    /// records the object in the cleanup list after `after_create`, as upstream records it
    /// after the constructor body.
    fn construct<T: Component>(&self) -> (Handle<T>, Owned) {
        let ptr = NonNull::from(Box::leak(Box::new(T::new(self))));
        let (new_t, owned) = self.own(ptr, free_box::<T>);
        T::after_create(&new_t, self);
        (new_t, owned)
    }

    fn own<T>(&self, ptr: NonNull<T>, free: unsafe fn(NonNull<()>)) -> (Handle<T>, Owned) {
        #[cfg(debug_assertions)]
        let alive = Rc::new(());
        let handle = Handle {
            ptr,
            #[cfg(debug_assertions)]
            alive: Rc::downgrade(&alive),
        };
        let owned = Owned {
            ptr: ptr.cast(),
            free,
            #[cfg(debug_assertions)]
            alive,
        };
        (handle, owned)
    }
}

fn singleton_of<T>(handle: &Handle<T>) -> Singleton {
    Singleton {
        ptr: handle.ptr.cast(),
        #[cfg(debug_assertions)]
        alive: handle.alive.clone(),
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // A component that reaches the model while it is dropped panics in a debug build.
        #[cfg(debug_assertions)]
        {
            self.alive = Rc::new(());
        }
        self.singletons.get_mut().clear();
        // The order of deletion seems to be platform dependent.
        // We force a reverse order on the cleanup vector.
        let cleanup_list = std::mem::take(self.cleanup_list.get_mut());
        for owned in cleanup_list.into_iter().rev() {
            #[cfg(debug_assertions)]
            drop(owned.alive);
            // SAFETY: `free` matches how `ptr` was made, and each entry is freed once.
            unsafe { (owned.free)(owned.ptr) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpsat_testutil::expect_death;
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    #[derive(Default)]
    struct A {
        name: DebugRefCell<String>,
    }

    impl Component for A {
        fn new(_model: &Model) -> Self {
            A::default()
        }
    }

    struct B {
        a: Handle<A>,
    }

    impl Component for B {
        fn new(model: &Model) -> Self {
            B {
                a: model.get_or_create::<A>(),
            }
        }
    }

    impl B {
        fn name(&self) -> String {
            self.a.name.borrow().clone()
        }
    }

    #[test]
    fn model_test_recursive_creation_test() {
        // Arrange
        let model = Model::default();

        // Act
        let b = model.get_or_create::<B>();
        *model.get_or_create::<A>().name.borrow_mut() = "test".to_string();

        // Assert
        assert_eq!("test", b.name());
    }

    struct C1;

    impl Component for C1 {
        fn new(_model: &Model) -> Self {
            C1
        }
    }

    struct C2;

    impl Component for C2 {
        fn new(_model: &Model) -> Self {
            C2
        }
    }

    struct C3 {
        name: String,
    }

    // C3 has a no-argument constructor and a model constructor upstream. The model one wins.
    impl Component for C3 {
        fn new(_model: &Model) -> Self {
            C3 {
                name: "model".to_string(),
            }
        }
    }

    #[test]
    fn model_test_default_constructor_fallback() {
        // Arrange
        let model = Model::default();

        // Act
        model.get_or_create::<C1>();
        model.get_or_create::<C2>();

        // Assert
        assert_eq!(model.get_or_create::<C3>().name, "model");
    }

    #[test]
    fn model_test_register() {
        // Arrange
        let model = Model::default();
        let c3 = Arc::new(C3 {
            name: "Shared struct".to_string(),
        });

        // Act
        model.register(c3.clone());

        // Assert
        assert_eq!(model.get_or_create::<C3>().name, c3.name);
    }

    #[test]
    fn model_test_register_death_test() {
        // Arrange
        let model = Model::default();
        model.register(Arc::new(C3 {
            name: String::new(),
        }));
        let c3_2 = Arc::new(C3 {
            name: String::new(),
        });

        // Act and Assert
        expect_death(
            || model.register(c3_2),
            "the type is already registered in the model",
        );
    }

    #[test]
    fn model_test_register_death_test2() {
        // Arrange
        let model = Model::default();
        model.get_or_create::<C3>();
        let c3 = Arc::new(C3 {
            name: String::new(),
        });

        // Act and Assert
        expect_death(
            || model.register(c3),
            "the type is already registered in the model",
        );
    }

    // A small copy of the SAT core: the solver calls its propagators with the trail, the
    // integer trail enqueues on the trail through its own handle and tells the solver the
    // model is unsat, and the watcher runs a constraint that writes the watcher's bitset
    // through the integer trail and asks to be called again.

    type Literal = i32;

    const DECISION: Literal = 1;
    const FIXED: Literal = -103;
    const TARGET: i64 = 3;

    fn bound_literal(bound: i64) -> Literal {
        100 + bound as Literal
    }

    trait SatPropagator {
        fn propagate(&self, trail: &Trail) -> bool;
    }

    trait PropagatorInterface {
        fn propagate(&self) -> bool;
    }

    #[derive(Default)]
    struct Trail {
        literals: DebugRefCell<Vec<Literal>>,
    }

    impl Component for Trail {
        fn new(_model: &Model) -> Self {
            Trail::default()
        }
    }

    impl Trail {
        fn index(&self) -> usize {
            self.literals.borrow().len()
        }

        fn literal_is_true(&self, literal: Literal) -> bool {
            self.literals.borrow().contains(&literal)
        }

        fn enqueue(&self, true_literal: Literal) {
            self.literals.borrow_mut().push(true_literal);
        }
    }

    struct SatSolver {
        trail: Handle<Trail>,
        propagators: DebugRefCell<Vec<Handle<dyn SatPropagator>>>,
        model_is_unsat: Cell<bool>,
    }

    impl Component for SatSolver {
        fn new(model: &Model) -> Self {
            SatSolver {
                trail: model.get_or_create::<Trail>(),
                propagators: DebugRefCell::default(),
                model_is_unsat: Cell::new(false),
            }
        }
    }

    impl SatSolver {
        fn add_propagator(&self, propagator: Handle<dyn SatPropagator>) {
            self.propagators.borrow_mut().push(propagator);
        }

        fn notify_that_model_is_unsat(&self) {
            self.model_is_unsat.set(true);
        }

        fn propagate(&self) -> bool {
            loop {
                let old_index = self.trail.index();
                let num_propagators = self.propagators.borrow().len();
                for i in 0..num_propagators {
                    let propagator = self.propagators.borrow()[i].clone();
                    if !propagator.propagate(&self.trail) {
                        return false;
                    }
                    if self.trail.index() > old_index {
                        break;
                    }
                }
                if self.trail.index() == old_index {
                    return true;
                }
            }
        }
    }

    struct IntegerTrail {
        trail: Handle<Trail>,
        sat_solver: Handle<SatSolver>,
        watchers: DebugRefCell<Vec<Handle<DebugRefCell<Vec<usize>>>>>,
        lower_bound: Cell<i64>,
        propagation_trail_index: Cell<usize>,
    }

    impl Component for IntegerTrail {
        fn new(model: &Model) -> Self {
            IntegerTrail {
                trail: model.get_or_create::<Trail>(),
                sat_solver: model.get_or_create::<SatSolver>(),
                watchers: DebugRefCell::default(),
                lower_bound: Cell::new(0),
                propagation_trail_index: Cell::new(0),
            }
        }

        fn after_create(this: &Handle<Self>, model: &Model) {
            model
                .get_or_create::<SatSolver>()
                .add_propagator(handle_as_dyn!(this, dyn SatPropagator));
        }
    }

    impl IntegerTrail {
        fn register_watcher(&self, bitset: Handle<DebugRefCell<Vec<usize>>>) {
            self.watchers.borrow_mut().push(bitset);
        }

        fn enqueue(&self, var: usize, bound: i64) {
            if bound <= self.lower_bound.get() {
                return;
            }
            self.lower_bound.set(bound);
            for bitset in self.watchers.borrow().iter() {
                bitset.borrow_mut().push(var);
            }
            self.trail.enqueue(bound_literal(bound));
        }
    }

    impl SatPropagator for IntegerTrail {
        fn propagate(&self, trail: &Trail) -> bool {
            if self.trail.literal_is_true(-FIXED) {
                self.sat_solver.notify_that_model_is_unsat();
                return false;
            }
            if !self.trail.literal_is_true(FIXED) {
                self.trail.enqueue(FIXED);
            }
            while self.propagation_trail_index.get() < trail.index() {
                let index = self.propagation_trail_index.get();
                self.propagation_trail_index.set(index + 1);
                if trail.literals.borrow()[index] == DECISION {
                    self.enqueue(0, 1);
                }
            }
            true
        }
    }

    #[derive(Default)]
    struct GenericLiteralWatcher {
        modified_vars: DebugRefCell<Vec<usize>>,
        watchers: DebugRefCell<Vec<Handle<dyn PropagatorInterface>>>,
        call_again: Cell<bool>,
        writes_seen_while_running: Cell<usize>,
    }

    impl Component for GenericLiteralWatcher {
        fn new(_model: &Model) -> Self {
            GenericLiteralWatcher::default()
        }

        fn after_create(this: &Handle<Self>, model: &Model) {
            model
                .get_or_create::<SatSolver>()
                .add_propagator(handle_as_dyn!(this, dyn SatPropagator));
            model
                .get_or_create::<IntegerTrail>()
                .register_watcher(this.field(|watcher| &watcher.modified_vars));
        }
    }

    impl GenericLiteralWatcher {
        fn register(&self, propagator: Handle<dyn PropagatorInterface>) {
            self.watchers.borrow_mut().push(propagator);
        }

        fn call_again_during_this_propagation(&self) {
            self.call_again.set(true);
        }
    }

    impl SatPropagator for GenericLiteralWatcher {
        fn propagate(&self, _trail: &Trail) -> bool {
            if self.modified_vars.borrow_mut().drain(..).count() == 0 {
                return true;
            }
            let num_watchers = self.watchers.borrow().len();
            for id in 0..num_watchers {
                let watcher = self.watchers.borrow()[id].clone();
                loop {
                    self.call_again.set(false);
                    if !watcher.propagate() {
                        return false;
                    }
                    let seen = self.modified_vars.borrow_mut().drain(..).count();
                    self.writes_seen_while_running
                        .set(self.writes_seen_while_running.get() + seen);
                    if !self.call_again.get() {
                        break;
                    }
                }
            }
            true
        }
    }

    struct LowerBoundPusher {
        model: Handle<Model>,
        watcher: Handle<GenericLiteralWatcher>,
        calls: Cell<i32>,
    }

    impl Component for LowerBoundPusher {
        fn new(model: &Model) -> Self {
            LowerBoundPusher {
                model: model.handle(),
                watcher: model.get_or_create::<GenericLiteralWatcher>(),
                calls: Cell::new(0),
            }
        }

        fn after_create(this: &Handle<Self>, _model: &Model) {
            this.watcher
                .register(handle_as_dyn!(this, dyn PropagatorInterface));
        }
    }

    impl PropagatorInterface for LowerBoundPusher {
        fn propagate(&self) -> bool {
            self.calls.set(self.calls.get() + 1);
            let integer_trail = self.model.get_or_create::<IntegerTrail>();
            let lower_bound = integer_trail.lower_bound.get();
            if lower_bound < TARGET {
                integer_trail.enqueue(0, lower_bound + 1);
                self.watcher.call_again_during_this_propagation();
            }
            true
        }
    }

    #[test]
    fn a_propagation_runs_the_re_entrant_call_chains_of_the_sat_core() {
        // Arrange
        let model = Model::default();
        let pusher = model.get_or_create::<LowerBoundPusher>();
        let sat_solver = model.get_or_create::<SatSolver>();
        model.get_or_create::<Trail>().enqueue(DECISION);

        // Act
        let result = sat_solver.propagate();

        // Assert
        assert!(!result);
        assert!(sat_solver.model_is_unsat.get());
        assert_eq!(
            *model.get_or_create::<Trail>().literals.borrow(),
            vec![1, -103, 101, 102, 103]
        );
        assert_eq!(model.get_or_create::<IntegerTrail>().lower_bound.get(), 3);
        assert_eq!(pusher.calls.get(), 3);
        assert_eq!(
            model
                .get_or_create::<GenericLiteralWatcher>()
                .writes_seen_while_running
                .get(),
            2
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_borrow_held_across_a_call_back_to_its_field_panics_in_a_debug_build() {
        // Arrange
        let model = Model::default();
        let integer_trail = model.get_or_create::<IntegerTrail>();
        let trail = model.get_or_create::<Trail>();
        let _held = trail.literals.borrow();

        // Act and Assert
        expect_death(
            || integer_trail.enqueue(0, 1),
            "DebugRefCell already borrowed",
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_handle_used_after_its_model_is_dropped_panics_in_a_debug_build() {
        // Arrange
        let model = Model::default();
        let trail = model.get_or_create::<Trail>();

        // Act
        drop(model);

        // Assert
        expect_death(
            || {
                trail.index();
            },
            "a handle was used after its component was dropped",
        );
    }

    #[derive(Default)]
    struct DropLog(Mutex<Vec<&'static str>>);

    impl DropLog {
        fn record(&self, name: &'static str) {
            self.0.lock().unwrap().push(name);
        }
    }

    trait Named {
        fn name(&self) -> &'static str;
    }

    struct Solver {
        log: Handle<DropLog>,
        propagators: DebugRefCell<Vec<Handle<dyn Named>>>,
    }

    impl Component for Solver {
        fn new(model: &Model) -> Self {
            Solver {
                log: model.get_singleton::<DropLog>().unwrap(),
                propagators: DebugRefCell::default(),
            }
        }
    }

    impl Drop for Solver {
        fn drop(&mut self) {
            self.log.record("solver");
        }
    }

    struct Propagator {
        log: Handle<DropLog>,
    }

    impl Component for Propagator {
        fn new(model: &Model) -> Self {
            Propagator {
                log: model.get_singleton::<DropLog>().unwrap(),
            }
        }

        fn after_create(this: &Handle<Self>, model: &Model) {
            model
                .get_or_create::<Solver>()
                .propagators
                .borrow_mut()
                .push(handle_as_dyn!(this, dyn Named));
        }
    }

    impl Named for Propagator {
        fn name(&self) -> &'static str {
            "propagator"
        }
    }

    impl Drop for Propagator {
        fn drop(&mut self) {
            self.log.record(self.name());
        }
    }

    struct Later {
        log: Handle<DropLog>,
    }

    impl Component for Later {
        fn new(model: &Model) -> Self {
            Later {
                log: model.get_singleton::<DropLog>().unwrap(),
            }
        }
    }

    impl Drop for Later {
        fn drop(&mut self) {
            self.log.record("later");
        }
    }

    #[test]
    fn the_model_drops_components_in_reverse_order_of_creation() {
        // Arrange
        let log = Arc::new(DropLog::default());
        let model = Model::default();
        model.register(log.clone());
        model.get_or_create::<Solver>();
        model.get_or_create::<Propagator>();
        model.get_or_create::<Later>();

        // Act
        drop(model);

        // Assert
        assert_eq!(
            *log.0.lock().unwrap(),
            vec!["later", "propagator", "solver"]
        );
        assert_eq!(Arc::strong_count(&log), 1);
    }

    struct P {
        log: Handle<DropLog>,
        q: DebugRefCell<Option<Handle<Q>>>,
    }

    struct Q {
        log: Handle<DropLog>,
        p: Handle<P>,
    }

    impl Component for P {
        fn new(model: &Model) -> Self {
            P {
                log: model.get_singleton::<DropLog>().unwrap(),
                q: DebugRefCell::default(),
            }
        }
    }

    impl Component for Q {
        fn new(model: &Model) -> Self {
            Q {
                log: model.get_singleton::<DropLog>().unwrap(),
                p: model.get_or_create::<P>(),
            }
        }

        fn after_create(this: &Handle<Self>, _model: &Model) {
            *this.p.q.borrow_mut() = Some(this.clone());
        }
    }

    impl Drop for P {
        fn drop(&mut self) {
            self.log.record("p");
        }
    }

    impl Drop for Q {
        fn drop(&mut self) {
            self.log.record("q");
        }
    }

    #[test]
    fn two_components_that_point_to_each_other_are_both_dropped() {
        // Arrange
        let log = Arc::new(DropLog::default());
        let model = Model::default();
        model.register(log.clone());
        let q = model.get_or_create::<Q>();
        let p = model.get_or_create::<P>();
        let q_of_p = p.q.borrow().clone().unwrap();
        assert_eq!(q_of_p.p.as_non_null(), p.as_non_null());
        assert_eq!(q.p.as_non_null(), p.as_non_null());

        // Act
        drop(model);

        // Assert
        assert_eq!(*log.0.lock().unwrap(), vec!["q", "p"]);
    }

    struct Registry {
        members: DebugRefCell<Vec<Handle<Member>>>,
    }

    impl Component for Registry {
        fn new(_model: &Model) -> Self {
            Registry {
                members: DebugRefCell::default(),
            }
        }
    }

    struct Member;

    impl Component for Member {
        fn new(_model: &Model) -> Self {
            Member
        }

        fn after_create(this: &Handle<Self>, model: &Model) {
            model
                .get_or_create::<Registry>()
                .members
                .borrow_mut()
                .push(this.clone());
        }
    }

    #[test]
    fn create_runs_the_same_construction_steps_as_get_or_create() {
        // Arrange
        let model = Model::default();

        // Act
        let first = model.create::<Member>();
        let second = model.create::<Member>();

        // Assert
        let registry = model.get_or_create::<Registry>();
        let members = registry.members.borrow();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].as_non_null(), first.as_non_null());
        assert_eq!(members[1].as_non_null(), second.as_non_null());
        assert!(model.get_singleton::<Member>().is_none());
    }

    struct SharedCounter(AtomicI64);

    #[test]
    fn a_shared_object_is_registered_in_the_model_of_each_worker_thread() {
        // Arrange
        let shared = Arc::new(SharedCounter(AtomicI64::new(0)));

        // Act
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let shared = shared.clone();
                std::thread::spawn(move || {
                    let model = Model::default();
                    model.register(shared);
                    let counter = model.get_singleton::<SharedCounter>().unwrap();
                    counter.0.fetch_add(1, Ordering::SeqCst);
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }

        // Assert
        assert_eq!(shared.0.load(Ordering::SeqCst), 2);
        assert_eq!(Arc::strong_count(&shared), 1);
    }
}
