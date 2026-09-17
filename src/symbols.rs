//! Typed symbol interning for resolved QCL models.
//!
//! A [`crate::symbols::SymbolTable`] assigns stable, dense IDs in insertion order while
//! preventing duplicate declarations.

use std::{collections::HashMap, hash::Hash, marker::PhantomData};

use thiserror::Error;

use crate::domain::{AgentId, AtomId, StateId};

/// The typed-index contract used by symbol tables.
pub trait SymbolId: Copy + Eq + Hash {
    /// Construct an ID from a dense zero-based index.
    fn from_index(index: usize) -> Self;
    /// Return the dense zero-based index.
    fn index(self) -> usize;
}

macro_rules! impl_symbol_id {
    ($id:ty) => {
        impl SymbolId for $id {
            fn from_index(index: usize) -> Self {
                Self::new(index)
            }

            fn index(self) -> usize {
                self.index()
            }
        }
    };
}

impl_symbol_id!(AgentId);
impl_symbol_id!(StateId);
impl_symbol_id!(AtomId);

/// Failure returned while inserting or resolving a symbol.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SymbolTableError {
    /// The name already exists in the table.
    #[error("duplicate symbol `{name}`")]
    Duplicate {
        /// Name that was inserted twice.
        name: String,
    },
    /// The requested name does not exist.
    #[error("unknown symbol `{name}`")]
    Unknown {
        /// Name that could not be resolved.
        name: String,
    },
    /// An ID does not correspond to an entry in the table.
    #[error("symbol id {index} is not present in this table")]
    InvalidId {
        /// Invalid dense index.
        index: usize,
    },
}

/// A dense, insertion-ordered mapping from names to one kind of domain ID.
#[derive(Clone, Debug)]
pub struct SymbolTable<I: SymbolId> {
    names: Vec<String>,
    ids: HashMap<String, I>,
    marker: PhantomData<I>,
}

impl<I: SymbolId> Default for SymbolTable<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: SymbolId> SymbolTable<I> {
    /// Create an empty symbol table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            ids: HashMap::new(),
            marker: PhantomData,
        }
    }

    /// Create an empty table with capacity for `capacity` names.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            names: Vec::with_capacity(capacity),
            ids: HashMap::with_capacity(capacity),
            marker: PhantomData,
        }
    }

    /// Add a new name, returning its next dense ID.
    ///
    /// # Errors
    ///
    /// Returns [`SymbolTableError::Duplicate`] when the name is already
    /// defined in this table.
    pub fn insert(&mut self, name: impl Into<String>) -> Result<I, SymbolTableError> {
        let name = name.into();
        if self.ids.contains_key(&name) {
            return Err(SymbolTableError::Duplicate { name });
        }

        let id = I::from_index(self.names.len());
        self.names.push(name.clone());
        self.ids.insert(name, id);
        Ok(id)
    }

    /// Define a new name, returning its next dense ID.
    ///
    /// # Errors
    ///
    /// Returns [`SymbolTableError::Duplicate`] when the name is already
    /// defined in this table.
    pub fn define(&mut self, name: impl Into<String>) -> Result<I, SymbolTableError> {
        self.insert(name)
    }

    /// Return the ID for `name`, or `None` when it is not defined.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<I> {
        self.ids.get(name).copied()
    }

    /// Resolve a name to its typed ID.
    ///
    /// # Errors
    ///
    /// Returns [`SymbolTableError::Unknown`] when the name is not defined.
    pub fn lookup(&self, name: &str) -> Result<I, SymbolTableError> {
        self.get(name).ok_or_else(|| SymbolTableError::Unknown {
            name: name.to_owned(),
        })
    }

    /// Resolve a name to its typed ID.
    ///
    /// # Errors
    ///
    /// Returns [`SymbolTableError::Unknown`] when the name is not defined.
    pub fn resolve(&self, name: &str) -> Result<I, SymbolTableError> {
        self.lookup(name)
    }

    /// Return the name associated with `id`, or `None` for an invalid ID.
    #[must_use]
    pub fn name(&self, id: I) -> Option<&str> {
        self.names.get(id.index()).map(String::as_str)
    }

    /// Return the number of names in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Return whether the table contains no names.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Iterate over `(id, name)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (I, &str)> {
        self.names
            .iter()
            .enumerate()
            .map(|(index, name)| (I::from_index(index), name.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_dense_and_typed() {
        let mut agents = SymbolTable::<AgentId>::new();
        assert_eq!(agents.insert("alice"), Ok(AgentId::new(0)));
        assert_eq!(agents.insert("bob"), Ok(AgentId::new(1)));
        assert_eq!(agents.lookup("bob"), Ok(AgentId::new(1)));
        assert_eq!(agents.name(AgentId::new(0)), Some("alice"));
        assert_eq!(
            agents.iter().collect::<Vec<_>>(),
            vec![(AgentId::new(0), "alice"), (AgentId::new(1), "bob")]
        );
    }

    #[test]
    fn duplicate_and_unknown_names_are_errors() {
        let mut states = SymbolTable::<StateId>::new();
        states.define("s0").expect("first definition");
        assert_eq!(
            states.define("s0"),
            Err(SymbolTableError::Duplicate {
                name: "s0".to_owned()
            })
        );
        assert_eq!(
            states.lookup("missing"),
            Err(SymbolTableError::Unknown {
                name: "missing".to_owned()
            })
        );
        assert_eq!(states.name(StateId::new(9)), None);
    }
}
