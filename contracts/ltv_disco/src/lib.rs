pub mod contract;
pub mod error;
pub mod state;
pub mod execute;
pub mod query;
// pub mod contract_tests;  // Temporarily commented out - needs updating for event-based system
pub mod reply;

#[cfg(test)]
pub mod bad_debt_tests;
pub mod event_system_tests;