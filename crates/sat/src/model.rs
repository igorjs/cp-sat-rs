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
//! A component type implements `Component`. `Model::get_or_create` builds it once with
//! `Component::new`, which may fetch the components it needs from the model, and hands out
//! an `Rc<RefCell<T>>`. A component keeps another one as a `Handle`, which does not own it,
//! and borrows it for one call with `handle.get().borrow_mut()`.
//!
//! A component that is called with `&mut` access to another one never borrows that one again
//! through a handle: it uses the argument. Borrowing twice panics.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// A type that `Model` can build. It is the `T(Model* model)` constructor, or `T()` when
/// `new` ignores the model.
pub trait Component: Sized + 'static {
    fn new(model: &Model) -> Self;

    /// Runs after the new object is in the model. Upstream constructors that pass `this` to
    /// another component, such as `model->GetOrCreate<SatSolver>()->AddPropagator(this)`
    /// (`sat/integer.h:558`), do that here, since a Rust constructor has no address yet.
    fn after_create(_this: &Rc<RefCell<Self>>, _model: &Model) {}
}

/// A pointer from one component to another that the component does not own, as the raw
/// pointer members of the upstream components.
pub struct Handle<T: ?Sized>(Weak<RefCell<T>>);

impl<T: ?Sized> Handle<T> {
    pub fn new(component: &Rc<RefCell<T>>) -> Self {
        Handle(Rc::downgrade(component))
    }

    /// Panics when the model that owned the component has been dropped.
    pub fn get(&self) -> Rc<RefCell<T>> {
        self.0
            .upgrade()
            .expect("the model that owns the component was dropped")
    }
}

impl<T: ?Sized> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
    }
}

/// Owns everything related to a particular optimization model.
#[derive(Default)]
pub struct Model {
    name: String,
    // Map of the type to a "singleton" of that type. It holds a second `Rc` of an owned
    // component, so `drop` empties it first.
    singletons: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
    // The list of items to delete.
    cleanup_list: RefCell<Vec<Rc<dyn Any>>>,
}

impl Model {
    /// Names the model for debugging or logging.
    pub fn new(name: impl Into<String>) -> Self {
        let mut model = Model::default();
        model.name = name.into();
        model
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
    pub fn get_or_create<T: Component>(&self) -> Rc<RefCell<T>> {
        if let Some(found) = self.get_singleton::<T>() {
            return found;
        }

        // New element.
        let new_t = Rc::new(RefCell::new(T::new(self)));
        self.singletons
            .borrow_mut()
            .insert(TypeId::of::<T>(), new_t.clone());
        self.take_ownership_of(new_t.clone());
        T::after_create(&new_t, self);
        new_t
    }

    /// Like `get_or_create` but does not create the object if it does not exist.
    ///
    /// Upstream returns a const pointer here. The Rust borrow, `borrow` or `borrow_mut`,
    /// decides whether the caller reads or writes.
    pub fn get_singleton<T: 'static>(&self) -> Option<Rc<RefCell<T>>> {
        let found = self.singletons.borrow().get(&TypeId::of::<T>())?.clone();
        Some(downcast(found))
    }

    /// Same as `get_singleton`; upstream returns a mutable pointer here.
    pub fn mutable<T: 'static>(&self) -> Option<Rc<RefCell<T>>> {
        self.get_singleton::<T>()
    }

    /// Gives ownership of an object to this model. It is destroyed when the model is.
    pub fn take_ownership<T: 'static>(&self, t: T) -> Rc<RefCell<T>> {
        let t = Rc::new(RefCell::new(t));
        self.take_ownership_of(t.clone());
        t
    }

    /// Returns a non-singleton object owned by the model and created with
    /// `Component::new`.
    pub fn create<T: Component>(&self) -> Rc<RefCell<T>> {
        let new_t = Rc::new(RefCell::new(T::new(self)));
        self.take_ownership_of(new_t.clone());
        new_t
    }

    /// Registers a non-owned class that will be "singleton" in the model: the caller keeps
    /// its own `Rc` and the model does not destroy the object.
    ///
    /// It is an error to call this on an already registered class.
    pub fn register<T: 'static>(&self, non_owned_class: Rc<RefCell<T>>) {
        let mut singletons = self.singletons.borrow_mut();
        let type_id = TypeId::of::<T>();
        assert!(
            !singletons.contains_key(&type_id),
            "the type is already registered in the model"
        );
        singletons.insert(type_id, non_owned_class);
    }

    pub fn unregister<T: 'static>(&self) {
        self.singletons.borrow_mut().remove(&TypeId::of::<T>());
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn take_ownership_of<T: 'static>(&self, t: Rc<RefCell<T>>) {
        self.cleanup_list.borrow_mut().push(t);
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // The order of deletion seems to be platform dependent.
        // We force a reverse order on the cleanup vector.
        self.singletons.get_mut().clear();
        let cleanup_list = self.cleanup_list.get_mut();
        while cleanup_list.pop().is_some() {}
    }
}

