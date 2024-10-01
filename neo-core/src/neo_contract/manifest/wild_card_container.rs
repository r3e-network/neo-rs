use std::marker::PhantomData;
use serde_json::Value;
use neo_json::json_convert_trait::JsonConvertibleTrait;
use neo_json::json_error::JsonError;

/// A container that supports wildcard.
#[derive(Clone, Debug)]
pub struct WildcardContainer<T> {
    data: Option<Vec<T>>,
    _phantom: PhantomData<T>,
}

impl<T> WildcardContainer<T> {
    /// Indicates whether the container is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        self.data.is_none()
    }

    /// Creates a new instance with the initial elements.
    pub fn create(data: Vec<T>) -> Self {
        Self {
            data: Some(data),
            _phantom: PhantomData,
        }
    }

    /// Creates a new instance with wildcard.
    pub fn create_wildcard() -> Self {
        Self {
            data: None,
            _phantom: PhantomData,
        }
    }
}

impl<T> JsonConvertibleTrait for WildcardContainer<T> {
    fn to_json(&self) -> Value {
        if self.is_wildcard() {
            serde_json::Value::String("*".to_string())
        } else {
            serde_json::Value::Array(
                self.data
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|item| item.to_json())
                    .collect()
            )
        }
    }

    fn from_json(json: &Value) -> Result<Self, JsonError>
    where
        Self: Sized,
        T: JsonConvertibleTrait,
    {
        match json {
            serde_json::Value::String(s) if s == "*" => Ok(Self::create_wildcard()),
            serde_json::Value::Array(array) => {
                let data = array
                    .iter()
                    .map(|item| T::from_json(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::create(data))
            }
            _ => Err(JsonError::InvalidFormat),
        }
    }
}

impl<T> std::ops::Index<usize> for WildcardContainer<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data.as_ref().expect("Cannot index a wildcard container")[index]
    }
}

impl<T> IntoIterator for WildcardContainer<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.unwrap_or_default().into_iter()
    }
}

impl<'a, T> IntoIterator for &'a WildcardContainer<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.as_ref().map_or_else(|| [].iter(), |v| v.iter())
    }
}

impl<T> FromIterator<T> for WildcardContainer<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::create(iter.into_iter().collect())
    }
}
