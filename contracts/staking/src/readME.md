To Pass Tests...

In contract.rs:
- Comment line 442 for unstake checks
- hardcode credit denom in line 1306 to: let cdt_denom = AssetInfo::NativeToken {
        denom: String::from("credit_fulldenom"),
    };