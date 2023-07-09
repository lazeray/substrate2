//! Caching utilities.
#![warn(missing_docs)]

use std::{any::Any, fmt::Debug, hash::Hash, sync::Arc};

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

pub mod disk;
pub mod error;
pub mod mem;

/// A handle to a cache entry that might still be generating.
#[derive(Debug)]
pub struct CacheHandle<V, E>(Arc<OnceCell<Result<V, E>>>);

impl<V, E> Clone for CacheHandle<V, E> {
    fn clone(&self) -> Self {
        CacheHandle(self.0.clone())
    }
}

impl<V, E> CacheHandle<V, E> {
    /// Blocks on the cache entry, returning the result once it is ready.
    ///
    /// Returns an error if one was returned by the generator.
    pub fn try_get(&self) -> Result<&V, &E> {
        self.0.wait().as_ref()
    }

    /// Checks whether the underlying entry is ready.
    ///
    /// Returns the entry if available, otherwise returns [`None`].
    pub fn poll(&self) -> Option<&Result<V, E>> {
        self.0.get()
    }
}

impl<V, E: Debug> CacheHandle<V, E> {
    /// Blocks on the cache entry, returning its output.
    ///
    /// # Panics
    ///
    /// Panics if an error was returned by the generator.
    pub fn get(&self) -> &V {
        self.try_get().unwrap()
    }
}

impl<V: Debug, E> CacheHandle<V, E> {
    /// Blocks on the cache entry, returning the error thrown during generaiton.
    ///
    /// # Panics
    ///
    /// Panics if no error was returned by the generator.
    pub fn get_err(&self) -> &E {
        self.try_get().unwrap_err()
    }
}

/// A cacheable object.
///
/// # Examples
///
/// ```
/// use cache::Cacheable;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize, Hash, Eq, PartialEq)]
/// pub struct Params {
///     param1: u64,
///     param2: String,
/// };
///
/// impl Cacheable for Params {
///     type Output = u64;
///     type Error = anyhow::Error;
///
///     fn generate(&self) -> anyhow::Result<u64> {
///         println!("Executing an expensive computation...");
///
///         // ...
///         # let error_condition = true;
///         # let computation_result = 64;
///
///         if error_condition {
///             anyhow::bail!("an error occured during computation");
///         }
///
///         Ok(computation_result)
///     }
/// }
/// ```
pub trait Cacheable: Serialize + Deserialize<'static> + Hash + Eq + Send + Sync + Any {
    /// The output produced by generating the object.
    type Output: Send + Sync + Serialize + Deserialize<'static>;
    /// The error type returned by [`Cacheable::generate`].
    type Error: Send + Sync;

    /// Generates the output of the cacheable object.
    fn generate(&self) -> Result<Self::Output, Self::Error>;
}

/// A cacheable object whose generator needs to store state.
///
/// # Examples
///
/// ```
/// use std::sync::{Arc, Mutex};
/// use cache::CacheableWithState;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize, Clone, Hash, Eq, PartialEq)]
/// pub struct Params {
///     param1: u64,
///     param2: String,
/// };
///
/// #[derive(Clone)]
/// pub struct Log(Arc<Mutex<Vec<Params>>>);
///
/// impl CacheableWithState<Log> for Params {
///     type Output = u64;
///     type Error = anyhow::Error;
///
///     fn generate_with_state(&self, state: Log) -> anyhow::Result<u64> {
///         println!("Logging parameters...");
///         state.0.lock().unwrap().push(self.clone());
///
///         println!("Executing an expensive computation...");
///
///         // ...
///         # let error_condition = true;
///         # let computation_result = 64;
///
///         if error_condition {
///             anyhow::bail!("an error occured during computation");
///         }
///
///         Ok(computation_result)
///     }
/// }
/// ```
pub trait CacheableWithState<S: Send + Sync + Any>:
    Serialize + Deserialize<'static> + Hash + Eq + Send + Sync + Any
{
    /// The output produced by generating the object.
    type Output: Send + Sync + Serialize + Deserialize<'static>;
    /// The error type returned by [`CacheableWithState::generate_with_state`].
    type Error: Send + Sync;

    /// Generates the output of the cacheable object using `state`.
    ///
    /// **Note:** The state is not used to determine whether the object should be regenerated. As
    /// such, it should not impact the output of this function but rather should only be used to
    /// store collateral or reuse computation from other function calls.
    fn generate_with_state(&self, state: S) -> Result<Self::Output, Self::Error>;
}
