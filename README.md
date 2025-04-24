# stfix

The STFIX program is a Solana-based token staking protocol implemented using the Anchor framework. It allows users to stake SOL, mint STFIX tokens, and earn interest based on fixed lock terms. The program also supports early redemption with penalties, lock extension with auto-compounding, and administrative controls for managing configurations and whitelists.

## Features

### User Functions
1. **Stake SOL**  
   Stake SOL to mint STFIX tokens at a 1:1 ratio and record the staking position.
   - **Function**: stake
   - **Parameters**: 
     - amount: Amount of SOL to stake.
     - term: Lock term (ThirtyDays or NinetyDays).
     - nonce: Unique identifier for the stake position.
     - memo: Optional memo for the stake.

2. **Redeem After Lock**  
   Redeem SOL after the lock period, burn STFIX tokens, and receive the principal plus interest.
   - **Function**: redeem

3. **Early Redemption**  
   Redeem SOL before the lock period ends with a penalty applied.
   - **Function**: early_redeem

4. **Extend Lock**  
   Extend the lock period and auto-compound the accrued interest into the principal.
   - **Function**: extend_lock
   - **Parameters**:
     - additional_term: Additional lock term (ThirtyDays or NinetyDays).

### Admin Functions
1. **Initialize Configuration**  
   Set up the program's configuration, vaults, mint, and parameters.
   - **Function**: initialize
   - **Parameters**:
     - yield_rate_30: Yield rate for 30-day lock term.
     - yield_rate_90: Yield rate for 90-day lock term.
     - cooldown_seconds: Minimum time between stakes for a user.
     - penalty_rate_bps: Penalty rate for early redemption (in basis points).
     - whitelist_only: Whether staking is restricted to whitelisted users.

2. **Top Up Yield Vault**  
   Add funds to the yield vault for paying interest.
   - **Function**: top_up_yield
   - **Parameters**:
     - amount: Amount of SOL to add to the yield vault.

3. **Manage Whitelist**  
   Add or remove wallets from the whitelist.
   - **Functions**: add_to_whitelist, remove_from_whitelist
   - **Parameters**:
     - user: Public key of the wallet to add or remove.

## Accounts

### Config
Stores the program's configuration and parameters.
- **Fields**:
  - admin: Admin public key.
  - stfix_mint: STFIX token mint address.
  - principal_vault: Vault for storing staked SOL.
  - yield_vault: Vault for storing yield funds.
  - yield_rate_30: Yield rate for 30-day lock term.
  - yield_rate_90: Yield rate for 90-day lock term.
  - cooldown_seconds: Minimum time between stakes.
  - penalty_rate_bps: Penalty rate for early redemption.
  - whitelist_only: Restricts staking to whitelisted users.
  - whitelist: List of whitelisted public keys.
  - total_interest_paid: Total interest paid by the program.

### StakePosition
Represents a user's staking position.
- **Fields**:
  - user: User's public key.
  - amount: Amount of staked SOL.
  - deposit_time: Timestamp of the stake.
  - term: Lock term in days.
  - in_use: Reentrancy guard.
  - memo: Optional memo.
  - nonce: Unique identifier.

### UserState
Tracks the user's last stake time for rate limiting.
- **Fields**:
  - owner: User's public key.
  - last_stake_time: Timestamp of the last stake.

## Events

- **StakeEvent**: Emitted when a user stakes SOL.
- **RedeemEvent**: Emitted when a user redeems after the lock period.
- **EarlyRedeemEvent**: Emitted when a user redeems early with a penalty.
- **TopUpYieldEvent**: Emitted when the yield vault is topped up.

## Error Codes

- LockPeriodNotCompleted: Lock period not yet completed.
- Unauthorized: Unauthorized access.
- InsufficientYieldVaultFunds: Insufficient funds in the yield vault.
- Reentrancy: Reentrancy detected.
- RateLimited: User is rate-limited from staking again.
- NotWhitelisted: User is not on the whitelist.

## Constants

- SECONDS_PER_DAY: Number of seconds in a day (86,400).
- MAX_WHITELIST: Maximum number of whitelisted users (10).
- MAX_MEMO_LEN: Maximum length of a memo (128 characters).

## Usage

1. **Initialize the Program**: Call the initialize function with the desired parameters.
2. **Stake SOL**: Use the stake function to lock SOL and mint STFIX tokens.
3. **Redeem or Extend Lock**: After the lock period, use redeem to withdraw funds or extend_lock to compound interest.
4. **Early Redemption**: Use early_redeem to withdraw funds before the lock period ends with a penalty.
5. **Admin Actions**: Use admin functions to manage the yield vault and whitelist.

## License

This program is licensed under the terms of the MIT License.
