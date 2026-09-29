#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DiscountsContract;

    use membrane::system_discounts::{ExecuteMsg, InstantiateMsg, QueryMsg, Config, UpdateConfig, UserDiscountResponse};
    use membrane::staking::Config as Staking_Config;
    use membrane::osmosis_proxy::{QueryMsg as OsmosisProxyQueryMsg, TokenInfoResponse};
    use membrane::ltv_disco::{AllUserDepositsResponse, UserDepositInfo, BackingDeposit, QueryMsg as LTVDiscoQueryMsg};
    use membrane::types::StakeDistribution;

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal, Coin,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};

    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    const MBRN_DENOM: &str = "mbrn_denom";

    // Generate valid bech32 addresses for testing using MockApi
    use cosmwasm_std::testing::MockApi;
    fn user_addr() -> Addr {
        MockApi::default().addr_make("user")
    }
    fn admin_addr() -> Addr {
        MockApi::default().addr_make("admin")
    }

    // ======================== Mock Contracts ========================

    // Discounts Contract
    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        );
        Box::new(contract)
    }

    // Mock Staking Contract (returns mbrn_denom in Config)
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {
        Config {},
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::Config {} => {
                        Ok(to_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: MBRN_DENOM.to_string(),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            max_commission_rate: Decimal::zero(),
                            unstaking_period: 0,
                            keep_raw_cdt: false,
                            lock_duration_ceiling: 365,
                            vesting_rev_multiplier: Decimal::one(),
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                            emissions_voting_contract: None,
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Osmosis Proxy Contract (returns configurable total supply)
    thread_local! {
        static MBRN_SUPPLY: std::cell::RefCell<Uint128> = std::cell::RefCell::new(Uint128::new(1_000_000_000));
    }

    pub fn set_mbrn_supply(supply: u128) {
        MBRN_SUPPLY.with(|s| *s.borrow_mut() = Uint128::new(supply));
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct OsmosisProxy_MockInstantiateMsg {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: OsmosisProxy_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _, _: OsmosisProxy_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: OsmosisProxyQueryMsg| -> StdResult<Binary> {
                match msg {
                    OsmosisProxyQueryMsg::GetTokenInfo { denom } => {
                        let supply = MBRN_SUPPLY.with(|s| *s.borrow());
                        Ok(to_binary(&TokenInfoResponse {
                            denom,
                            current_supply: supply,
                            max_supply: Uint128::zero(),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    _ => Ok(to_binary(&"")?),
                }
            },
        );
        Box::new(contract)
    }

    // Mock LTV Disco Contract (returns configurable user deposits)
    thread_local! {
        static LTV_DISCO_DEPOSITS: std::cell::RefCell<HashMap<String, Vec<UserDepositInfo>>> = std::cell::RefCell::new(HashMap::new());
    }

    pub fn set_ltv_disco_deposits(user: String, deposits: Vec<UserDepositInfo>) {
        LTV_DISCO_DEPOSITS.with(|d| d.borrow_mut().insert(user, deposits));
    }

    pub fn clear_ltv_disco_deposits() {
        LTV_DISCO_DEPOSITS.with(|d| d.borrow_mut().clear());
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct LTVDisco_MockInstantiateMsg {}

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: LTVDisco_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _, _: LTVDisco_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LTVDiscoQueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDiscoQueryMsg::GetAllUserDeposits { user } => {
                        let deposits = LTV_DISCO_DEPOSITS.with(|d| {
                            d.borrow().get(&user).cloned().unwrap_or_default()
                        });
                        Ok(to_binary(&AllUserDepositsResponse { deposits })?)
                    },
                    _ => Ok(to_binary(&"")?),
                }
            },
        );
        Box::new(contract)
    }

    // Mock Oracle / CDP (needed by InstantiateMsg but not used by discount logic)
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct GenericMockInstantiateMsg {}

    pub fn generic_mock_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: GenericMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _, _: GenericMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: GenericMockInstantiateMsg| -> StdResult<Binary> { Ok(to_binary(&"")?) },
        );
        Box::new(contract)
    }

    // ======================== Setup ========================

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();
            router.bank = bank;
        })
    }

    /// Setup with optional user MBRN balance and LTV Disco deposits
    fn setup(
        user_mbrn_balance: u128,
        max_discount: Option<Decimal>,
        ratio: Option<Decimal>,
    ) -> (App, DiscountsContract) {
        let mut app = mock_app();

        // Give user MBRN balance
        if user_mbrn_balance > 0 {
            app.init_modules(|router, _, storage| {
                router.bank.init_balance(
                    storage,
                    &user_addr(),
                    vec![coin(user_mbrn_balance, MBRN_DENOM)],
                ).unwrap();
            });
        }

        // Deploy mock contracts
        let staking_id = app.store_code(staking_contract());
        let staking_addr = app.instantiate_contract(staking_id, admin_addr(), &Staking_MockInstantiateMsg {}, &[], "staking", None).unwrap();

        let oracle_id = app.store_code(generic_mock_contract());
        let oracle_addr = app.instantiate_contract(oracle_id, admin_addr(), &GenericMockInstantiateMsg {}, &[], "oracle", None).unwrap();

        let cdp_id = app.store_code(generic_mock_contract());
        let cdp_addr = app.instantiate_contract(cdp_id, admin_addr(), &GenericMockInstantiateMsg {}, &[], "cdp", None).unwrap();

        let proxy_id = app.store_code(osmosis_proxy_contract());
        let proxy_addr = app.instantiate_contract(proxy_id, admin_addr(), &OsmosisProxy_MockInstantiateMsg {}, &[], "proxy", None).unwrap();

        let disco_id = app.store_code(ltv_disco_contract());
        let disco_addr = app.instantiate_contract(disco_id, admin_addr(), &LTVDisco_MockInstantiateMsg {}, &[], "disco", None).unwrap();

        // Deploy system_discounts
        let discounts_id = app.store_code(discounts_contract());
        let msg = InstantiateMsg {
            owner: None, // Defaults to info.sender
            positions_contract: cdp_addr.to_string(),
            oracle_contract: oracle_addr.to_string(),
            staking_contract: staking_addr.to_string(),
            lockdrop_contract: None,
            discount_vault_contract: None,
            ltv_disco_contract: Some(disco_addr.to_string()),
            osmosis_proxy: proxy_addr.to_string(),
            minimum_time_in_network: 7,
            max_discount: max_discount.or(Some(Decimal::percent(30))),
            max_boost: None,
            ratio_of_mbrn_supply_at_max_discount: ratio,
        };

        let discounts_addr = app.instantiate_contract(discounts_id, admin_addr(), &msg, &[], "discounts", None).unwrap();

        (app, DiscountsContract(discounts_addr))
    }

    fn query_discount(app: &App, contract: &DiscountsContract, user: &str) -> UserDiscountResponse {
        app.wrap().query_wasm_smart(
            contract.addr(),
            &QueryMsg::UserDiscount { user: user.to_string() },
        ).unwrap()
    }

    fn make_disco_deposit(deposit_tokens: u128) -> UserDepositInfo {
        UserDepositInfo {
            asset: "mbrn".to_string(),
            slot: 50,
            deposit_id: Uint128::one(),
            deposit: BackingDeposit {
                user: user_addr(),
                vault_tokens: Uint128::new(deposit_tokens),
                last_claimed: 0,
                start_time: 0,
                deposit_time: None,
                compound_claims: false,
                manager: None,
                depositor: None,
                withdrawals_enabled: true,
                revenue_destination: None,
            },
            deposit_tokens: Uint128::new(deposit_tokens),
        }
    }

    // ======================== Tests ========================

    mod linear_discount {
        use super::*;

        #[test]
        fn zero_balance_zero_disco_returns_zero() {
            // Total supply = 1B, threshold = 1% = 10M, user has 0
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(0, None, None);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::zero());
        }

        #[test]
        fn wallet_only_at_half_threshold() {
            // Supply = 1B, ratio = 1%, threshold = 10M, user wallet = 5M → 50% of 30% = 15%
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(5_000_000, None, None);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(15));
        }

        #[test]
        fn wallet_at_full_threshold() {
            // Supply = 1B, threshold = 10M, user wallet = 10M → 100% of 30% = 30%
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(10_000_000, None, None);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(30));
        }

        #[test]
        fn wallet_over_threshold_capped() {
            // Supply = 1B, threshold = 10M, user wallet = 20M → capped at 30%
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(20_000_000, None, None);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(30));
        }

        #[test]
        fn disco_only_at_full_threshold() {
            // User wallet = 0, disco deposits = 10M
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(0, None, None);

            set_ltv_disco_deposits(user_addr().to_string(), vec![make_disco_deposit(10_000_000)]);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(30));

            clear_ltv_disco_deposits();
        }

        #[test]
        fn combined_wallet_and_disco() {
            // Wallet = 3M, Disco = 2M, total = 5M, threshold = 10M → 50% of 30% = 15%
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(3_000_000, None, None);

            set_ltv_disco_deposits(user_addr().to_string(), vec![make_disco_deposit(2_000_000)]);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(15));

            clear_ltv_disco_deposits();
        }

        #[test]
        fn zero_total_supply_returns_zero() {
            set_mbrn_supply(0);
            let (app, contract) = setup(10_000_000, None, None);

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::zero());
        }

        #[test]
        fn custom_max_discount_and_ratio() {
            // max_discount = 50%, ratio = 2% → threshold = 20M
            // user = 10M → 50% of 50% = 25%
            set_mbrn_supply(1_000_000_000);
            let (app, contract) = setup(
                10_000_000,
                Some(Decimal::percent(50)),
                Some(Decimal::percent(2)),
            );

            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(25));
        }
    }

    mod static_and_timed_discounts {
        use super::*;

        #[test]
        fn static_discount_overrides_calculated() {
            set_mbrn_supply(1_000_000_000);
            let (mut app, contract) = setup(5_000_000, None, None);

            // Set a static discount of 50% for USER
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                positions_contract: None,
                oracle_contract: None,
                staking_contract: None,
                lockdrop_contract: None,
                discount_vault_contract: None,
                ltv_disco_contract: None,
                osmosis_proxy: None,
                minimum_time_in_network: None,
                max_discount: None,
                max_boost: None,
                static_discount: Some(UserDiscountResponse {
                    user: user_addr().to_string(),
                    discount: Decimal::percent(50),
                }),
                ratio_of_mbrn_supply_at_max_discount: None,
            });
            let cosmos_msg = contract.call(msg, vec![]).unwrap();
            app.execute(admin_addr(), cosmos_msg).unwrap();

            // Static discount (50%) should override calculated (15%)
            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(50));
        }

        #[test]
        fn timed_discount_overrides_calculated() {
            set_mbrn_supply(1_000_000_000);
            let (mut app, contract) = setup(5_000_000, None, None);

            // Set a timed discount period of 40%
            let msg = ExecuteMsg::SetDiscountPeriod {
                start_time: None,
                duration: 60, // 60 minutes
                discount: Decimal::percent(40),
            };
            let cosmos_msg = contract.call(msg, vec![]).unwrap();
            app.execute(admin_addr(), cosmos_msg).unwrap();

            // Timed discount (40%) should override calculated (15%)
            let res = query_discount(&app, &contract, &user_addr().to_string());
            assert_eq!(res.discount, Decimal::percent(40));
        }
    }
}
