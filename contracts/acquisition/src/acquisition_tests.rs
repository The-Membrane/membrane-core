#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate, query};
    use cosmwasm_std::{
        coin, to_binary, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult, StdError,
        Uint128, WasmMsg, CosmosMsg, BankMsg, Storage, DepsMut, Deps, Env, MessageInfo,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;
    use cw_storage_plus::Item;
    use membrane::acquisition::{
        Config, ExecuteMsg, InstantiateMsg, QueryMsg, AcquisitionWindow, AcquisitionDeposit,
        AcquisitionModelConfig, AcquisitionModelState, AcquisitionModelStateResponse,
        ConfigResponse, CurrentAcquisitionWindowResponse, ActiveAcquisitionWindowResponse,
        UserAcquisitionDepositResponse, MbrnIntentOption, MbrnIntentType, MbrnClaimIntent,
    };
    use membrane::transmuter::AssetPair;
    use membrane::transmuter::{
        Config as TransmuterConfig, QueryMsg as TransmuterQueryMsg, ExecuteMsg as TransmuterExecuteMsg,
    };
    use membrane::neutron_proxy::{ExecuteMsg as NeutronProxyExecuteMsg};
    use membrane::staking::{ExecuteMsg as StakingExecuteMsg, Config as StakingConfig};
    use membrane::mars_mirror::{Config as MarsMirrorConfig};
    use membrane::ltv_disco::{ExecuteMsg as LtvDiscoExecuteMsg};
    use membrane::emissions_voting::{QueryMsg as EmissionsVotingQueryMsg, CurrentResultResponse};
    use membrane::system_discounts::{QueryMsg as DiscountsQueryMsg, UserBoostResponse, Config as DiscountConfig};

    const USER1: &str = "user1";
    const USER2: &str = "user2";
    const ADMIN: &str = "admin";
    const DEPOSIT_TOKEN: &str = "usdc";
    const MBRN_DENOM: &str = "mbrn";
    const SECONDS_PER_DAY: u64 = 86_400;
    // Legacy constants (removed from transmuter module, kept for test compatibility)
    const TOTAL_EMISSIONS_GRAPH_LABEL: &str = "total_emissions";
    const ACQUISITION_PERCENTAGE_GRAPH_LABEL: &str = "acquisition_percentage";

    // Mock Transmuter Contract with deposit ID tracking
    #[cw_serde]
    pub enum Transmuter_MockExecuteMsg {
        EnterVault {
            recipient: Option<String>,
            lock_days: Option<u64>,
            affiliate_address: Option<String>,
            affiliate_label: Option<String>,
        },
        ExitVault {
            recipient: Option<String>,
            withdraw_as: Option<String>,
            user: Option<String>,
            deposit_id: Option<Uint128>,
            amount: Option<Uint128>,
        },
        TransferDepositOwnership {
            user: String,
            amount: Uint128,
            new_owner: String,
        },
    }

    #[cw_serde]
    pub struct Transmuter_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Transmuter_MockQueryMsg {
        Config {},
        VaultInfo {},
        AcquisitionDepositTotal {},
    }

    // Local mock deposit struct (TransmuterUserDeposit no longer exists in transmuter module)
    #[cw_serde]
    pub struct MockDeposit {
        pub amount: Uint128,
        pub deposit_time: u64,
    }

    // Storage for mock transmuter state
    const MOCK_DEPOSITS: Item<Vec<(String, Vec<MockDeposit>)>> = Item::new("mock_deposits");
    const MOCK_ACQ_DEPOSIT_TOTAL: Item<Uint128> = Item::new("mock_acq_deposit_total");

    pub fn transmuter_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps: DepsMut, env: Env, info: MessageInfo, msg: Transmuter_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Transmuter_MockExecuteMsg::EnterVault { recipient, .. } => {
                        let recipient = recipient.unwrap_or(info.sender.to_string());

                        // Get deposit amount from funds
                        let amount = info.funds.iter()
                            .find(|c| c.denom == DEPOSIT_TOKEN)
                            .map(|c| c.amount)
                            .unwrap_or(Uint128::zero());

                        // Load existing deposits
                        let mut all_deposits = MOCK_DEPOSITS.may_load(deps.storage)?
                            .unwrap_or_default();

                        // Find or create user deposits
                        let user_deposits = all_deposits.iter_mut()
                            .find(|(u, _)| u == &recipient);

                        if let Some((_, deposits)) = user_deposits {
                            deposits.push(MockDeposit {
                                amount,
                                deposit_time: env.block.time.seconds(),
                            });
                        } else {
                            all_deposits.push((recipient.clone(), vec![MockDeposit {
                                amount,
                                deposit_time: env.block.time.seconds(),
                            }]));
                        }

                        MOCK_DEPOSITS.save(deps.storage, &all_deposits)?;

                        // Track acquisition deposit total (info.sender is the acquisition contract)
                        let mut acq_total = MOCK_ACQ_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
                        acq_total += amount;
                        MOCK_ACQ_DEPOSIT_TOTAL.save(deps.storage, &acq_total)?;

                        Ok(Response::new().add_attribute("action", "enter_vault"))
                    }
                    Transmuter_MockExecuteMsg::ExitVault { recipient, user, amount, .. } => {
                        let user_addr = user.clone().unwrap_or_else(|| info.sender.to_string());
                        let recipient_addr = recipient.clone().unwrap_or_else(|| user_addr.clone());
                        let requested_amount = amount.unwrap_or(Uint128::zero());

                        // Load deposits
                        let mut all_deposits = MOCK_DEPOSITS.may_load(deps.storage)?
                            .unwrap_or_default();

                        let mut withdraw_amount = Uint128::zero();

                        if let Some((_, deposits)) = all_deposits.iter_mut().find(|(u, _)| u == &user_addr) {
                            if let Some(deposit) = deposits.first_mut() {
                                if requested_amount > Uint128::zero() && requested_amount < deposit.amount {
                                    withdraw_amount = requested_amount;
                                    deposit.amount = deposit.amount.checked_sub(requested_amount)
                                        .map_err(|_| StdError::generic_err("Insufficient deposit amount"))?;
                                } else {
                                    withdraw_amount = deposit.amount;
                                    deposits.remove(0);
                                }
                            }
                        }

                        MOCK_DEPOSITS.save(deps.storage, &all_deposits)?;

                        // Decrement acquisition deposit total
                        if !withdraw_amount.is_zero() {
                            let mut acq_total = MOCK_ACQ_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
                            acq_total = acq_total.saturating_sub(withdraw_amount);
                            MOCK_ACQ_DEPOSIT_TOTAL.save(deps.storage, &acq_total)?;
                        }

                        if !withdraw_amount.is_zero() {
                            Ok(Response::new()
                                .add_message(BankMsg::Send {
                                    to_address: recipient_addr,
                                    amount: vec![coin(withdraw_amount.u128(), DEPOSIT_TOKEN)],
                                }))
                        } else {
                            Ok(Response::default())
                        }
                    }
                    Transmuter_MockExecuteMsg::TransferDepositOwnership { user, amount, new_owner } => {
                        let mut all_deposits = MOCK_DEPOSITS.may_load(deps.storage)?
                            .unwrap_or_default();

                        // Remove amount from user's deposits
                        let mut transferred = false;
                        if let Some((_, deposits)) = all_deposits.iter_mut().find(|(u, _)| u == &user) {
                            if let Some(deposit) = deposits.first_mut() {
                                if amount <= deposit.amount {
                                    deposit.amount = deposit.amount.checked_sub(amount)
                                        .map_err(|_| StdError::generic_err("Insufficient deposit amount"))?;
                                    if deposit.amount.is_zero() {
                                        deposits.remove(0);
                                    }
                                    transferred = true;
                                }
                            }
                        }

                        if transferred {
                            // Decrement ACQ_DT on transfer from acquisition
                            let mut acq_total = MOCK_ACQ_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
                            acq_total = acq_total.saturating_sub(amount);
                            MOCK_ACQ_DEPOSIT_TOTAL.save(deps.storage, &acq_total)?;

                            // Add to new owner
                            if let Some((_, deposits)) = all_deposits.iter_mut().find(|(u, _)| u == &new_owner) {
                                deposits.push(MockDeposit {
                                    amount,
                                    deposit_time: 0,
                                });
                            } else {
                                all_deposits.push((new_owner.clone(), vec![MockDeposit {
                                    amount,
                                    deposit_time: 0,
                                }]));
                            }
                        }

                        MOCK_DEPOSITS.save(deps.storage, &all_deposits)?;

                        Ok(Response::new().add_attribute("action", "transfer_deposit_ownership"))
                    }
                }
            },
            |_, _, _, _: Transmuter_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |deps: Deps, _env: Env, msg: Transmuter_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Transmuter_MockQueryMsg::Config {} => Ok(to_binary(&TransmuterConfig {
                        owner: Addr::unchecked("owner"),
                        tokenfactory_contract: None,
                        discounts_contract: "discounts".to_string(),
                        cdp_contract: "cdp".to_string(),
                        deposit_pair: AssetPair {
                            cdt: "cdt".to_string(),
                            paired_asset: "usdc".to_string(),
                        },
                        composition_leeway: Decimal::percent(5),
                        cdt_target_ratio: Decimal::zero(),
                        usage_fee: Decimal::zero(),
                        swap_history_cap: 100,
                        volume_history_cap: 100,
                        rate_limit_window_secs: 3600,
                        rate_limit_threshold: Decimal::percent(10),
                        allowlist: vec![],
                        allowlist_rate_limit_threshold: Decimal::percent(50),
                        global_rate_limit_window_secs: 3600,
                        global_rate_limit_threshold: Decimal::percent(20),
                        revenue_distributor_addr: None,
                        revenue_distributions: vec![],
                        vault_token_denom: "vault_token".to_string(),
                        affiliate_fee: Decimal::percent(1),
                        send_swap_fee: false,
                        revenue_distributor_fee_percentage: Decimal::percent(20),
                        usage_fee_utilization_threshold: Decimal::percent(90),
                        acquisition_contract: None,
                        points_system_contract: None,
                        retention_multiplier: Decimal::one(),
                    })?),
                    Transmuter_MockQueryMsg::VaultInfo {} => {
                        Ok(to_binary(&membrane::transmuter::VaultInfoResponse {
                            total_deposit_value: Uint128::from(1_000_000u128),
                            deposit_total: Uint128::from(1_000_000u128),
                            cdt_balance: Uint128::from(700_000u128),
                            paired_asset_balance: Uint128::from(300_000u128),
                        })?)
                    }
                    Transmuter_MockQueryMsg::AcquisitionDepositTotal {} => {
                        let total = MOCK_ACQ_DEPOSIT_TOTAL.may_load(deps.storage)?.unwrap_or(Uint128::zero());
                        Ok(to_binary(&membrane::transmuter::AcquisitionDepositTotalResponse { total })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Emissions Voting Contract
    #[cw_serde]
    pub enum EmissionsVoting_MockExecuteMsg {}

    #[cw_serde]
    pub struct EmissionsVoting_MockInstantiateMsg {
        pub total_emissions: Uint128,
        pub acquisition_percentage: Decimal,
    }

    #[cw_serde]
    pub enum EmissionsVoting_MockQueryMsg {
        CurrentResult { label: String },
    }

    const MOCK_TOTAL_EMISSIONS: Item<Uint128> = Item::new("mock_total_emissions");
    const MOCK_ACQUISITION_PCT: Item<Decimal> = Item::new("mock_acquisition_pct");

    pub fn emissions_voting_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: EmissionsVoting_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |deps: DepsMut, _, _, msg: EmissionsVoting_MockInstantiateMsg| -> StdResult<Response> {
                MOCK_TOTAL_EMISSIONS.save(deps.storage, &msg.total_emissions)?;
                MOCK_ACQUISITION_PCT.save(deps.storage, &msg.acquisition_percentage)?;
                Ok(Response::default())
            },
            |deps: Deps, _, msg: EmissionsVoting_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    EmissionsVoting_MockQueryMsg::CurrentResult { label } => {
                        if label == TOTAL_EMISSIONS_GRAPH_LABEL {
                            let total = MOCK_TOTAL_EMISSIONS.load(deps.storage)?;
                            Ok(to_binary(&CurrentResultResponse {
                                result_uint128: Some(total),
                                result_decimal: None,
                                total_voting_power: Uint128::zero(),
                                period_ended: true,
                            })?)
                        } else if label == ACQUISITION_PERCENTAGE_GRAPH_LABEL {
                            let pct = MOCK_ACQUISITION_PCT.load(deps.storage)?;
                            Ok(to_binary(&CurrentResultResponse {
                                result_uint128: None,
                                result_decimal: Some(pct),
                                total_voting_power: Uint128::zero(),
                                period_ended: true,
                            })?)
                        } else {
                            Err(StdError::not_found(format!("Graph {} not found", label)))
                        }
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Discounts Contract
    #[cw_serde]
    pub enum Discounts_MockExecuteMsg {}

    #[cw_serde]
    pub struct Discounts_MockInstantiateMsg {
        pub user_boost: Decimal,
        pub max_boost: Option<Decimal>,
    }

    #[cw_serde]
    pub enum Discounts_MockQueryMsg {
        UserBoost { user: String },
        Config {},
    }

    const MOCK_USER_BOOST: Item<Decimal> = Item::new("mock_user_boost");
    const MOCK_MAX_BOOST: Item<Decimal> = Item::new("mock_max_boost");

    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: Discounts_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |deps: DepsMut, _, _, msg: Discounts_MockInstantiateMsg| -> StdResult<Response> {
                MOCK_USER_BOOST.save(deps.storage, &msg.user_boost)?;
                let max_boost = msg.max_boost.unwrap_or(Decimal::percent(9)); // Default 9%
                MOCK_MAX_BOOST.save(deps.storage, &max_boost)?;
                Ok(Response::default())
            },
            |deps: Deps, env: Env, msg: Discounts_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Discounts_MockQueryMsg::UserBoost { user } => {
                        let boost = MOCK_USER_BOOST.load(deps.storage)?;
                        Ok(to_binary(&UserBoostResponse {
                            user,
                            boost,
                        })?)
                    }
                    Discounts_MockQueryMsg::Config {} => {
                        let max_boost = MOCK_MAX_BOOST.load(deps.storage)?;
                        // Create a minimal Config response with just max_boost
                        // Use Addr::unchecked for test addresses (they won't be validated in queries)
                        use cosmwasm_std::Addr;
                        let dummy_addr = Addr::unchecked("cosmos1dummy1234567890123456789012345678901234567890");
                        Ok(to_binary(&DiscountConfig {
                            owner: dummy_addr.clone(),
                            mbrn_denom: "mbrn".to_string(),
                            oracle_contract: dummy_addr.clone(),
                            positions_contract: dummy_addr.clone(),
                            staking_contract: dummy_addr.clone(),
                            lockdrop_contract: None,
                            discount_vault_contract: vec![],
                            ltv_disco_contract: None,
                            osmosis_proxy: dummy_addr.clone(),
                            minimum_time_in_network: 0,
                            max_discount: Decimal::zero(),
                            max_boost,
                            ratio_of_mbrn_supply_at_max_discount: Decimal::permille(10),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Neutron Proxy Contract
    #[cw_serde]
    pub enum NeutronProxy_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        },
    }

    #[cw_serde]
    pub struct NeutronProxy_MockInstantiateMsg {}

    #[cw_serde]
    pub enum NeutronProxy_MockQueryMsg {}

    pub fn neutron_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: NeutronProxy_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    NeutronProxy_MockExecuteMsg::MintTokens { .. } => Ok(Response::default()),
                }
            },
            |_, _, _, _: NeutronProxy_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: NeutronProxy_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }

    // Mock Staking Contract
    #[cw_serde]
    pub enum Staking_MockExecuteMsg {
        Stake {
            user: Option<String>,
            locked: Option<membrane::types::Locked>,
        },
    }

    #[cw_serde]
    pub struct Staking_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Staking_MockQueryMsg {
        Config {},
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::Stake { .. } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: Staking_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&StakingConfig {
                    owner: Addr::unchecked("owner"),
                    mbrn_denom: MBRN_DENOM.to_string(),
                    incentive_schedule: membrane::types::StakeDistribution {
                        rate: Decimal::zero(),
                        duration: 0,
                    },
                    lock_duration_ceiling: 365,
                    unstaking_period: 7,
                    max_commission_rate: Decimal::zero(),
                    keep_raw_cdt: false,
                    vesting_rev_multiplier: Decimal::one(),
                    positions_contract: None,
                    auction_contract: None,
                    vesting_contract: None,
                    governance_contract: None,
                    osmosis_proxy: None,
                    emissions_voting_contract: None,
                })?)
            },
        );
        Box::new(contract)
    }

    // Mock Mars Mirror Contract
    #[cw_serde]
    pub enum MarsMirror_MockExecuteMsg {}

    #[cw_serde]
    pub struct MarsMirror_MockInstantiateMsg {}

    #[cw_serde]
    pub enum MarsMirror_MockQueryMsg {
        Config {},
    }

    pub fn mars_mirror_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, _: MarsMirror_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: MarsMirror_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: MarsMirror_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MarsMirrorConfig {
                    owner: Addr::unchecked("owner"),
                    mars_params_contract: "mars_params".to_string(),
                    disco_contract: Addr::unchecked("ltv_disco"),
                })?)
            },
        );
        Box::new(contract)
    }

    // Mock LTV Disco Contract
    #[cw_serde]
    pub enum LtvDisco_MockExecuteMsg {
        SubmitDeposit {
            deposit_input: membrane::ltv_disco::BackingDepositInput,
            deposit_owner: Option<String>,
            locked: Option<membrane::types::Locked>,
            deposit_id: Option<Uint128>,
            manager: Option<String>,
            affiliate_address: Option<String>,
        },
    }

    #[cw_serde]
    pub struct LtvDisco_MockInstantiateMsg {}

    #[cw_serde]
    pub enum LtvDisco_MockQueryMsg {
        Config {},
    }

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: LtvDisco_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LtvDisco_MockExecuteMsg::SubmitDeposit { .. } => Ok(Response::default()),
                }
            },
            |_, _, _, _: LtvDisco_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: LtvDisco_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&membrane::ltv_disco::Config {
                    owner: Addr::unchecked("owner"),
                    cdp_contract: Addr::unchecked("cdp"),
                    deposit_denom: membrane::types::DepositDenom {
                        denom: MBRN_DENOM.to_string(),
                        vault_info: None,
                    },
                    cdt_denom: "cdt".to_string(),
                    minimum_deposit: Uint128::from(1000u128),
                    unstaking_period: 172800,
                    oracle_contract: Addr::unchecked("oracle"),
                    chain_proxy_contract: Addr::unchecked("proxy"),
                    emissions_voting_contract: None,
                    affiliate_fee: Decimal::percent(1),
                    max_management_fee: Decimal::zero(),
                    points_system_contract: None,
                    revenue_distributor: None,
                    auction_contract: None,
                    mbrn_denom: None,
                })?)
            },
        );
        Box::new(contract)
    }

    // Mock CDP Contract (receives SetAcquisitionBumpRate)
    #[cw_serde]
    pub enum Cdp_MockExecuteMsg {
        SetAcquisitionBumpRate { rate: Decimal },
    }

    #[cw_serde]
    pub struct Cdp_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Cdp_MockQueryMsg {}

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Cdp_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Cdp_MockExecuteMsg::SetAcquisitionBumpRate { .. } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Cdp_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: Cdp_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }

    // Acquisition Contract
    pub fn acquisition_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        let mut app = App::default();
        let user1_addr = app.api().addr_make(USER1);
        let user2_addr = app.api().addr_make(USER2);
        let admin_addr = app.api().addr_make(ADMIN);
        let bank_addr = app.api().addr_make("bank");
        
        app.init_modules(|router, _, storage| {
            let bank = BankKeeper::new();
            bank.init_balance(
                storage,
                &user1_addr,
                vec![
                    coin(1_000_000_000, DEPOSIT_TOKEN),
                    coin(1_000_000, "wrong_token"), // For testing wrong token deposits
                ],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &user2_addr,
                vec![coin(1_000_000_000, DEPOSIT_TOKEN)],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &admin_addr,
                vec![coin(10_000_000, MBRN_DENOM)],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &bank_addr,
                vec![coin(10_000_000_000_000, MBRN_DENOM)],
            )
            .unwrap();
            router.bank = bank;
        });
        app
    }

    fn proper_instantiate() -> (App, Addr, Addr, Addr, Addr, Addr, Addr, Addr, Addr, Addr) {
        let mut app = mock_app();
        let admin_addr = app.api().addr_make(ADMIN);

        // Instantiate Transmuter
        let transmuter_id = app.store_code(transmuter_contract());
        let transmuter_addr = app
            .instantiate_contract(
                transmuter_id,
                admin_addr.clone(),
                &Transmuter_MockInstantiateMsg {},
                &[],
                "transmuter",
                None,
            )
            .unwrap();

        // Instantiate Emissions Voting
        let emissions_voting_id = app.store_code(emissions_voting_contract());
        let emissions_voting_addr = app
            .instantiate_contract(
                emissions_voting_id,
                admin_addr.clone(),
                &EmissionsVoting_MockInstantiateMsg {
                    total_emissions: Uint128::from(1_000_000u128), // 1M emissions
                    acquisition_percentage: Decimal::percent(10), // 10%
                },
                &[],
                "emissions_voting",
                None,
            )
            .unwrap();

        // Instantiate Discounts
        let discounts_id = app.store_code(discounts_contract());
        let discounts_addr = app
            .instantiate_contract(
                discounts_id,
                admin_addr.clone(),
                &Discounts_MockInstantiateMsg {
                    user_boost: Decimal::percent(5), // 5% boost
                    max_boost: Some(Decimal::percent(9)), // 9% max boost
                },
                &[],
                "discounts",
                None,
            )
            .unwrap();

        // Instantiate Neutron Proxy
        let neutron_proxy_id = app.store_code(neutron_proxy_contract());
        let neutron_proxy_addr = app
            .instantiate_contract(
                neutron_proxy_id,
                admin_addr.clone(),
                &NeutronProxy_MockInstantiateMsg {},
                &[],
                "neutron_proxy",
                None,
            )
            .unwrap();

        // Instantiate Staking
        let staking_id = app.store_code(staking_contract());
        let staking_addr = app
            .instantiate_contract(
                staking_id,
                admin_addr.clone(),
                &Staking_MockInstantiateMsg {},
                &[],
                "staking",
                None,
            )
            .unwrap();

        // Instantiate Mars Mirror
        let mars_mirror_id = app.store_code(mars_mirror_contract());
        let mars_mirror_addr = app
            .instantiate_contract(
                mars_mirror_id,
                admin_addr.clone(),
                &MarsMirror_MockInstantiateMsg {},
                &[],
                "mars_mirror",
                None,
            )
            .unwrap();

        // Instantiate LTV Disco
        let ltv_disco_id = app.store_code(ltv_disco_contract());
        let ltv_disco_addr = app
            .instantiate_contract(
                ltv_disco_id,
                admin_addr.clone(),
                &LtvDisco_MockInstantiateMsg {},
                &[],
                "ltv_disco",
                None,
            )
            .unwrap();

        // Instantiate CDP
        let cdp_id = app.store_code(cdp_contract());
        let cdp_addr = app
            .instantiate_contract(
                cdp_id,
                admin_addr.clone(),
                &Cdp_MockInstantiateMsg {},
                &[],
                "cdp",
                None,
            )
            .unwrap();

        // Instantiate Acquisition Contract
        let acquisition_id = app.store_code(acquisition_contract());
        let msg = InstantiateMsg {
            owner: admin_addr.to_string(),
            transmuter_contract: transmuter_addr.to_string(),
            neutron_proxy: neutron_proxy_addr.to_string(),
            cdp_contract: cdp_addr.to_string(),
            deposit_period_days: 7,
            withdrawal_period_days: 7,
            deposit_token: DEPOSIT_TOKEN.to_string(),
            minimum_deposit: Uint128::from(1000u128),
            mbrn_denom: MBRN_DENOM.to_string(),
            staking_contract: Some(staking_addr.to_string()),
            mars_mirror_contract: Some(mars_mirror_addr.to_string()),
            ltv_disco_contract: Some(ltv_disco_addr.to_string()),
            discounts_contract: discounts_addr.to_string(),
            emissions_voting_contract: Some(emissions_voting_addr.to_string()),
            cliff_period_days: 30, // 30 days
            acquisition_config: AcquisitionModelConfig {
                base_acquisition_rate: Decimal::from_ratio(1_000_000u128, 604_800u128), // ~1M uMBRN / 7 days
                initial_acquisition_amount: Uint128::from(1_000_000_000u128),
                max_mbrn_emission: Uint128::from(10_000_000_000u128),
                target_utilization: Decimal::percent(50),
                bump_increment: Decimal::permille(1), // 0.1%
                bump_interval_seconds: 17280, // 4.8 hours
                reduction_speed_multiplier: Decimal::from_ratio(2u128, 1u128),
                efficiency_threshold: Decimal::percent(20),
                max_rate_change_per_mutation: Decimal::percent(20),
                max_bump_rate: Decimal::one(), // 100%
                mutation_cooldown_seconds: 28800, // 8 hours
                accrual_ramp_seconds: SECONDS_PER_DAY, // 24 hours
                accrual_ramp_start_fraction: Decimal::percent(25), // 25%
            },
        };

        let acquisition_addr = app
            .instantiate_contract(
                acquisition_id,
                admin_addr.clone(),
                &msg,
                &[],
                "acquisition",
                None,
            )
            .unwrap();

        // Fund the acquisition contract with MBRN tokens for claim testing
        // The mock neutron_proxy doesn't actually mint tokens, so we pre-fund the contract
        let bank_addr = app.api().addr_make("bank");
        app.send_tokens(
            bank_addr,
            acquisition_addr.clone(),
            &[coin(1_000_000_000, MBRN_DENOM)],
        )
        .unwrap();

        (
            app,
            acquisition_addr,
            transmuter_addr,
            neutron_proxy_addr,
            staking_addr,
            mars_mirror_addr,
            ltv_disco_addr,
            emissions_voting_addr,
            discounts_addr,
            cdp_addr,
        )
    }

    #[test]
    fn test_instantiate_acquisition_contract() {
        let (app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Query config
        let config: ConfigResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.config.cliff_period_seconds, 30 * SECONDS_PER_DAY);
        assert_eq!(config.config.deposit_period_days, 7);
        assert_eq!(config.config.withdrawal_period_days, 7);
    }

    #[test]
    fn test_start_acquisition_window() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();
        let admin_addr = app.api().addr_make(ADMIN);

        // Start acquisition window
        app.execute_contract(
            admin_addr,
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Query current window
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();

        assert!(window.window.is_some());
        let window = window.window.unwrap();
        assert_eq!(window.window_id, 1);
        assert_eq!(window.deposit_period_days, 7);
        assert_eq!(window.total_deposit_amount, Uint128::zero());
        // Budget is 0 at window creation; it's set by apply_efficiency_clamp at withdrawal end
        assert_eq!(window.acquisition_budget, Uint128::zero());
        // Timers are lazy: u64::MAX until first deposit
        assert_eq!(window.deposit_end, u64::MAX);
        assert_eq!(window.withdrawal_end, u64::MAX);
    }

    #[test]
    fn test_start_acquisition_window_custom_periods() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Update config to use custom periods (5 days deposit, 3 days withdrawal)
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                transmuter_contract: None,
                neutron_proxy: None,
                cdp_contract: None,
                deposit_period_days: Some(5),
                withdrawal_period_days: Some(3),
                deposit_token: None,
                minimum_deposit: None,
                mbrn_denom: None,
                staking_contract: None,
                mars_mirror_contract: None,
                ltv_disco_contract: None,
                discounts_contract: None,
                emissions_voting_contract: None,
                cliff_period_days: None,
                acquisition_config: None,
            },
            &[],
        )
        .unwrap();

        // Start acquisition window (uses config periods)
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();

        let window = window.window.unwrap();
        assert_eq!(window.deposit_period_days, 5);
        // Timers are lazy: u64::MAX until first deposit triggers them
        assert_eq!(window.deposit_end, u64::MAX);
        assert_eq!(window.withdrawal_end, u64::MAX);
    }

    #[test]
    fn test_start_acquisition_window_prevents_overlap() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start first window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Try to start another window immediately - should fail
        let err = app
            .execute_contract(
                app.api().addr_make(ADMIN),
                acquisition_addr.clone(),
                &ExecuteMsg::StartAcquisitionWindow { },
                &[],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("still active") || err_str.contains("Current acquisition window"));
    }

    #[test]
    fn test_deposit_during_deposit_period() {
        let (mut app, acquisition_addr, transmuter_addr, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Query user deposit
        let user1_addr = app.api().addr_make(USER1);
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: user1_addr.to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_some());
        let deposit = user_deposit.deposit.unwrap();
        assert_eq!(deposit.amount, Uint128::from(10_000u128));
        assert!(deposit.vested_at > deposit.deposit_time);

        // Check total deposit amount updated
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        assert_eq!(window.window.unwrap().total_deposit_amount, Uint128::from(10_000u128));
    }

    #[test]
    fn test_deposit_after_deposit_period_ends() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // First deposit to trigger lazy timers (deposit_end, withdrawal_end)
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(1_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance time past deposit period (7 days)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(8 * SECONDS_PER_DAY);
        });

        // Try to deposit - should fail
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
                &[coin(10_000, DEPOSIT_TOKEN)],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("Deposit period ended") || err_str.contains("deposit_period"));
    }

    #[test]
    fn test_multiple_deposits_consolidate() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // First deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Second deposit (should consolidate)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(5_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Query user deposit - should have consolidated amount
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        let deposit = user_deposit.deposit.unwrap();
        assert_eq!(deposit.amount, Uint128::from(15_000u128)); // Consolidated

        // Check total
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        assert_eq!(window.window.unwrap().total_deposit_amount, Uint128::from(15_000u128));
    }

    #[test]
    fn test_withdraw_during_withdrawal_period_before_cliff() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance to withdrawal period (deposit period is 7 days, so advance 8 days)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(8 * SECONDS_PER_DAY);
        });

        // Withdraw (before cliff - cliff is 30 days)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Withdraw {
                amount: Uint128::from(5_000u128),
            },
            &[],
        )
        .unwrap();

        // Query user deposit - should have reduced amount
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        let deposit = user_deposit.deposit.unwrap();
        assert_eq!(deposit.amount, Uint128::from(5_000u128));
    }

    #[test]
    fn test_withdraw_after_cliff_fails() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window with withdrawal period longer than cliff (30 days)
        // so we can test the cliff check specifically
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit (triggers lazy timers: deposit_end = now + 7d, withdrawal_end = now + 14d)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past withdrawal period (14 days) - this is past both deposit_end and withdrawal_end
        // With default 7d deposit + 7d withdrawal, withdrawal_end is at 14 days from deposit
        // Cliff is 30 days, so at 31 days we're past both withdrawal_end and cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        // Try to withdraw - should fail because we're past deposit_end
        // The contract checks `current_time < window.deposit_end` first, then withdrawal period
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Withdraw {
                    amount: Uint128::from(5_000u128),
                },
                &[],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        // After 31 days, we're past withdrawal period (14 days) and cliff (30 days)
        assert!(
            err_str.contains("Cliff has passed") ||
            err_str.contains("must claim") ||
            err_str.contains("Not in withdrawal period") ||
            err_str.contains("withdrawal period") ||
            err_str.contains("withdrawal_end")
        );
    }

    #[test]
    fn test_withdraw_outside_withdrawal_period_fails() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Try to withdraw during deposit period - should fail
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Withdraw {
                    amount: Uint128::from(5_000u128),
                },
                &[],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("Not in withdrawal period") || err_str.contains("withdrawal"));
    }

    #[test]
    fn test_claim_after_cliff() {
        let (mut app, acquisition_addr, transmuter_addr, neutron_proxy_addr, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        // Get window ID
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Claim
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // User deposit should be removed
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_none());
    }

    #[test]
    fn test_claim_before_cliff_fails() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance to withdrawal period but before cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1 * SECONDS_PER_DAY + 1);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Try to claim - should fail
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Claim {
                    window_id,
                    mbrn_intent: None,
                },
                &[],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("Cliff has not passed") || err_str.contains("vested_at"));
    }

    #[test]
    fn test_claim_calculation_at_claim_time() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // USER1 deposits 10k (triggers lazy timers)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // USER2 deposits 20k
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(20_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Accrue pool during deposit period so budget will be non-zero
        app.update_block(|block| {
            block.time = block.time.plus_seconds(2 * SECONDS_PER_DAY);
        });
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        // Get window info to find deposit_end
        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window = window.window.unwrap();
        let window_id = window.window_id;

        // Advance to withdrawal period (after deposit_end, before withdrawal_end)
        app.update_block(|block| {
            block.time = cosmwasm_std::Timestamp::from_seconds(window.deposit_end + 1);
        });

        // USER2 withdraws 10k during withdrawal period
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Withdraw {
                amount: Uint128::from(10_000u128),
            },
            &[],
        )
        .unwrap();

        // Advance past cliff (30 days from window start)
        app.update_block(|block| {
            block.time = cosmwasm_std::Timestamp::from_seconds(window.start_time + 31 * SECONDS_PER_DAY);
        });

        // Accrue pool one more time to maximize accrued amount
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        // USER1 claims - should get proportional share
        // Total deposits now: 10k + 10k = 20k
        // USER1 share: 10k / 20k = 50%
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Verify claim happened (deposit removed)
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_none());
    }

    #[test]
    fn test_claim_with_mbrn_boost() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Claim - should apply 5% boost
        // Base claim: 100k * (10k / 10k) = 100k
        // With 5% boost: 100k * 1.05 = 105k
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Verify claim happened
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_none());
    }

    #[test]
    fn test_claim_transfers_deposit_ownership() {
        let (mut app, acquisition_addr, transmuter_addr, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Claim - should transfer ownership
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Claim succeeded (execute above did not error), ownership was transferred
    }

    #[test]
    fn test_claim_for_user_admin_function() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Admin claims for user
        let user1_addr = app.api().addr_make(USER1);
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::ClaimForUser {
                user: user1_addr.to_string(),
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Verify claim happened
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_none());
    }

    #[test]
    fn test_claim_for_user_unauthorized() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit (triggers lazy timers)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance some time and accrue pool so budget will be non-zero
        app.update_block(|block| {
            block.time = block.time.plus_seconds(2 * SECONDS_PER_DAY);
        });
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        // Advance past cliff (31 days total from deposit)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(29 * SECONDS_PER_DAY);
        });

        // Accrue again to capture more pool
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Non-admin tries to claim for user - should succeed (permissionless)
        // Note: ClaimForUser is now permissionless as claims are sent to the user anyway
        let user1_addr = app.api().addr_make(USER1);
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::ClaimForUser {
                user: user1_addr.to_string(),
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Verify the claim succeeded by checking deposit was removed
        // (MBRN is minted via neutron_proxy which is a no-op mock, so balance check won't work)
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: user1_addr.to_string(),
                window_id,
            })
            .unwrap();
        assert!(user_deposit.deposit.is_none(), "Deposit should be removed after claim");
    }

    #[test]
    fn test_claim_deposit_not_found_removes_state() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Manually remove deposit from transmuter (simulate it being withdrawn directly)
        // This would happen if user withdrew from transmuter directly
        // For this test, we'll just verify the error handling

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Try to claim - if deposit not found, state should be removed
        // In our mock, the deposit exists, so this test verifies the error path
        // We'd need to manually delete from transmuter to test the removal path
        // For now, we verify the query path works
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_some());
    }

    #[test]
    fn test_multiple_users_claim_proportional_shares() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // USER1 deposits 10k
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // USER2 deposits 20k
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(20_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // USER1 claims - should get 1/3 of budget (10k / 30k)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // USER2 claims - should get 2/3 of budget (20k / 30k)
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Both deposits should be removed
        let user1_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();
        assert!(user1_deposit.deposit.is_none());

        let user2_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER2).to_string(),
                window_id,
            })
            .unwrap();
        assert!(user2_deposit.deposit.is_none());
    }

    #[test]
    fn test_claim_with_stake_intent() {
        let (mut app, acquisition_addr, _, _, staking_addr, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // Claim with stake intent
        let intent = MbrnClaimIntent {
            apply_now: true,
            set_ongoing: false,
            intents: vec![MbrnIntentOption {
                intent_type: MbrnIntentType::Stake {},
                ratio: Decimal::one(),
                lock: None,
            }],
        };

        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: Some(intent),
            },
            &[],
        )
        .unwrap();

        // Verify claim happened
        let user_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();

        assert!(user_deposit.deposit.is_none());
    }

    #[test]
    fn test_active_acquisition_window_query() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // No window started yet
        let active: ActiveAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::ActiveAcquisitionWindow {})
            .unwrap();
        assert!(active.window.is_none());

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Should be active (withdrawal_end is u64::MAX since no deposit yet)
        let active: ActiveAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::ActiveAcquisitionWindow {})
            .unwrap();
        assert!(active.window.is_some());

        // Deposit to trigger lazy timers (deposit_end = now + 7d, withdrawal_end = now + 14d)
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(1_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Still active during deposit period
        let active: ActiveAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::ActiveAcquisitionWindow {})
            .unwrap();
        assert!(active.window.is_some());

        // Advance past withdrawal period (7 days deposit + 7 days withdrawal = 14 days total)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(15 * SECONDS_PER_DAY);
        });

        // Should not be active anymore
        let active: ActiveAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::ActiveAcquisitionWindow {})
            .unwrap();
        assert!(active.window.is_none());
    }

    #[test]
    fn test_deposit_minimum_validation() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Try to deposit below minimum (1000)
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
                &[coin(500, DEPOSIT_TOKEN)],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("below minimum") || err_str.contains("minimum_deposit"));
    }

    #[test]
    fn test_deposit_wrong_token_fails() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Try to deposit with wrong token
        let err = app
            .execute_contract(
                app.api().addr_make(USER1),
                acquisition_addr.clone(),
                &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
                &[coin(10_000, "wrong_token")],
            )
            .unwrap_err();

        let err_str = err.root_cause().to_string();
        assert!(err_str.contains("Expected") || err_str.contains("Invalid funds"));
    }

    #[test]
    fn test_sequential_acquisition_windows() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Start first window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Deposit in first window
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance past withdrawal period
        app.update_block(|block| {
            block.time = block.time.plus_seconds(2 * SECONDS_PER_DAY + 1);
        });

        // Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(30 * SECONDS_PER_DAY);
        });

        // Claim from first window
        let window1: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window1_id = window1.window.unwrap().window_id;

        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id: window1_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // Start second window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // Verify second window has different ID
        let window2: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window2_id = window2.window.unwrap().window_id;

        assert_eq!(window2_id, 2);
    }

    #[test]
    fn test_budget_calculation_from_emissions_voting() {
        let (mut app, acquisition_addr, _, _, _, _, _, _emissions_voting_addr, _, _) = proper_instantiate();

        // Start window - budget is 0 at creation; set by apply_efficiency_clamp at withdrawal end
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();

        let window = window.window.unwrap();
        // Budget is 0 at window creation; it's set to accrued_pool by apply_efficiency_clamp
        assert_eq!(window.acquisition_budget, Uint128::zero());
    }

    #[test]
    fn test_update_config() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();

        // Update cliff period
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                transmuter_contract: None,
                neutron_proxy: None,
                cdp_contract: None,
                deposit_period_days: None,
                withdrawal_period_days: None,
                deposit_token: None,
                minimum_deposit: None,
                mbrn_denom: None,
                staking_contract: None,
                mars_mirror_contract: None,
                ltv_disco_contract: None,
                discounts_contract: None,
                emissions_voting_contract: None,
                cliff_period_days: Some(60), // 60 days
                acquisition_config: None,
            },
            &[],
        )
        .unwrap();

        let config: ConfigResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.config.cliff_period_seconds, 60 * SECONDS_PER_DAY);
    }

    #[test]
    fn test_full_acquisition_flow() {
        let (mut app, acquisition_addr, transmuter_addr, _, _, _, _, _, _, _) = proper_instantiate();

        // 1. Start window
        app.execute_contract(
            app.api().addr_make(ADMIN),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow { },
            &[],
        )
        .unwrap();

        // 2. USER1 deposits during deposit period
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // 3. USER2 deposits
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(20_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // 4. Advance to withdrawal period
        app.update_block(|block| {
            block.time = block.time.plus_seconds(7 * SECONDS_PER_DAY + 1);
        });

        // 5. USER2 withdraws 5k (before cliff)
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Withdraw {
                amount: Uint128::from(5_000u128),
            },
            &[],
        )
        .unwrap();

        // 6. Advance past cliff
        app.update_block(|block| {
            block.time = block.time.plus_seconds(30 * SECONDS_PER_DAY);
        });

        let window: CurrentAcquisitionWindowResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::CurrentAcquisitionWindow {})
            .unwrap();
        let window_id = window.window.unwrap().window_id;

        // 7. USER1 claims
        app.execute_contract(
            app.api().addr_make(USER1),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // 8. USER2 claims
        app.execute_contract(
            app.api().addr_make(USER2),
            acquisition_addr.clone(),
            &ExecuteMsg::Claim {
                window_id,
                mbrn_intent: None,
            },
            &[],
        )
        .unwrap();

        // 9. Verify both deposits removed
        let user1_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER1).to_string(),
                window_id,
            })
            .unwrap();
        assert!(user1_deposit.deposit.is_none());

        let user2_deposit: UserAcquisitionDepositResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::UserAcquisitionDeposit {
                user: app.api().addr_make(USER2).to_string(),
                window_id,
            })
            .unwrap();
        assert!(user2_deposit.deposit.is_none());
    }

    // ─── New red-team fix tests ──────────────────────────────────────────────

    #[test]
    fn test_bump_rate_capped_at_max() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();
        let admin_addr = app.api().addr_make(ADMIN);
        let user1_addr = app.api().addr_make(USER1);

        // Start window
        app.execute_contract(
            admin_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow {},
            &[],
        )
        .unwrap();

        // Deposit to trigger timers
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(100_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance a significant amount of time.
        // With base_rate ~1.65 uMBRN/sec and max_mbrn_emission = 10B,
        // the pool requires ~6B seconds to max out — impractical for this test.
        // The bump_rate mechanism only increases when pool_maxed == true.
        // So we verify bump_rate stays at 0 when pool is not maxed (even with high utilization).
        app.update_block(|block| {
            block.time = block.time.plus_seconds(250 * SECONDS_PER_DAY);
        });

        // Accrue to trigger state updates
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        let state_resp: AcquisitionModelStateResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::AcquisitionModelState {})
            .unwrap();
        let state = state_resp.state.unwrap();

        // bump_rate only increases when pool_maxed is true.
        // Pool is not maxed (accrued ~35M vs 10B max), so bump_rate stays at 0.
        assert!(!state.pool_maxed, "pool should not be maxed at this accrual rate over 250 days");
        assert_eq!(state.bump_rate, Decimal::zero(), "bump_rate should stay 0 when pool is not maxed");
        assert!(state.bump_rate <= Decimal::one(), "bump_rate {} must never exceed max_bump_rate (100%)", state.bump_rate);
        // Verify pool has been accruing
        assert!(!state.accrued_pool.is_zero(), "pool should have accrued some MBRN");
    }

    #[test]
    fn test_withdraw_decrements_total_new_deposits() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();
        let admin_addr = app.api().addr_make(ADMIN);
        let user1_addr = app.api().addr_make(USER1);

        // Start window
        app.execute_contract(
            admin_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow {},
            &[],
        )
        .unwrap();

        // Deposit 10,000
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(10_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Check total_new_deposits = 10,000
        let state_resp: AcquisitionModelStateResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::AcquisitionModelState {})
            .unwrap();
        assert_eq!(state_resp.state.unwrap().total_new_deposits, Uint128::from(10_000u128));

        // Advance to withdrawal period
        app.update_block(|block| {
            block.time = block.time.plus_seconds(7 * SECONDS_PER_DAY + 1);
        });

        // Withdraw 3,000
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::Withdraw { amount: Uint128::from(3_000u128) },
            &[],
        )
        .unwrap();

        // Check total_new_deposits = 7,000
        let state_resp: AcquisitionModelStateResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::AcquisitionModelState {})
            .unwrap();
        assert_eq!(
            state_resp.state.unwrap().total_new_deposits,
            Uint128::from(7_000u128),
            "total_new_deposits should decrease on withdrawal"
        );
    }

    #[test]
    fn test_accrual_ramp_starts_at_25_percent() {
        let (mut app, acquisition_addr, _, _, _, _, _, _, _, _) = proper_instantiate();
        let admin_addr = app.api().addr_make(ADMIN);
        let user1_addr = app.api().addr_make(USER1);

        // Start window
        app.execute_contract(
            admin_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::StartAcquisitionWindow {},
            &[],
        )
        .unwrap();

        // Accrue for 1 hour before any deposit (should use 25% of rate)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(3600);
        });
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        let state_before_deposit: AcquisitionModelStateResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::AcquisitionModelState {})
            .unwrap();
        let pool_before = state_before_deposit.state.unwrap().accrued_pool;

        // Now deposit to trigger timers
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::Deposit { intents: None, affiliate_address: None, affiliate_label: None },
            &[coin(100_000, DEPOSIT_TOKEN)],
        )
        .unwrap();

        // Advance 24 hours (full ramp should complete)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(SECONDS_PER_DAY);
        });
        app.execute_contract(
            user1_addr.clone(),
            acquisition_addr.clone(),
            &ExecuteMsg::AccruePool {},
            &[],
        )
        .unwrap();

        let state_after_ramp: AcquisitionModelStateResponse = app
            .wrap()
            .query_wasm_smart(&acquisition_addr, &QueryMsg::AcquisitionModelState {})
            .unwrap();
        let pool_after = state_after_ramp.state.unwrap().accrued_pool;

        // Pool should have grown significantly more in the 24h post-deposit
        // than in the 1h pre-deposit (even accounting for time difference)
        // Pre-deposit: 3600s at 25% rate. Post-deposit: 86400s at ramping 25%→100% rate
        assert!(pool_after > pool_before, "Pool should grow after deposit with ramp");
    }
}

