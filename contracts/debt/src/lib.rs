#![allow(non_snake_case)]
#![allow(unused_parens)]
#![allow(unused_doc_comments)]
#![allow(non_camel_case_types)]

pub mod contract;
pub mod error;
pub mod state;
pub mod positions;
pub mod rates;
pub mod debt_manager;
pub mod query;
pub mod reply;
pub mod helpers;

pub use error::ContractError;

#[cfg(test)]
pub mod testing;
