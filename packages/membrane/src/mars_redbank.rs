use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Decimal, Uint128};


#[cw_serde]
pub enum ExecuteMsg {
    /// Deposit native coins. Deposited coins must be sent in the transaction
    /// this call is made
    Deposit {
        /// Credit account id (Rover)
        account_id: Option<String>,

        /// Address that will receive the coins
        on_behalf_of: Option<String>,
    },

    /// Withdraw native coins
    Withdraw {
        /// Asset to withdraw
        denom: String,
        /// Amount to be withdrawn. If None is specified, the full amount will be withdrawn.
        amount: Option<Uint128>,
        /// The address where the withdrawn amount is sent
        recipient: Option<String>,
        /// Credit account id (Rover)
        account_id: Option<String>,
        // Withdraw action related to liquidation process initiated in credit manager.
        // This flag is used to identify different way for pricing assets during liquidation.
        liquidation_related: Option<bool>,
    },
}


#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get asset market with underlying collateral and debt amount
    #[returns(MarketV2Response)]
    MarketV2 {
        denom: String,
    },

    /// Get user collateral position for a specific asset
    #[returns(UserCollateralResponse)]
    UserCollateral {
        user: String,
        account_id: Option<String>,
        denom: String,
    },

    // /// Get liquidity scaled amount for a given underlying asset amount.
    // /// (i.e: how much scaled collateral is added if the given amount is deposited)
    // #[returns(Uint128)]
    // ScaledLiquidityAmount {
    //     denom: String,
    //     amount: Uint128,
    // },

    // /// Get underlying asset amount for a given asset and scaled amount.
    // /// (i.e. How much underlying asset will be released if withdrawing by burning a given scaled
    // /// collateral amount stored in state.)
    // #[returns(Uint128)]
    // UnderlyingLiquidityAmount {
    //     denom: String,
    //     amount_scaled: Uint128,
    // },
}

#[cw_serde]
pub struct UserCollateralResponse {
    /// Asset denom
    pub denom: String,
    /// Scaled collateral amount stored in contract state
    pub amount_scaled: Uint128,
    /// Underlying asset amount that is actually deposited at the current block
    pub amount: Uint128,
    /// Wether the user is using asset as collateral or not
    pub enabled: bool,
}
#[cw_serde]
pub struct Market {
    /// Denom of the asset
    pub denom: String,
    /// Portion of the borrow rate that is kept as protocol rewards
    pub reserve_factor: Decimal,

    /// model (params + internal state) that defines how interest rate behaves
    pub interest_rate_model: InterestRateModel,

    /// Borrow index (Used to compute borrow interest)
    pub borrow_index: Decimal,
    /// Liquidity index (Used to compute deposit interest)
    pub liquidity_index: Decimal,
    /// Rate charged to borrowers
    pub borrow_rate: Decimal,
    /// Rate paid to depositors
    pub liquidity_rate: Decimal,
    /// Timestamp (seconds) where indexes and where last updated
    pub indexes_last_updated: u64,

    /// Total collateral scaled for the market's currency
    pub collateral_total_scaled: Uint128,
    /// Total debt scaled for the market's currency
    pub debt_total_scaled: Uint128,
}

#[cw_serde]
#[derive(Eq, Default)]
pub struct InterestRateModel {
    /// Optimal utilization rate
    pub optimal_utilization_rate: Decimal,
    /// Base rate
    pub base: Decimal,
    /// Slope parameter for interest rate model function when utilization_rate <= optimal_utilization_rate
    pub slope_1: Decimal,
    /// Slope parameter for interest rate model function when utilization_rate > optimal_utilization_rate
    pub slope_2: Decimal,
}

#[cw_serde]
pub struct MarketV2Response {
    pub collateral_total_amount: Uint128,
    pub debt_total_amount: Uint128,
    pub utilization_rate: Decimal,

    #[serde(flatten)]
    pub market: Market,
}