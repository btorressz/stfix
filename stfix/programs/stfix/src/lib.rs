use anchor_lang::prelude::*;
use anchor_spl::{
    token::{self, Mint, Token, TokenAccount, MintTo, Burn},
    associated_token::AssociatedToken,
};

declare_id!("ProgramID");

#[program]
pub mod stfix {
    use super::*;

    /// ADMIN: initialize config, vaults, mint, and parameters
    pub fn initialize(
        ctx: Context<Initialize>,
        yield_rate_30: u64,
        yield_rate_90: u64,
        cooldown_seconds: i64,
        penalty_rate_bps: u64,
        whitelist_only: bool,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = *ctx.accounts.admin.key;
        config.stfix_mint = ctx.accounts.stfix_mint.key();
        config.principal_vault = ctx.accounts.principal_vault.key();
        config.yield_vault = ctx.accounts.yield_vault.key();
        config.yield_rate_30 = yield_rate_30;
        config.yield_rate_90 = yield_rate_90;
        config.cooldown_seconds = cooldown_seconds;
        config.penalty_rate_bps = penalty_rate_bps;
        config.whitelist_only = whitelist_only;
        config.whitelist.clear();
        config.total_interest_paid = 0;
        Ok(())
    }

    /// USER: stake SOL, mint STFIX, record position
    pub fn stake(
        ctx: Context<Stake>,
        amount: u64,
        term: LockTerm,
        nonce: u64,
        memo: Option<String>,
    ) -> Result<()> {
        let config = &ctx.accounts.config;
        let now = Clock::get()?.unix_timestamp;

        // whitelist check
        if config.whitelist_only {
            require!(
                config.whitelist.contains(ctx.accounts.user.key),
                ErrorCode::NotWhitelisted
            );
        }

        // rate limit
        let state = &mut ctx.accounts.user_state;
        if now - state.last_stake_time < config.cooldown_seconds {
            return err!(ErrorCode::RateLimited);
        }
        state.last_stake_time = now;

        // reentrancy guard
        let pos = &mut ctx.accounts.stake_position;
        require!(!pos.in_use, ErrorCode::Reentrancy);
        pos.in_use = true;

        // transfer SOL → principal vault
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.user.to_account_info(),
                    to: ctx.accounts.principal_vault.clone(),
                },
            ),
            amount,
        )?;

        // mint STFIX 1:1
        let bump = ctx.bumps.config;
        let seeds = &[b"config".as_ref(), &[bump]];
        let signer = &[&seeds[..]];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.stfix_mint.to_account_info(),
                    to: ctx.accounts.user_stfix_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer,
            ),
            amount,
        )?;

        // record stake
        pos.user = *ctx.accounts.user.key;
        pos.amount = amount;
        pos.deposit_time = now;
        pos.term = term.days();
        pos.nonce = nonce;
        pos.memo = memo.clone();
        pos.in_use = false;

        emit!(StakeEvent {
            user: *ctx.accounts.user.key,
            amount,
            term: term.days() as u64,
            timestamp: now,
            memo,
        });

        Ok(())
    }

    /// USER: redeem after lock, burn STFIX, return principal + interest
    pub fn redeem(ctx: Context<Redeem>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let pos = &mut ctx.accounts.stake_position;
        let now = Clock::get()?.unix_timestamp;
        let unlock = pos.deposit_time + pos.term * SECONDS_PER_DAY;

        require!(now >= unlock, ErrorCode::LockPeriodNotCompleted);
        require!(!pos.in_use, ErrorCode::Reentrancy);
        pos.in_use = true;

        // compute fixed interest
        let rate = if pos.term == 30 {
            config.yield_rate_30
        } else {
            config.yield_rate_90
        };
        let interest = pos
            .amount
            .checked_mul(rate).unwrap()
            .checked_div(10_000).unwrap();

        // ensure yield vault has funds
        require!(
            **ctx.accounts.yield_vault.lamports.borrow() >= interest,
            ErrorCode::InsufficientYieldVaultFunds
        );

        // burn STFIX
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.stfix_mint.to_account_info(),
                    from: ctx.accounts.user_stfix_ata.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            pos.amount,
        )?;

        // return principal
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.principal_vault.clone(),
                    to: ctx.accounts.user.to_account_info(),
                },
            ),
            pos.amount,
        )?;

        // pay interest
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.yield_vault.clone(),
                    to: ctx.accounts.user.to_account_info(),
                },
            ),
            interest,
        )?;

        // track total interest paid
        config.total_interest_paid = config
            .total_interest_paid
            .checked_add(interest as u128)
            .unwrap();

        emit!(RedeemEvent {
            user: *ctx.accounts.user.key,
            principal: pos.amount,
            interest,
            timestamp: now,
        });

        pos.amount = 0;
        pos.in_use = false;
        Ok(())
    }

    /// USER: early exit with penalty
    pub fn early_redeem(ctx: Context<EarlyRedeem>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let pos = &mut ctx.accounts.stake_position;

        require!(!pos.in_use, ErrorCode::Reentrancy);
        pos.in_use = true;

        let penalty = pos
            .amount
            .checked_mul(config.penalty_rate_bps).unwrap()
            .checked_div(10_000).unwrap();
        let payout = pos.amount.checked_sub(penalty).unwrap();

        // burn STFIX
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.stfix_mint.to_account_info(),
                    from: ctx.accounts.user_stfix_ata.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            pos.amount,
        )?;

        // pay out principal minus penalty
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.principal_vault.clone(),
                    to: ctx.accounts.user.to_account_info(),
                },
            ),
            payout,
        )?;
        // send penalty to yield vault
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.principal_vault.clone(),
                    to: ctx.accounts.yield_vault.clone(),
                },
            ),
            penalty,
        )?;

        emit!(EarlyRedeemEvent {
            user: *ctx.accounts.user.key,
            amount: pos.amount,
            penalty,
            timestamp: Clock::get()?.unix_timestamp,
        });

        pos.amount = 0;
        pos.in_use = false;
        Ok(())
    }

    /// USER: extend lock and auto-compound interest
    pub fn extend_lock(ctx: Context<ExtendLock>, additional_term: LockTerm) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let pos = &mut ctx.accounts.stake_position;
        let now = Clock::get()?.unix_timestamp;
        require!(!pos.in_use, ErrorCode::Reentrancy);
        pos.in_use = true;

        // calculate accrued interest
        let elapsed = now - pos.deposit_time;
        let days = (elapsed / SECONDS_PER_DAY) as u64;
        let rate = if pos.term == 30 {
            config.yield_rate_30
        } else {
            config.yield_rate_90
        };
        let interest = pos
            .amount
            .checked_mul(rate).unwrap()
            .checked_mul(days).unwrap()
            .checked_div(10_000 * pos.term as u64).unwrap();

        // roll interest into principal vault
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.yield_vault.clone(),
                    to: ctx.accounts.principal_vault.clone(),
                },
            ),
            interest,
        )?;
        config.total_interest_paid = config
            .total_interest_paid
            .checked_add(interest as u128)
            .unwrap();

        // update position
        pos.amount = pos.amount.checked_add(interest).unwrap();
        pos.deposit_time = now;
        pos.term = additional_term.days();
        pos.in_use = false;

        Ok(())
    }

    /// ADMIN: top up yield vault
    pub fn top_up_yield(ctx: Context<TopUpYield>, amount: u64) -> Result<()> {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: ctx.accounts.yield_vault.clone(),
                },
            ),
            amount,
        )?;
        emit!(TopUpYieldEvent {
            admin: *ctx.accounts.admin.key,
            amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    /// ADMIN: add a wallet to whitelist
    pub fn add_to_whitelist(ctx: Context<UpdateWhitelist>, user: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        if !cfg.whitelist.contains(&user) {
            cfg.whitelist.push(user);
        }
        Ok(())
    }

    /// ADMIN: remove a wallet from whitelist
    pub fn remove_from_whitelist(ctx: Context<UpdateWhitelist>, user: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.whitelist.retain(|&u| u != user);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(
    yield_rate_30: u64,
    yield_rate_90: u64,
    cooldown_seconds: i64,
    penalty_rate_bps: u64,
    whitelist_only: bool
)]
pub struct Initialize<'info> {
    #[account(
        init,
        seeds = [b"config"],
        bump,
        payer = admin,
        space = 8 + Config::LEN
    )]
    pub config: Account<'info, Config>,

    #[account(
        init,
        seeds = [b"principal-vault"],
        bump,
        payer = admin,
        space = 8
    )]
    pub principal_vault: AccountInfo<'info>,

    #[account(
        init,
        seeds = [b"yield-vault"],
        bump,
        payer = admin,
        space = 8
    )]
    pub yield_vault: AccountInfo<'info>,

    #[account(
        init,
        seeds = [b"stfix-mint"],
        bump,
        payer = admin,
        mint::decimals = 9,
        mint::authority = config
    )]
    pub stfix_mint: Account<'info, Mint>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(
    amount: u64,
    term: LockTerm,
    nonce: u64,
    memo: Option<String>
)]
pub struct Stake<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        seeds = [b"user-state", user.key().as_ref()],
        bump,
        payer = user,
        space = 8 + UserState::LEN
    )]
    pub user_state: Account<'info, UserState>,

    #[account(
        init,
        seeds = [b"user-stake", user.key().as_ref(), &nonce.to_le_bytes()],
        bump,
        payer = user,
        space = 8 + StakePosition::LEN
    )]
    pub stake_position: Account<'info, StakePosition>,

    #[account(mut, seeds = [b"principal-vault"], bump)]
    pub principal_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"stfix-mint"], bump)]
    pub stfix_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = user,
        associated_token::mint = stfix_mint,
        associated_token::authority = user
    )]
    pub user_stfix_ata: Account<'info, TokenAccount>,

    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref(), &stake_position.nonce.to_le_bytes()],
        bump,
        has_one = user
    )]
    pub stake_position: Account<'info, StakePosition>,

    #[account(mut, seeds = [b"principal-vault"], bump)]
    pub principal_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"yield-vault"], bump)]
    pub yield_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"stfix-mint"], bump)]  
    pub stfix_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = stfix_mint,
        associated_token::authority = user
    )]
    pub user_stfix_ata: Account<'info, TokenAccount>,

    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct EarlyRedeem<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref(), &stake_position.nonce.to_le_bytes()],
        bump,
        has_one = user
    )]
    pub stake_position: Account<'info, StakePosition>,

    #[account(mut, seeds = [b"principal-vault"], bump)]
    pub principal_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"yield-vault"], bump)]
    pub yield_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"stfix-mint"], bump)]
    pub stfix_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = stfix_mint,
        associated_token::authority = user
    )]
    pub user_stfix_ata: Account<'info, TokenAccount>,

    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExtendLock<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"user-stake", user.key().as_ref(), &stake_position.nonce.to_le_bytes()],
        bump,
        has_one = user
    )]
    pub stake_position: Account<'info, StakePosition>,

    #[account(mut, seeds = [b"principal-vault"], bump)]
    pub principal_vault: AccountInfo<'info>,

    #[account(mut, seeds = [b"yield-vault"], bump)]
    pub yield_vault: AccountInfo<'info>,

    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TopUpYield<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        has_one = admin
    )]
    pub config: Account<'info, Config>,

    #[account(mut, seeds = [b"yield-vault"], bump)]
    pub yield_vault: AccountInfo<'info>,

    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateWhitelist<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        has_one = admin
    )]
    pub config: Account<'info, Config>,

    pub admin: Signer<'info>,
}

