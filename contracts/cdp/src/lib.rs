#![allow(non_snake_case)]
#![allow(unused_parens)]
#![allow(unused_doc_comments)]
#![allow(non_camel_case_types)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::inconsistent_digit_grouping)]
pub mod contract;
mod error;
pub mod helpers;
pub mod liquidations;
pub mod positions;
pub mod query;
pub mod state;
pub mod reply;
pub mod rates;
pub mod risk_engine;

pub use crate::error::ContractError;

#[cfg(test)]
#[allow(unused_variables)]
pub mod testing;
