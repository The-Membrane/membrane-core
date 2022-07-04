# Floating Redemption Price

Taking inspiration from Reflexer Labs, CDL's redemption price will be floating, its "interest rate" or redemption rate will move inverse to market conditions. \
\
The rates won't effect the price in the short term, but the presence of it should put marketing correcting pressures on the CDL holders and CDP owners. \
\
**Ex:** At an initial price of 1.00, if CDL's TWAP is .95, then its redemption rate will be 5%. In other words, in a year at similar market conditions, the redemption price would be 1.05.\
\
**Ex 2:** On the flipside, if the TWAP is 1.05 at the same initial price of 1.00, then the rate is _negative_ 5% or a price of .95 in a year.\
\
If I have an open CDP and I know the redemption price will increase, I'm incentivized to rebuy and/or repay earlier or be in danger of having to pay more in the future. This danger doesn't exist unless market price follows redemption price.&#x20;

The key here, is that the redemption price may not directly affect market price, but it increases the LTV ratio of all CDPs, encouraging users who don't currently own their loan repayment amount to go and acquire it. The users who haven't sold their CDL can simply repay down to their desired LTV.\
\
In a negative rate environment, users are incentivized to sell their CDL, knowing the protocol is devaluing it overtime. In a static redemption price system, Ex 2 would be a normal arb where a user can mint and sell CDL on the market for profit, rebuying to repay the loan as the price falls. The floating rate systems ensures that overtime this arb opportunity increases, independent of continual market price increases.\
\
This system ultimately trades small-scale volatility for long-term stability, without the need for centralized collateral (i.e. existential risk).

[https://medium.com/reflexer-labs/stability-without-pegs-8c6a1cbc7fbd](https://medium.com/reflexer-labs/stability-without-pegs-8c6a1cbc7fbd)\
\
[https://twitter.com/ameensol/status/1420048205127946246?s=20\&t=VVBsx8gveHSZr6hWhzIrNA](https://twitter.com/ameensol/status/1420048205127946246?s=20\&t=VVBsx8gveHSZr6hWhzIrNA)&#x20;

### PID Controller

The PID is what controls the redemption rate changes in the system. Due to its use of real-time data, it'll need to be called by an external participant. Initially this will be incentivized by **MBRN** in multiples of gas cost. In the future this will be fulfilled by cron job sequencers that are getting built around the ecosystem.

\
**Base rate:** Gas cost \
**Market Rate:** Base \* X, where X is a 1% price difference\
\
The 1% delta acts as the minimum for redemption rate updates. There will also be a 2M liquidity minimum for rate updates to curb manipulation.
