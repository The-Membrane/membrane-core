mod tests {

    use crate::helpers::OPContract;

    use membrane::oracle::PriceResponse;
    use membrane::osmosis_proxy::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::types::{AssetInfo, Asset, Basket, LiquidityInfo, Owner, SupplyCap};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //OP Contract
    pub fn op_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //Mock Liquidity Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Liquidity_MockExecuteMsg {
        AddAsset { asset: LiquidityInfo },
        EditAsset { asset: LiquidityInfo },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Liquidity_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Liquidity_MockQueryMsg {
        Liquidity { asset: AssetInfo },
    }

    pub fn liquidity_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Liquidity_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Liquidity_MockExecuteMsg::AddAsset { asset } => Ok(Response::default()),
                    Liquidity_MockExecuteMsg::EditAsset { asset } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Liquidity_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Liquidity_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Liquidity_MockQueryMsg::Liquidity { asset } => {
                        Ok(to_binary(&Uint128::new(49999u128))?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockExecuteMsg {  }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Oracle_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            basket_id: Option<Uint128>,
        }
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Price {
                        asset_info,
                        twap_timeframe,
                        basket_id,
                    } => Ok(to_binary(&PriceResponse {
                        prices: vec![],
                        price: Decimal::one(),
                        decimals: 6,
                    })?)
                }
            },
        );
        Box::new(contract)
    }

    //Mock Positions Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        Config {},
        GetBasket {},
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::new())
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    CDP_MockQueryMsg::Config {} => {
                        Ok(to_binary(&membrane::cdp::Config {
                            owner: Addr::unchecked(ADMIN),
                            oracle_contract: None,
                            stability_pool: None,
                            dex_router: None,
                            staking_contract: None,
                            osmosis_proxy: None,
                            debt_auction: None,
                            liquidity_contract: None,
                            discounts_contract: None,
                            liq_fee: Decimal::zero(),
                            collateral_twap_timeframe: 60,
                            credit_twap_timeframe: 60,
                            oracle_time_limit: 60,
                            cpc_multiplier: Decimal::zero(),
                            debt_minimum: Uint128::zero(),
                            base_debt_cap_multiplier: Uint128::zero(),
                            rate_slope_multiplier: Decimal::zero(),
                        })?)
                        },
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::zero(),
                            current_position_id: Uint128::zero(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![  SupplyCap {
                                asset_info: AssetInfo::NativeToken { denom: String::from("factory/collateral/#1") },
                                current_supply: Uint128::one(),
                                debt_total: Uint128::zero(),
                                stability_pool_ratio_for_debt_cap: None,
                                supply_cap_ratio: Decimal::zero(),
                                lp: false
                            } ],
                            lastest_collateral_rates: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("factory/cdt/#1") }, amount: Uint128::zero() },
                            credit_price: PriceResponse { 
                                prices: vec![], 
                                price: Decimal::one(), 
                                decimals: 6
                            },
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: Uint128::zero(),
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(100_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract2"),
                vec![coin(100_000_000, "credit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, OPContract) {
        let mut app = mock_app();

        //Instaniate CDP
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Liquidity contract
        let liq_id = app.store_code(liquidity_contract());

        let liq_contract_addr = app
            .instantiate_contract(
                liq_id,
                Addr::unchecked(ADMIN),
                &Liquidity_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Liquidity contract
        let oracle_id = app.store_code(oracle_contract());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Osmosis Proxy contract
        let op_id = app.store_code(op_contract());

        let msg = InstantiateMsg {};

        let op_contract_addr = app
            .instantiate_contract(
                op_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let op_contract = OPContract(op_contract_addr);

        //Instaniate CDP Again
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        let msg = ExecuteMsg::UpdateConfig { 
            owners: None, 
            liquidity_multiplier: None,
            add_owner: Some(true), 
            debt_auction: Some(String::from("debt_auction")),
            positions_contract: Some(String::from("contract0")),
            liquidity_contract: Some(String::from("contract1")),
            oracle_contract: Some(String::from("contract2")),
        };
        let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        (app, op_contract)
    }

    mod token_handler {

        //use cosmwasm_std::coins;
        use membrane::osmosis_proxy::OwnerResponse;

        use super::*;

        
        #[test]
        fn query_owner_liquidity_multiplier() {
            let (mut app, op_contract) = proper_instantiate();

            //Query non-CDP Owner Liquidity Multiplier's
            let owner: OwnerResponse = app
                .wrap()
                .query_wasm_smart(
                    op_contract.addr(),
                    &QueryMsg::GetOwner { owner: ADMIN.into() },
                )
                .unwrap();           
            assert_eq!(
                owner.liquidity_multiplier,
                Decimal::zero()
            );            

            //Add new Owner
            let msg = ExecuteMsg::UpdateConfig { 
                owners: Some(vec![Owner { 
                    owner: Addr::unchecked("contract0"), 
                    total_minted: Uint128::zero(), 
                    stability_pool_ratio: None, 
                    non_token_contract_auth: false, 
                    is_position_contract: true
                }]), 
                liquidity_multiplier: None,
                add_owner: Some(true), 
                debt_auction: None,
                positions_contract: None,
                liquidity_contract: None,
                oracle_contract: None,
            };
            let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query CDP Owner Liquidity Multiplier's
            let owner: OwnerResponse = app
                .wrap()
                .query_wasm_smart(
                    op_contract.addr(),
                    &QueryMsg::GetOwner { owner: "contract0".into() },
                )
                .unwrap();
            assert_eq!(
                owner.liquidity_multiplier,
                Decimal::one()
            );

            //Add new Owner
            let msg = ExecuteMsg::UpdateConfig { 
                owners: Some(vec![Owner { 
                    owner: Addr::unchecked("contract4"), 
                    total_minted: Uint128::zero(), 
                    stability_pool_ratio: None, 
                    non_token_contract_auth: false, 
                    is_position_contract: true
                }]), 
                liquidity_multiplier: None,
                add_owner: Some(true), 
                debt_auction: None,
                positions_contract: None,
                liquidity_contract: None,
                oracle_contract: None,
            };
            let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query CDP Owner Liquidity Multiplier's
            let owner: OwnerResponse = app
                .wrap()
                .query_wasm_smart(
                    op_contract.addr(),
                    &QueryMsg::GetOwner { owner: "contract0".into() },
                )
                .unwrap();
            assert_eq!(
                owner.liquidity_multiplier,
                Decimal::percent(50)
            );
        }

        // #[test]
        // fn mint_with_owner_limits() {
        //     let (mut app, op_contract) = proper_instantiate();

        //     //Create Denom
        //     let msg = ExecuteMsg::CreateDenom { 
        //         subdenom: String::from("cdt"), 
        //         max_supply: Some(Uint128::new(10)), 
        //     };
        //     let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        //     //Mint tokens as ADMIN: Error due to 0 multiplier
        //     let msg = ExecuteMsg::MintTokens { denom: String::from("factory/cdt/#1"), amount: 100u128.into(), mint_to_address: String::from("creator") };
        //     let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

        //     //Edit Owner's liquidity multipler
        //     let msg = ExecuteMsg::EditOwner { 
        //         owner: String::from(ADMIN), 
        //         liquidity_multiplier: Some(Decimal::one()), 
        //         stability_pool_ratio: Some(Decimal::zero()),
        //         non_token_contract_auth: None 
        //     };
        //     let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        //     //Mint tokens as ADMIN: Success
        //     let msg = ExecuteMsg::MintTokens { denom: String::from("factory/cdt/#1"), amount: 100u128.into(), mint_to_address: String::from("creator") };
        //     let cosmos_msg = op_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        //     //Assert Config
        //     let expected_config = Config {
        //         owners: vec![ Owner {
        //             owner: Addr::unchecked(ADMIN),
        //             total_minted: Uint128::new(100),
        //             liquidity_multiplier: Some(Decimal::one()),
        //             stability_pool_ratio: Some(Decimal::zero()),
        //             non_token_contract_auth: true, 
        //         }],
        //         debt_auction: Some(Addr::unchecked("debt_auction")),
        //         positions_contract: Some(Addr::unchecked("contract0")),
        //         liquidity_contract: Some(Addr::unchecked("contract1")),
        //     };
        //     let config: Config = app
        //         .wrap()
        //         .query_wasm_smart(
        //             op_contract.addr(),
        //             &QueryMsg::Config {  },
        //         )
        //         .unwrap();           
        //     assert_eq!(
        //         config,
        //         expected_config
        //     );           
            
        // }
    }

}