#[account]
pub struct Config {
    pub admin: Pubkey,
    pub stfix_mint: Pubkey,
    pub principal_vault: Pubkey,
    pub yield_vault: Pubkey,
    pub yield_rate_30: u64,
    pub yield_rate_90: u64,
    pub cooldown_seconds: i64,
    pub penalty_rate_bps: u64,
    pub whitelist_only: bool,
    pub whitelist: Vec<Pubkey>,
    pub total_interest_paid: u128,
}
impl Config {
    pub const LEN: usize =
        32*4 +        // admin + 3 pubkeys
        8*2  +        // two rates
        8    +        // cooldown_seconds
        8    +        // penalty_rate_bps
        1    +        // whitelist_only
        4 + (32 * MAX_WHITELIST) + // vec<Pubkey>
        16;           // u128
}

#[account]
pub struct StakePosition {
    pub user: Pubkey,
    pub amount: u64,
    pub deposit_time: i64,
    pub term: i64,
    pub in_use: bool,
    pub memo: Option<String>,
    pub nonce: u64,
}
impl StakePosition {
    pub const LEN: usize =
        32 +                 // user
        8  +                 // amount
        8  +                 // deposit_time
        8  +                 // term
        1  +                 // in_use
        4 + MAX_MEMO_LEN +   // memo Option<String>
        8;                   // nonce
}

