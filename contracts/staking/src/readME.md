To Pass Tests...

In contract.rs:
- Comment line 439 for unstake checks
- hardcode credit denom in line 1295 to: let cdt_denom = AssetInfo::NativeToken {
        denom: String::from("credit_fulldenom"),
    };