## API Endpoint Overview

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/private/user/nonce/generate` | POST | Generate nonce for login |
| `/v1/private/user/login/dex` | POST | User login to get access token |
| `/v1/private/token/upload` | POST | Upload token image |
| `/v1/private/token/create` | POST | Create token and get signature parameters |

## Complete API Flow

### 1. Get Nonce
**Endpoint**: `https://four.meme/meme-api/v1/private/user/nonce/generate`  
**Method**: POST  
**Parameters**:
```json
{
  "accountAddress": "user wallet address",
  "verifyType": "LOGIN",
  "networkCode": "BSC"
}
```
**Response**:
```json
{
  "code": "0",
  "data": "generated nonce value"
}
```

### 2. User Login
**Endpoint**: `https://four.meme/meme-api/v1/private/user/login/dex`  
**Method**: POST  
**Parameters**:
```json
{
  "region": "WEB",
  "langType": "EN",
  "loginIp": "",
  "inviteCode": "",
  "verifyInfo": {
    "address": "user wallet address",
    "networkCode": "BSC",
    "signature": "signature of 'You are sign in Meme {nonce}' signed with private key",
    "verifyType": "LOGIN"
  },
  "walletName": "MetaMask"
}
```
**Response**:
```json
{
  "code": "0",
  "data": "access_token"
}
```

### 3. Upload Token Image
**Endpoint**: `https://four.meme/meme-api/v1/private/token/upload`  
**Method**: POST  
**Headers**:
```
Content-Type: multipart/form-data
meme-web-access: {access_token}
```
**Parameters**:
- `file`: Image file data (supports jpeg, png, gif, bmp, webp formats)

**Response**:
```json
{
  "code": "0",
  "data": "uploaded image URL"
}
```

### 4. Create Token
**Endpoint**: `https://four.meme/meme-api/v1/private/token/create`  
**Method**: POST  
**Headers**:
```
meme-web-access: {access_token}
```

## Parameter Explanation (Distinguishing Fixed and Customizable Parameters)

### Customizable Parameters

| Parameter | Description | Example Value | Limitations |
|-----------|-------------|---------------|-------------|
| name | Token name | "RELEASE" | Customizable |
| shortName | Token symbol/ticker | "RELS" | Customizable |
| desc | Token description | "RELEASE DESC" | Customizable |
| imgUrl | Token image URL | "https://static.four.meme/market/..." | Must be uploaded to the platform |
| launchTime | Launch timestamp | 1740708849097 | Customizable |
| label | Token category | "AI" | Must be one of the platform-supported categories: Meme/AI/Defi/Games/Infra/De-Sci/Social/Depin/Charity/Others |
| lpTradingFee | Trading fee rate | 0.0025 | Fixed as 0.0025 |
| webUrl | Project website | "https://example.com" | Customizable |
| twitterUrl | Project Twitter | "https://x.com/example" | Customizable |
| telegramUrl | Project Telegram | "https://telegram.com/example" | Customizable |
| preSale | Presale amount | "0.1" | Pre-purchased BNB amount by the creator; "0" if not purchased |
| onlyMPC | X Mode Token | false | Whether to create a token in X Mode | Customizable

### Fixed Parameters (Cannot be Adjusted or Customized by Thirdparties)

| Parameter | Fixed Value | Description |
|-----------|-------------|-------------|
| totalSupply | 1000000000 | Total token supply is fixed at 1 billion |
| raisedAmount | 24 | Raised amount is fixed at 24 BNB |
| saleRate | 0.8 | Sale ratio is fixed at 80% |
| reserveRate | 0 | Reserved ratio is fixed at 0 |
| funGroup | false | Fixed parameter |
| clickFun | false | Fixed parameter |
| symbol | "BNB" | Fixed use of BNB as base currency |

### Fixed Parameters for raisedToken. Different raised token configs can be queried by https://four.meme/meme-api/v1/public/config (Cannot modify internal params)

```json
"raisedToken": {
  "symbol": "BNB",
  "nativeSymbol": "BNB",
  "symbolAddress": "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c",
  "deployCost": "0",
  "buyFee": "0.01",
  "sellFee": "0.01",
  "minTradeFee": "0",
  "b0Amount": "8",
  "totalBAmount": "24",
  "totalAmount": "1000000000",
  "logoUrl": "https://static.four.meme/market/68b871b6-96f7-408c-b8d0-388d804b34275092658264263839640.png",
  "tradeLevel": ["0.1", "0.5", "1"],
  "status": "PUBLISH",
  "buyTokenLink": "https://pancakeswap.finance/swap",
  "reservedNumber": 10,
  "saleRate": "0.8",
  "networkCode": "BSC",
  "platform": "MEME"
}
```

## Blockchain Interaction

After obtaining the API signature, you need to call the chain contract `TokenManager2`'s `createToken` method:

```java
TokenManager2 contract = TokenManager2.load(tokenManagerAddress, web3j, credentials, gasProvider);
TransactionReceipt receipt = contract.createToken(createArg, sign).send();
```

Where:
- `createArg`: The `createArg` returned by the API converted into byte array
- `sign`: The `signature` returned by the API converted into byte array

## Notes

1. Token creation requires meeting the minimum BNB balance requirement. The latest creation fee is 0.01 BNB.
2. Images must be uploaded to the four.meme platform.
3. There are certain creation fees for creating tokens (which are currently free).
4. Most technical parameters are fixed and cannot be adjusted (including total supply, raised amount, sale ratio, etc.).
5. Only display information like token name, symbol, description, and image can be customized.
6. The token label must be one of the platform-supported categories.
7. The trading fee rate can only be 0.0025. 

With the above API flow, users can complete the entire process of logging in and creating tokens on the four.meme platform, but they should note that most technical parameters are preset and fixed by the platform.