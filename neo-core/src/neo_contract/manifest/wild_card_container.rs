// Copyright (C) 2015-2024 The Neo Project.
//
// wild_card_container.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;
use neo::json::Json;
use std::marker::PhantomData;

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

    /// Converts the container from a JSON object.
    pub fn from_json<F>(json: &Json, element_selector: F) -> Result<Self, Error>
    where
        F: Fn(&Json) -> Result<T, Error>,
    {
        match json {
            Json::String(s) if s == "*" => Ok(Self::create_wildcard()),
            Json::Array(array) => {
                let data = array
                    .iter()
                    .map(element_selector)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::create(data))
            }
            _ => Err(Error::Format),
        }
    }

    /// Converts the container to a JSON object.
    pub fn to_json<F>(&self, element_selector: F) -> Json
    where
        F: Fn(&T) -> Json,
    {
        if self.is_wildcard() {
            Json::String("*".to_string())
        } else {
            Json::Array(
                self.data
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(element_selector)
                    .collect(),
            )
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
