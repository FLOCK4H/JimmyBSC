use alloy::primitives::{Address};

alloy::sol! {
    #[sol(rpc)]
    interface IERC20Meta {
        function decimals() view returns (uint8);
        function symbol() view returns (string);
        function balanceOf(address owner) view returns (uint256);
    }

    #[sol(rpc)]
    interface IPancakeV2Factory {
        function getPair(address tokenA, address tokenB) view returns (address);
    }

    #[sol(rpc)]
    interface IPancakeV3Factory {
        function getPool(address tokenA, address tokenB, uint24 fee) view returns (address);
    }

    #[sol(rpc)]
    interface ITokenManagerHelper3 {
        function getTokenInfo(address token) view returns (
            uint256 version,
            address tokenManager,
            address quote,
            uint256 lastPrice,
            uint256 tradingFeeRate,
            uint256 minTradingFee,
            uint256 launchTime,
            uint256 offers,
            uint256 maxOffers,
            uint256 funds,
            uint256 maxFunds,
            bool liquidityAdded
        );

        function getPancakePair(address token) view returns (address);
        function tryBuy(address token, uint256 amount, uint256 funds) view returns (
            address tokenManager, address quote,
            uint256 estimatedAmount, uint256 estimatedCost, uint256 estimatedFee,
            uint256 amountMsgValue, uint256 amountApproval, uint256 amountFunds
        );
        function trySell(address token, uint256 amount) view returns (address tokenManager, address quote, uint256 funds, uint256 fee);

        // Add these helpers for fallback computation:
        function WETH() view returns (address);                 // WBNB on BSC
        function PANCAKE_FACTORY() view returns (address);      // V2 factory
        function PANCAKE_V3_FACTORY() view returns (address);   // V3 factory (future-proof)

        // Optional (addresses they store):
        function TOKEN_MANAGER() view returns (address);
        function TOKEN_MANAGER_2() view returns (address);

        // Bridging helpers (only for ERC20/ERC20 pairs per docs):
        function buyWithEth(uint256 origin, address token, address to, uint256 funds, uint256 minAmount) payable;
        function sellForEth(uint256 origin, address token, uint256 amount, uint256 minFunds, uint256 feeRate, address feeRecipient);
    }

    // V2 manager; include byte-args buy for X Mode + _tokenInfos getter
    struct BuyTokenParams {
        uint256 origin;
        address token;
        address to;
        uint256 amount;     // set 0 when using funds-based
        uint256 maxFunds;   // used when 'amount' > 0
        uint256 funds;      // set when using funds-based
        uint256 minAmount;
    }

    #[sol(rpc)]
    interface ITokenManager2 {
        function lastPrice(address tokenAddress) view returns (uint256);

        // X Mode encoded buy:
        function buyToken(bytes args, uint256 time, bytes signature) payable;

        // Normal buys:
        function buyToken(address token, uint256 amount, uint256 maxFunds) payable;
        function buyToken(address token, address to, uint256 amount, uint256 maxFunds) payable;
        function buyTokenAMAP(address token, uint256 funds, uint256 minAmount) payable;
        function buyTokenAMAP(address token, address to, uint256 funds, uint256 minAmount) payable;

        // Sells (simple path)
        function sellToken(address token, uint256 amount);

        // Public mapping accessor (full token meta)
        function _tokenInfos(address token) view returns (
            address base,
            address quote,
            uint256 template,
            uint256 totalSupply,
            uint256 maxOffers,
            uint256 maxRaising,
            uint256 launchTime,
            uint256 offers,
            uint256 funds,
            uint256 lastPrice,
            uint256 K,
            uint256 T,
            uint256 status
        );
    }
}

// handy for decoding 32-byte words into addresses if you need it elsewhere
pub fn decode_address_from_word(word: &[u8]) -> Address {
    debug_assert!(word.len() == 32);
    Address::from_slice(&word[12..])
}
