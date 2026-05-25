//! Caching module - matches C# Neo.IO.Caching exactly

macro_rules! impl_cache_wrapper_deref {
    (
        impl<$($generic:ident),+> for $wrapper:ty
        where { $($bounds:tt)* }
        => $target:ty
    ) => {
        impl<$($generic),+> ::std::ops::Deref for $wrapper
        where
            $($bounds)*
        {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        impl<$($generic),+> ::std::ops::DerefMut for $wrapper
        where
            $($bounds)*
        {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner
            }
        }
    };
}

pub mod cache;
pub mod ec_point_cache;
pub mod ecdsa_cache;
pub mod fifo_cache;
pub mod hashset_cache;
pub mod lru_cache;
pub mod relay_cache;
