use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

use membrane::types::{DebtAuction, FeeAuction};
use membrane::auction::Config;

pub const CONFIG: Item<Config> = Item::new("config");
pub const DEBT_AUCTION: Item<DebtAuction> = Item::new("ongoing_debt_auction"); //DebtAuction
pub const FEE_AUCTIONS: Map<String, FeeAuction> = Map::new("ongoing_fee_auction"); //AssetInfo, FeeAuction

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
