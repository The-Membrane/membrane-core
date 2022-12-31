#![allow(non_snake_case)]
#![allow(unused_parens)]
#![allow(unused_doc_comments)]
#![allow(non_camel_case_types)]
pub mod contracts;
mod error;
pub mod state;
mod helpers;

#[cfg(test)]
#[allow(unused_variables)]
pub mod integration_tests;

pub use crate::error::ContractError;
