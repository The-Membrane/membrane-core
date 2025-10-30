#![allow(non_snake_case)]
#![allow(unused_parens)]
#![allow(unused_doc_comments)]
#![allow(non_camel_case_types)]
pub mod vesting;
pub mod auction;
pub mod governance;
pub mod liq_queue;
pub mod liquidity_check;
pub mod oracle;
pub mod mm_oracle;
pub mod mm_swap;
pub mod osmosis_proxy;
pub mod cdp;
pub mod stability_pool;
pub mod stability_pool_vault;
pub mod mars_vault_token;
pub mod stable_earn_vault;
pub mod vol_earn_vault;
pub mod range_bound_lp_vault;
pub mod mars_redbank;
pub mod staking;
pub mod market_manager;
pub mod points_system;
pub mod system_discounts;
pub mod launch;
pub mod discount_vault;
pub mod helpers;
pub mod types;
pub mod math;
pub mod managed_market;
pub mod tokenfactory;
pub mod car;
pub mod race_engine;
pub mod rps_engine;
pub mod traits_engine;
pub mod track_manager;
pub mod tournament;
pub mod byte_minter;
pub mod revenue_distributor;
pub mod deployable_venue;
pub mod chain_proxy;
pub mod neutron_proxy;
pub mod neutron_oracle;
pub mod ltv_disco;
pub mod transmuter;
pub mod yield_arb;