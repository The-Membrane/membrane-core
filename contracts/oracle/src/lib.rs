#![allow(non_snake_case)]
#![allow(unused_parens)]
#![allow(unused_doc_comments)]
#![allow(non_camel_case_types)]
pub mod contracts;
pub mod error;
pub mod helpers;
pub mod state;

#[cfg(test)]
#[allow(unused_variables)]
pub mod integration_tests;