#[account]
pub struct UserState {
    pub owner: Pubkey,
    pub last_stake_time: i64,
}
impl UserState {
    pub const LEN: usize = 32 + 8;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum LockTerm {
    ThirtyDays,
    NinetyDays,
}
impl LockTerm {
    pub fn days(&self) -> i64 {
        match self {
            LockTerm::ThirtyDays => 30,
            LockTerm::NinetyDays => 90,
        }
    }
}

const SECONDS_PER_DAY: i64 = 86_400;
const MAX_WHITELIST: usize = 10;
const MAX_MEMO_LEN: usize = 128;

#[event]
pub struct StakeEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub term: u64,
    pub timestamp: i64,
    pub memo: Option<String>,
}

#[event]
pub struct RedeemEvent {
    pub user: Pubkey,
    pub principal: u64,
    pub interest: u64,
    pub timestamp: i64,
}

#[event]
pub struct EarlyRedeemEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub penalty: u64,
    pub timestamp: i64,
}

#[event]
pub struct TopUpYieldEvent {
    pub admin: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Lock period not yet completed")]
    LockPeriodNotCompleted,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Insufficient yield vault funds")]
    InsufficientYieldVaultFunds,
    #[msg("Reentrancy detected")]
    Reentrancy,
    #[msg("Rate limited: please wait before staking again")]
    RateLimited,
    #[msg("Not on whitelist")]
    NotWhitelisted,
}
