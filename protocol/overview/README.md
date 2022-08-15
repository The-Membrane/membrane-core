# Overview

By depositing collateral tokens into Membrane’s CDP mechanism, users can mint Collateral Debt Tokens (CDT). These tokens can then be redeposited back into the contract to retrieve the original collateral. \
\
In this sense, the mechanism is roughly analogous to a “Line of Credit”, wherein users can deposit their collateral to receive a line of credit against it. This enables a large amount of flexibility in otherwise rigid token positions, creating unique functionality for the CDT tokens to be used.

### Protocol Functions

The Membrane mechanism is composed of 4 primary parts: **Deposit**, **Mint**, **Repay**, and **Liquidate**.\
\
In the **Deposit** stage, the user can deposit their collateral into the protocol. Users will have the freedom to deposit assets separately or as a “bundle”. Bundling assets together uses their average interest rate and “Loan to Value” (LTV) in proportion, giving users the ability to mitigate volatile asset risk.

In the **Mint** stage, the user can mint CDT tokens up to their _max borrow LTV_. The more tokens minted, the larger the liquidation risk, as more value has been removed from the vault, putting further strain on the existing collateral.

During the **Repay** stage, users can repay the CDT tokens they have minted back into the protocol. This allows the user to reduce their risk of liquidation, and withdraw collateral as long as it doesn’t push their position above the max borrow LTV.

If the LTV ratio does exceed maximum safe ratio (meaning there isn’t enough collateral to safely guarantee the backed value of the CDT assets), the user’s position will be **Liquidated**.
