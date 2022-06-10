// RGB standard library
// Written in 2019-2022 by
//     Dr. Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

mod cache;
mod file;
#[cfg(feature = "sql")]
mod sql;

pub use cache::{Cache, CacheError};
pub use file::{FileCache, FileCacheConfig, FileCacheError};
#[cfg(feature = "sql")]
pub use sql::{SqlCache, SqlCacheConfig, SqlCacheError};
