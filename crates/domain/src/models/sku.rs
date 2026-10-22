use std::{fmt, str::FromStr};

/// A product code whose identity is its exact saved text, including casing.
/// Build structured codes with SkuBuilder, or load existing codes with Sku::new.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sku(Box<str>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidSku {
    #[error("SKU must not be empty or whitespace")]
    Empty,
    #[error("SKU field at index {index} must not be empty or whitespace")]
    EmptyField { index: usize },
}

impl Sku {
    /// Preserves existing text exactly; no automatic trimming or uppercasing.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidSku> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InvalidSku::Empty);
        }
        Ok(Self(value.into_boxed_str()))
    }

    /// Borrows the stored code without allocating.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sku {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for Sku {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for Sku {
    type Err = InvalidSku;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Owns editable fields, so callers may pass temporary strings.
/// Fields are joined with hyphens once at finalization. Field boundaries are
/// formatting conveniences, not a separately persisted SKU schema.
/// ```
/// use domain::models::sku::SkuBuilder;
/// let sku = SkuBuilder::new()
///     .add_field("GPU")
///     .add_field("5090")
///     .add_field(format!("{:03}", 1))
///     .finalize()?;
/// assert_eq!(sku.as_str(), "GPU-5090-001");
/// # Ok::<(), domain::models::sku::InvalidSku>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct SkuBuilder {
    fields: Vec<Box<str>>,
}

impl SkuBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_field(mut self, field: impl AsRef<str>) -> Self {
        self.fields.push(field.as_ref().into());
        self
    }

    /// Undoes the last addition. An empty builder stays empty.
    pub fn remove_last_field(mut self) -> Self {
        self.fields.pop();
        self
    }

    #[deprecated(note = "use remove_last_field; this removes a field rather than reversing it")]
    pub fn reverse_field(self) -> Self {
        self.remove_last_field()
    }

    pub fn snapshot(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.fields.iter().map(AsRef::as_ref)
    }

    pub fn finalize(self) -> Result<Sku, InvalidSku> {
        for (index, field) in self.fields.iter().enumerate() {
            if field.trim().is_empty() {
                return Err(InvalidSku::EmptyField { index });
            }
        }
        Sku::new(self.fields.join("-"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_owns_fields_and_undo_removes_only_the_last_one() {
        let builder = {
            let category = String::from("GPU");
            SkuBuilder::new()
                .add_field(&category)
                .add_field(format!("{}", 5090))
        };
        assert_eq!(builder.snapshot().collect::<Vec<_>>(), ["GPU", "5090"]);
        let sku = builder
            .add_field("wrong")
            .remove_last_field()
            .add_field("001")
            .finalize()
            .unwrap();
        assert_eq!(sku.as_str(), "GPU-5090-001");
        assert_eq!(sku.to_string(), "GPU-5090-001");
        assert_eq!(sku.as_ref(), sku.as_str());
        assert_eq!(sku.as_str().parse::<Sku>().unwrap(), sku);
    }

    #[test]
    fn empty_codes_and_empty_builder_fields_are_rejected() {
        assert_eq!(Sku::new(" \t\n"), Err(InvalidSku::Empty));
        assert_eq!(SkuBuilder::new().finalize(), Err(InvalidSku::Empty));
        assert_eq!(
            SkuBuilder::new().remove_last_field().finalize(),
            Err(InvalidSku::Empty)
        );
        for blank in ["", " ", "\t", "\u{2003}"] {
            assert_eq!(
                SkuBuilder::new()
                    .add_field("GPU")
                    .add_field(blank)
                    .finalize(),
                Err(InvalidSku::EmptyField { index: 1 })
            );
        }
    }

    #[test]
    fn identity_matches_saved_text_regardless_of_builder_boundaries() {
        let split = SkuBuilder::new()
            .add_field("GPU")
            .add_field("5090")
            .finalize()
            .unwrap();
        let combined = SkuBuilder::new().add_field("GPU-5090").finalize().unwrap();
        let loaded = Sku::new("GPU-5090").unwrap();
        let set: std::collections::HashSet<_> = [split, combined, loaded].into_iter().collect();
        assert_eq!(set.len(), 1);
        // Existing arbitrary nonblank codes still load without being rewritten.
        let text = " gpu-special 001 ";
        assert_eq!(Sku::new(text).unwrap().as_str(), text);
        assert_ne!(Sku::new("GPU").unwrap(), Sku::new("gpu").unwrap());
    }
}
