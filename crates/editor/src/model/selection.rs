//! The set of selected schematic objects.

use std::collections::BTreeSet;
use xsim_project::{Id, Schematic};

/// A reference to one object of a schematic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectRef {
    Component(Id),
    Wire(Id),
    Label(Id),
}

impl ObjectRef {
    /// Whether the object still exists in `schematic`.
    pub fn exists_in(self, schematic: &Schematic) -> bool {
        match self {
            Self::Component(id) => schematic.component(id).is_some(),
            Self::Wire(id) => schematic.wire(id).is_some(),
            Self::Label(id) => schematic.label(id).is_some(),
        }
    }
}

/// Selected objects, kept in a stable order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection(BTreeSet<ObjectRef>);

impl Selection {
    /// Every object of `schematic`.
    pub fn all(schematic: &Schematic) -> Self {
        let components = schematic
            .components
            .iter()
            .map(|c| ObjectRef::Component(c.id));
        let wires = schematic.wires.iter().map(|w| ObjectRef::Wire(w.id));
        let labels = schematic.labels.iter().map(|l| ObjectRef::Label(l.id));
        Self(components.chain(wires).chain(labels).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, object: ObjectRef) -> bool {
        self.0.contains(&object)
    }

    pub fn insert(&mut self, object: ObjectRef) {
        self.0.insert(object);
    }

    /// Adds `object`, or removes it if it was already selected.
    pub fn toggle(&mut self, object: ObjectRef) {
        if !self.0.remove(&object) {
            self.0.insert(object);
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = ObjectRef> + '_ {
        self.0.iter().copied()
    }

    pub fn has_component(&self, id: Id) -> bool {
        self.contains(ObjectRef::Component(id))
    }

    pub fn has_wire(&self, id: Id) -> bool {
        self.contains(ObjectRef::Wire(id))
    }

    pub fn has_label(&self, id: Id) -> bool {
        self.contains(ObjectRef::Label(id))
    }

    /// The only selected component, if exactly one object is selected and it
    /// is a component.
    pub fn single_component(&self) -> Option<Id> {
        match (self.len(), self.0.first()) {
            (1, Some(ObjectRef::Component(id))) => Some(*id),
            _ => None,
        }
    }

    /// Drops objects that no longer exist in `schematic`.
    pub fn retain_existing(&mut self, schematic: &Schematic) {
        self.0.retain(|o| o.exists_in(schematic));
    }
}

impl FromIterator<ObjectRef> for Selection {
    fn from_iter<T: IntoIterator<Item = ObjectRef>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Extend<ObjectRef> for Selection {
    fn extend<T: IntoIterator<Item = ObjectRef>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}
