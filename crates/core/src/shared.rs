//! Copy-on-write authoritative definitions. Derived keys can retain allocation
//! identity without retaining definition data; mutable access detaches it first.
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Shared<T>(Arc<T>);

/// An allocation identity with no ownership of its definition data. Keeping the
/// weak allocation prevents address reuse, while dropping the last strong owner
/// still releases T and all of its owned allocations. Arc::make_mut detaches even
/// a unique strong owner when an identity token exists.
pub struct Identity<T>(Weak<T>);

impl<T> Identity<T> {
    pub fn is_of(&self, value: &Shared<T>) -> bool {
        std::ptr::eq(self.0.as_ptr(), Arc::as_ptr(&value.0))
    }
}

impl<T> Clone for Identity<T> {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
    pub fn same_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn identity(&self) -> Identity<T> {
        Identity(Arc::downgrade(&self.0))
    }
}
impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}
impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        self.same_allocation(other) || self.0 == other.0
    }
}
impl<T: Eq> Eq for Shared<T> {}
impl<'a, T> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.0.as_ref().into_iter()
    }
}
impl<'a, T: Clone> IntoIterator for &'a mut Shared<T>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().into_iter()
    }
}
impl<T: Clone + IntoIterator> IntoIterator for Shared<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}
impl<A, T: FromIterator<A>> FromIterator<A> for Shared<T> {
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Self::new(T::from_iter(iter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn a_retained_key_detects_mutation_without_retaining_a_model() {
        let mut value = Shared::new(BTreeMap::from([("width".to_string(), 200)]));
        let old = value.clone();
        assert!(value.same_allocation(&old));
        value.insert("width".to_string(), 700);
        assert!(!value.same_allocation(&old));
        assert_eq!(old["width"], 200);
        assert_eq!(value["width"], 700);
        assert_eq!(serde_json::to_value(&old).unwrap(), serde_json::json!({"width":200}));
        let read: Shared<BTreeMap<String, i32>> = serde_json::from_value(serde_json::json!({"width":200})).unwrap();
        assert_eq!(read, old);
        assert!(!read.same_allocation(&old));
    }

    #[test]
    fn identity_tokens_detect_unique_owner_mutations_and_snapshot_branches() {
        let mut value = Shared::new(vec![200]);
        let first = value.identity();
        assert!(first.is_of(&value));
        value.push(700);
        assert!(!first.is_of(&value), "a unique owner must detach its weak identity on mutation");
        let second = value.identity();
        let branch = value.clone();
        assert!(second.is_of(&branch));
        value.push(900);
        assert!(!second.is_of(&value));
        assert!(second.is_of(&branch));
        assert_eq!(&*branch, &[200, 700]);
        assert!(!first.is_of(&Shared::new(vec![200])));
    }

    #[test]
    fn identity_tokens_do_not_retain_definition_data() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let value = Shared::new(Probe(Arc::clone(&drops)));
        let identity = value.identity();
        assert!(identity.is_of(&value));
        drop(value);
        assert_eq!(drops.load(Ordering::Relaxed), 1, "an identity must not keep the definition alive");
        drop(identity);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}
