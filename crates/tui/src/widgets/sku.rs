//! SKU candidates and their session-local supplier origins.
//!
//! The UI owns a registry of strong supplier references. Candidates retain their
//! owned SKU, a domain-typed supplier ID, and a weak link to the original supplier.
//! Removing a registry entry releases its strong reference; the weak link expires
//! when the last strong owner is dropped. This registry is not persisted.
//!
//! ```
//! use std::sync::Arc;
//! use app::domain::{CounterSkuBuilder, Id, SkuFamily};
//! use tui::widgets::sku::{SkuSupplierIdentity, SkuSupplierRegistry};
//!
//! struct Hardware;
//! impl SkuFamily for Hardware {}
//! let mut suppliers = SkuSupplierRegistry::default();
//! let counter = CounterSkuBuilder::default().add("BOLT").build::<Hardware>(Some(4))?;
//! let id: Id<SkuSupplierIdentity> = suppliers.register(Arc::new(counter));
//! let first = suppliers.generate(id)?;
//! let second = suppliers.generate(id)?;
//! assert_eq!(first.sku().as_str(), "BOLT-0001");
//! assert_eq!(second.sku().as_str(), "BOLT-0002");
//! assert_eq!(first.supplier_id(), Some(id));
//! suppliers.remove(id);
//! assert!(first.supplier().is_none());
//! assert_eq!(first.sku().as_str(), "BOLT-0001");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use app::domain::{CounterExhausted, Id, Sku, SkuSupplier};

/// Marker for domain IDs identifying suppliers, rather than items or movements.
#[derive(Debug)]
pub enum SkuSupplierIdentity {}

pub type SupplierId = Id<SkuSupplierIdentity>;

/// Sync is required alongside Send to share suppliers across Send pages using Arc.
pub type DynSkuSupplier = dyn SkuSupplier<Output = Sku, Error = CounterExhausted> + Send + Sync;

#[derive(Debug, thiserror::Error)]
pub enum SkuSupplierError {
    #[error("SKU supplier is not registered: {0:?}")]
    NotRegistered(SupplierId),
    #[error("The original SKU supplier is no longer alive: {0:?}")]
    Expired(SupplierId),
    #[error(transparent)]
    Exhausted(#[from] CounterExhausted),
}

/// Strong ownership of suppliers for a UI session. Register each shared sequence
/// once and use its ID to generate candidates for multiple rows.
#[derive(Default)]
pub struct SkuSupplierRegistry {
    suppliers: HashMap<SupplierId, Arc<DynSkuSupplier>>,
}

impl SkuSupplierRegistry {
    pub fn register(&mut self, supplier: Arc<DynSkuSupplier>) -> SupplierId {
        let id = SupplierId::new();
        self.suppliers.insert(id, supplier);
        id
    }

    /// An owned reference keeps the supplier alive even after registry removal.
    pub fn get(&self, id: SupplierId) -> Option<Arc<DynSkuSupplier>> {
        self.suppliers.get(&id).cloned()
    }

    /// Releases the registry's ownership. Existing candidate values and origin
    /// IDs survive; retained external Arcs can keep the original supplier alive.
    pub fn remove(&mut self, id: SupplierId) -> bool {
        self.suppliers.remove(&id).is_some()
    }

    /// Generates a candidate without transferring strong ownership to the row.
    /// Sequence values are consumed even if the user later cancels the form.
    pub fn generate(&self, id: SupplierId) -> Result<SkuClonglomerate, SkuSupplierError> {
        let supplier = self
            .suppliers
            .get(&id)
            .ok_or(SkuSupplierError::NotRegistered(id))?;
        let sku = supplier.supply()?;
        Ok(SkuClonglomerate {
            sku,
            supplied_from: Some(SupplierOrigin {
                id,
                supplier: Arc::downgrade(supplier),
            }),
        })
    }
}

struct SupplierOrigin {
    id: SupplierId,
    supplier: Weak<DynSkuSupplier>,
}

/// A candidate value plus optional provenance. Reads never advance its supplier.
pub struct SkuClonglomerate {
    sku: Sku,
    supplied_from: Option<SupplierOrigin>,
}

impl SkuClonglomerate {
    pub fn manual(sku: Sku) -> Self {
        Self {
            sku,
            supplied_from: None,
        }
    }

    pub fn sku(&self) -> &Sku {
        &self.sku
    }

    /// Generated origin remains recorded even after the supplier expires.
    pub fn is_generated(&self) -> bool {
        self.supplied_from.is_some()
    }

    pub fn supplier_id(&self) -> Option<SupplierId> {
        self.supplied_from.as_ref().map(|origin| origin.id)
    }

    /// Temporarily acquires the exact original supplier, if it is still alive.
    pub fn supplier(&self) -> Option<Arc<DynSkuSupplier>> {
        self.supplied_from.as_ref()?.supplier.upgrade()
    }