fn downcast<T: 'static>(found: Rc<dyn Any>) -> Rc<RefCell<T>> {
    found
        .downcast::<RefCell<T>>()
        .unwrap_or_else(|_| unreachable!("a singleton is stored under the id of its own type"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpsat_testutil::expect_death;

    #[derive(Default)]
    struct A {
        name: String,
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
                a: Handle::new(&model.get_or_create::<A>()),
            }
        }
    }

    impl B {
        fn name(&self) -> String {
            self.a.get().borrow().name.clone()
        }
    }

    #[test]
    fn model_test_recursive_creation_test() {
        // Arrange
        let model = Model::default();

        // Act
        let b = model.get_or_create::<B>();
        model.get_or_create::<A>().borrow_mut().name = "test".to_string();

        // Assert
        assert_eq!("test", b.borrow().name());
    }

    #[derive(Default)]
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
        assert_eq!(model.get_or_create::<C3>().borrow().name, "model");
    }

    #[test]
    fn model_test_register() {
        // Arrange
        let model = Model::default();
        let c3 = Rc::new(RefCell::new(C3 {
            name: "Shared struct".to_string(),
        }));

        // Act
        model.register(c3.clone());

        // Assert
        assert_eq!(model.get_or_create::<C3>().borrow().name, c3.borrow().name);
    }

    #[test]
    fn model_test_register_death_test() {
        // Arrange
        let model = Model::default();
        let c3 = Rc::new(RefCell::new(C3 {
            name: String::new(),
        }));
        model.register(c3);
        let c3_2 = Rc::new(RefCell::new(C3 {
            name: String::new(),
        }));

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
        let c3 = Rc::new(RefCell::new(C3 {
            name: String::new(),
        }));

        // Act and Assert
        expect_death(
            || model.register(c3),
            "the type is already registered in the model",
        );
    }

    // The next four tests prove the form of the container on the cases the SAT core needs.

    impl B {
        fn rename(&self, name: &str) {
            self.a.get().borrow_mut().name = name.to_string();
        }
    }

    #[test]
    fn a_component_reads_another_through_its_handle() {
        // Arrange
        let model = Model::default();
        let b = model.get_or_create::<B>();

        // Act
        model.get_or_create::<A>().borrow_mut().name = "read".to_string();

        // Assert
        assert_eq!(b.borrow().name(), "read");
    }

    #[test]
    fn a_component_mutates_another_through_its_handle() {
        // Arrange
        let model = Model::default();
        let b = model.get_or_create::<B>();

        // Act
        b.borrow().rename("changed");

        // Assert
        assert_eq!(model.get_or_create::<A>().borrow().name, "changed");
    }

    struct P {
        q: Option<Handle<Q>>,
        value: i32,
    }

    struct Q {
        p: Handle<P>,
        value: i32,
    }

    impl Component for P {
        fn new(_model: &Model) -> Self {
            P { q: None, value: 0 }
        }
    }

    impl Component for Q {
        fn new(model: &Model) -> Self {
            Q {
                p: Handle::new(&model.get_or_create::<P>()),
                value: 0,
            }
        }

        fn after_create(this: &Rc<RefCell<Self>>, model: &Model) {
            model.get_or_create::<P>().borrow_mut().q = Some(Handle::new(this));
        }
    }

    #[test]
    fn two_components_hold_handles_to_each_other() {
        // Arrange
        let model = Model::default();
        let q = model.get_or_create::<Q>();
        let p = model.get_or_create::<P>();

        // Act
        q.borrow().p.get().borrow_mut().value = 5;
        p.borrow().q.as_ref().unwrap().get().borrow_mut().value = 7;

        // Assert
        assert_eq!((p.borrow().value, q.borrow().value), (5, 7));
    }

    trait Propagator {
        fn propagate(&mut self, trail: &mut Trail) -> bool;
    }

    #[derive(Default)]
    struct Trail {
        assigned: Vec<i32>,
        propagators: Vec<Rc<RefCell<dyn Propagator>>>,
    }

    impl Component for Trail {
        fn new(_model: &Model) -> Self {
            Trail::default()
        }
    }

    impl Trail {
        fn propagate(&mut self) {
            let mut i = 0;
            while i < self.propagators.len() {
                let propagator = self.propagators[i].clone();
                propagator.borrow_mut().propagate(self);
                i += 1;
            }
        }
    }

    struct Pusher {
        other: Handle<A>,
    }

    impl Propagator for Pusher {
        fn propagate(&mut self, trail: &mut Trail) -> bool {
            self.other.get().borrow_mut().name.push('!');
            trail.assigned.push(1);
            true
        }
    }

    impl Component for Pusher {
        fn new(model: &Model) -> Self {
            Pusher {
                other: Handle::new(&model.get_or_create::<A>()),
            }
        }

        fn after_create(this: &Rc<RefCell<Self>>, model: &Model) {
            let propagator: Rc<RefCell<dyn Propagator>> = this.clone();
            model
                .get_or_create::<Trail>()
                .borrow_mut()
                .propagators
                .push(propagator);
        }
    }

    #[test]
    fn a_trail_calls_trait_object_propagators_with_mutable_access_to_itself() {
        // Arrange
        let model = Model::default();
        model.get_or_create::<Pusher>();
        let trail = model.get_or_create::<Trail>();

        // Act
        trail.borrow_mut().propagate();

        // Assert
        assert_eq!(trail.borrow().assigned, vec![1]);
        assert_eq!(model.get_or_create::<A>().borrow().name, "!");
    }

    #[test]
    fn a_component_made_inside_the_constructor_of_another_exists_once() {
        // Arrange
        let model = Model::default();

        // Act
        let first = model.get_or_create::<B>();
        let a = model.get_or_create::<A>();

        // Assert
        assert!(Rc::ptr_eq(&first.borrow().a.get(), &a));
        assert!(Rc::ptr_eq(&model.get_or_create::<B>(), &first));
    }
}
