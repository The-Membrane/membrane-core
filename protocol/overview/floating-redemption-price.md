# Floating Redemption Price

Membrane uses a "Reflex-Index", pioneered by Reflexer Labs, to regulate the relative price movement of CDT. As the redemption price is floating, its "interest rate" or redemption rate will move inverse to market conditions. \
\
The presence of a positive or negative redemption rate incentivizes market participants to remove or contribute CDT to the circulating supply in order to regulate desired price action over the long run. \
\
**Ex:** At an initial price of 1.00, if CDT's TWAP is .95, then its redemption rate will be 5%. In other words, in a year at similar market conditions, the redemption price would be 1.05.\
\
**Ex 2:** On the flipside, if the TWAP is 1.05 at the same initial price of 1.00, then the rate is _negative_ 5% or a price of .95 in a year.\


If rates are positive, users with open CDP positions are incentivized to rebuy and/or repay earlier, as the redemption price will increase, meaning users will have to pay more in the future to close their positions. This danger doesn't exist unless market price follows redemption price.&#x20;

The redemption price may not directly affect market price, but it increases the LTV ratio of all CDPs, encouraging users who don't currently own their loan repayment amount to go and acquire it. The users who haven't sold their CDL can simply repay down to their desired LTV.\
\
In a negative rate environment, users are incentivized to sell their CDT, as the protocol is technically devaluing it over time. In a static redemption price system, Ex 2 would be a normal arb where a user can mint and sell CDT on the market for profit, rebuying to repay the loan as the price falls. The floating rate systems ensures that overtime this arb opportunity increases, independent of continual market price increases.\
\
This system ultimately trades small-scale volatility for long-term stability, without the need for centralized collateral (i.e. existential risk).



_Additional resources for understanding the reflex-index system:_

[https://medium.com/reflexer-labs/stability-without-pegs-8c6a1cbc7fbd](https://medium.com/reflexer-labs/stability-without-pegs-8c6a1cbc7fbd)\
[https://twitter.com/ameensol/status/1420048205127946246?s=20\&t=VVBsx8gveHSZr6hWhzIrNA](https://twitter.com/ameensol/status/1420048205127946246?s=20\&t=VVBsx8gveHSZr6hWhzIrNA)&#x20;

### PID Controller

The PID is what controls the redemption rate changes in the system. Due to its use of real-time data, it'll need to be called by an external participant. Initially this will be incentivized by **MBRN** in multiples of gas cost. In the future this will be fulfilled by cron job sequencers that are getting built around the ecosystem.

\
**Base rate:** Gas cost \
**Market Rate:** Base \* X, where X is a 1% price difference\
\
The 1% delta acts as the minimum for redemption rate updates. There will also be a 2M liquidity minimum for rate updates to curb manipulation.