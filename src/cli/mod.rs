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

mod config;
mod error;
pub mod fungible;
mod runtime;
pub mod stash;

pub use config::{Config, Opts};
pub use error::Error;
pub use runtime::Runtime;

#[derive(ArgEnum, Copy, Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display(Debug)]
pub enum OutputFormat {
    Yaml,
    Json,
    Toml,
    Csv,
    Tsv,
    Bech32,
    PrettyPrint,
    StrictEncode,
}
