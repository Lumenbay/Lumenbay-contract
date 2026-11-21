#![no_std]
#![allow(missing_docs)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::unnecessary_cast)]
#![allow(dead_code)]

//! # Lumenbay Escrow Contract
//!
//! The public contract interface is implemented across domain-focused modules.

use soroban_sdk::contract;

mod admin;
mod errors;
mod escrow;
mod fees;
mod lifecycle;
mod mediation;
mod token_controls;
mod types;
mod utilities;

pub use errors::ContractError;
pub use types::*;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_integer_safety;
#[cfg(test)]
mod test_volume;

/// The Lumenbay escrow contract.
///
/// Its public interface is implemented by the domain modules declared above.
#[contract]
pub struct Contract;

soroban_sdk::contractmeta!(key = "name", val = "Lumenbay Escrow");
soroban_sdk::contractmeta!(
    key = "description",
    val = "Soroban escrow for the Lumenbay P2P marketplace: buyer-funded escrow, delivery release, disputes and configurable fees."
);
soroban_sdk::contractmeta!(
    key = "homepage",
    val = "https://lumenbay-frontend.vercel.app"
);
soroban_sdk::contractmeta!(
    key = "repository",
    val = "https://github.com/Lumenbay/Lumenbay-contract"
);
soroban_sdk::contractmeta!(
    key = "source",
    val = "https://github.com/Lumenbay/Lumenbay-contract/tree/main/contracts/marketx"
);
soroban_sdk::contractmeta!(key = "version", val = "v1.0.0");