    /// Explicitly replaces the candidate using its original sequence. Returns
    /// None for manual values; failures keep the current SKU and provenance intact.
    pub fn regenerate(&mut self) -> Result<Option<&Sku>, SkuSupplierError> {
        let Some(origin) = &self.supplied_from else {
            return Ok(None);
        };
        let supplier = origin
            .supplier
            .upgrade()
            .ok_or(SkuSupplierError::Expired(origin.id))?;
        let next = supplier.supply()?;
        self.sku = next;
        Ok(Some(&self.sku))
    }

    /// Manual input intentionally clears the previous supplier provenance.
    pub fn set_manual(&mut self, sku: Sku) {
        self.sku = sku;
        self.supplied_from = None;
    }

    pub fn into_sku(self) -> Sku {
        self.sku
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app::domain::{CounterSkuBuilder, SkuFamily};

    struct Hardware;
    impl SkuFamily for Hardware {}

    fn counter(start: usize) -> Arc<DynSkuSupplier> {
        Arc::new(
            CounterSkuBuilder::new(None, Some(start))
                .add("BOLT")
                .build::<Hardware>(Some(4))
                .unwrap(),
        )
    }

    #[test]
    fn candidates_share_one_sequence_and_keep_their_own_values() {
        let mut registry = SkuSupplierRegistry::default();
        let id = registry.register(counter(1));
        let mut first = registry.generate(id).unwrap();
        let second = registry.generate(id).unwrap();
        assert_eq!(first.sku().as_str(), "BOLT-0001");
        assert_eq!(second.sku().as_str(), "BOLT-0002");
        assert_eq!(first.supplier_id(), Some(id));
        assert_eq!(second.supplier_id(), Some(id));
        assert_eq!(first.regenerate().unwrap().unwrap().as_str(), "BOLT-0003");
        assert_eq!(second.sku().as_str(), "BOLT-0002");
        assert!(Arc::ptr_eq(
            &first.supplier().unwrap(),
            &registry.get(id).unwrap()
        ));
    }

    #[test]
    fn removal_expires_the_origin_but_preserves_the_sku_and_typed_id() {
        let mut registry = SkuSupplierRegistry::default();
        let id = registry.register(counter(1));
        let mut value = registry.generate(id).unwrap();
        let before = value.sku().clone();
        assert!(registry.remove(id));
        assert!(!registry.remove(id));
        assert!(registry.get(id).is_none());
        assert!(value.supplier().is_none());
        assert!(value.is_generated());
        assert_eq!(value.supplier_id(), Some(id));
        assert!(
            matches!(value.regenerate(), Err(SkuSupplierError::Expired(source)) if source == id)
        );
        assert_eq!(value.sku(), &before);
        assert!(
            matches!(registry.generate(id), Err(SkuSupplierError::NotRegistered(source)) if source == id)
        );
        // A newly registered counter cannot accidentally retarget the old origin.
        let replacement = registry.register(counter(1));
        assert_ne!(replacement, id);
        assert!(value.supplier().is_none());
        assert_eq!(value.into_sku(), before);
    }

    #[test]
    fn weak_links_survive_registry_removal_only_while_another_strong_owner_exists() {
        let mut registry = SkuSupplierRegistry::default();
        let shared = counter(1);
        let id = registry.register(shared.clone());
        let mut value = registry.generate(id).unwrap();
        registry.remove(id);
        assert_eq!(value.regenerate().unwrap().unwrap().as_str(), "BOLT-0002");
        drop(shared);
        assert!(matches!(
            value.regenerate(),
            Err(SkuSupplierError::Expired(_))
        ));
        assert_eq!(value.sku().as_str(), "BOLT-0002");
    }

    #[test]
    fn dropping_registry_drops_its_suppliers_and_manual_edits_clear_provenance() {
        let mut value = {
            let mut registry = SkuSupplierRegistry::default();
            let id = registry.register(counter(1));
            registry.generate(id).unwrap()
        };
        assert!(value.supplier().is_none());
        value.set_manual(Sku::new("MANUAL").unwrap());
        assert!(!value.is_generated());
        assert_eq!(value.supplier_id(), None);
        assert!(value.regenerate().unwrap().is_none());
        assert_eq!(value.into_sku().as_str(), "MANUAL");
    }

    #[test]
    fn exhaustion_preserves_the_current_value_and_source() {
        let mut registry = SkuSupplierRegistry::default();
        let id = registry.register(counter(usize::MAX - 1));
        let mut value = registry.generate(id).unwrap();
        let before = value.sku().clone();
        assert!(matches!(
            value.regenerate(),
            Err(SkuSupplierError::Exhausted(_))
        ));
        assert_eq!(value.sku(), &before);
        assert_eq!(value.supplier_id(), Some(id));
        let exhausted = registry.register(counter(usize::MAX));
        assert!(matches!(
            registry.generate(exhausted),
            Err(SkuSupplierError::Exhausted(_))
        ));
    }
}
