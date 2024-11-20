use near_contract_standards::fungible_token::Balance;
use near_contract_standards::fungible_token::core::ext_ft_core;
use near_sdk::{AccountId, env, ext_contract, Gas, log, near, NearToken, PanicOnDefault, Promise, PromiseOrValue, PromiseResult, require, serde_json};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};

pub mod ft_receiver;

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct TicleCore {
    vapis: LookupMap<String, VAPIInfo>,
    reviewers: LookupMap<AccountId, ReviewerInfo>,
    token_id: AccountId,
    owner_id: AccountId,
    signer_public_key: Vec<u8>,
    treasury: Balance,
    max_depositable_vapi_count: u8,
}

#[near(serializers = [borsh])]
pub struct VAPIInfo { 
    coder_info: CoderInfo,
    total_deposit_amount: Balance,
    acc_reward_per_share: Balance,
}

#[near(serializers = [borsh])]
pub struct CoderInfo {
    account_id: AccountId,
    unclaimed_reward_amount: Balance,
}

#[near(serializers = [borsh])]
pub struct ReviewerInfo {
    deposit_vapis: UnorderedMap<String, DepositInfo>,
    pending_amount: Balance,
    royalty_amount: Balance,
    delegators: UnorderedMap<AccountId, DelegatorInfo>,
    total_delegator_deposit_amount: Balance,
    acc_reward_per_share: Balance,
}

#[near(serializers = [borsh])]
pub struct DelegatorInfo {
    deposit_info: DepositInfo,
    refunding_amount: Balance,
    refunding_start_timestamp: u64,
}

#[near(serializers = [borsh])]
pub struct DepositInfo {
    deposit_amount: Balance,
    reward_debt: Balance,
}

#[near(serializers = [borsh, json])]
pub struct GetDepositInfoResponse {
    pub deposit_amount: Balance,
    pub reward: Balance,
}

#[ext_contract(ext_ft_burn)]
pub trait FungibleTokenBurn {
    fn burn(&mut self, amount: U128);
}

#[near]
impl TicleCore {
    #[init]
    pub fn new(token_id: AccountId, owner_id: AccountId) -> Self {
        let mut signer_public_key = env::signer_account_pk().into_bytes();

        // The first byte of the public key prefix indicates the algorithm used to generate the key
        // It will not be used, so remove it
        signer_public_key.remove(0);

        Self {
            vapis: LookupMap::new(b"v".to_vec()),
            reviewers: LookupMap::new(b"r".to_vec()),
            token_id,
            owner_id,
            signer_public_key,
            treasury: 0,
            max_depositable_vapi_count: 10,
        }
    }
}

#[near]
impl TicleCore {
    pub fn get_signer_public_key(&self) -> Vec<u8> {
        return self.signer_public_key.clone();
    }

    pub fn get_reviewer_deposit_info(&self, reviewer_id: &AccountId, vapi_id: String) -> GetDepositInfoResponse {
        let mut result = GetDepositInfoResponse {
            deposit_amount: 0,
            reward: 0,
        };

        let reviewer_info = self.reviewers.get(&reviewer_id);
        if reviewer_info.is_none() {
            return result;
        }

        let reviewer_info = reviewer_info.unwrap();
        let deposit_info = reviewer_info.deposit_vapis.get(&vapi_id);
        if deposit_info.is_none() {
            return result;
        }

        let deposit_info = deposit_info.unwrap();
        result.deposit_amount = deposit_info.deposit_amount;
        result.reward = self.pending_reward(deposit_info.deposit_amount, deposit_info.reward_debt, reviewer_info.acc_reward_per_share);
        return result;
    }

    pub fn get_delegator_deposit_info(&self, delegator_id: &AccountId, reviewer_id: &AccountId) -> GetDepositInfoResponse {
        let mut result = GetDepositInfoResponse {
            deposit_amount: 0,
            reward: 0,
        };

        let reviewer_info = self.reviewers.get(&reviewer_id);
        if reviewer_info.is_none() {
            return result;
        }

        let reviewer_info = reviewer_info.unwrap();
        let delegator_info = reviewer_info.delegators.get(&delegator_id);
        if delegator_info.is_none() {
            return result;
        }

        let delegator_info = delegator_info.unwrap();
        result.deposit_amount = delegator_info.deposit_info.deposit_amount;
        result.reward = self.pending_reward(delegator_info.deposit_info.deposit_amount, delegator_info.deposit_info.reward_debt, reviewer_info.acc_reward_per_share);
        return result;
    }

    pub fn get_reviewer_pending_amount(&self, reviewer_id: &AccountId) -> Balance {
        let reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        return reviewer_info.pending_amount;
    }

    pub fn get_reviewer_royalty_amount(&self, reviewer_id: &AccountId) -> Balance {
        let reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        return reviewer_info.royalty_amount;
    }
}

#[near]
impl TicleCore {
    fn pending_reward(&self, deposit_amount: Balance, reward_debt: Balance, acc_reward_per_share: Balance) -> Balance {
        let new_reward_debt = deposit_amount * acc_reward_per_share / 1_000_000_000_000;
        return new_reward_debt - reward_debt;
    }

    pub fn create_vapi(&mut self, vapi_id: String) {
        let coder_id = env::predecessor_account_id();
        let vapi = VAPIInfo {
            coder_info: CoderInfo {
                account_id: coder_id,
                unclaimed_reward_amount: 0,
            },
            total_deposit_amount: 0,
            acc_reward_per_share: 0,
        };
        self.vapis.insert(&vapi_id, &vapi);
    }

    pub fn create_reviewer(&mut self, reviewer_id: &AccountId) {
        let reviewer = ReviewerInfo {
            deposit_vapis: UnorderedMap::new(b"dv".to_vec()),
            total_delegator_deposit_amount: 0,
            pending_amount: 0,
            royalty_amount: 0,
            delegators: UnorderedMap::new(b"d".to_vec()),
            acc_reward_per_share: 0,
        };
        self.reviewers.insert(&reviewer_id, &reviewer);
    }

    #[payable]
    pub fn deposit_to_vapi(&mut self, vapi_id: String, amount: U128) -> Promise {
        let amount = amount.into();
        log!("[deposit_to_vapi] vapi_id: {}, amount: {}", vapi_id, amount);

        let reviewer_id = env::predecessor_account_id();

        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        let mut reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        require!(reviewer_info.pending_amount >= amount, "pending amount must be greater than amount");

        let mut deposit_info = reviewer_info.deposit_vapis.get(&vapi_id).unwrap_or(DepositInfo {
            deposit_amount: 0,
            reward_debt: 0,
        });
        
        let reward = self.pending_reward(deposit_info.deposit_amount, deposit_info.reward_debt, vapi.acc_reward_per_share);
        
        deposit_info.deposit_amount += amount + reward;
        deposit_info.reward_debt = deposit_info.deposit_amount * vapi.acc_reward_per_share / 1_000_000_000_000;

        reviewer_info.pending_amount -= amount;
        reviewer_info.deposit_vapis.insert(&vapi_id, &deposit_info);
        
        require!(reviewer_info.deposit_vapis.len() < self.max_depositable_vapi_count as u64, "Max depositable VAPI count reached");
        
        self.reviewers.insert(&reviewer_id, &reviewer_info);

        vapi.total_deposit_amount += amount;
        self.vapis.insert(&vapi_id, &vapi);

        // TODO: should emit success event
        return Promise::new(reviewer_id.clone());
    }

    pub fn compound(&mut self, reviewer_id: &AccountId) {
        let mut reviewer_info = self.reviewers.get(reviewer_id).expect("Reviewer not found");

        let mut total_royalty_amount: Balance = 0;
        let mut total_delegator_reward_amount: Balance = 0;
        
        for (vapi_id, mut deposit_info) in reviewer_info.deposit_vapis.to_vec() {
            let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
            let reward = self.pending_reward(deposit_info.deposit_amount, deposit_info.reward_debt, vapi.acc_reward_per_share);
            if reward == 0 {
                continue;
            }

            let royalty_amount = reward * 1 / 100;
            total_royalty_amount += royalty_amount;

            let delegator_reward_amount = reward - royalty_amount;
            total_delegator_reward_amount += delegator_reward_amount;

            deposit_info.deposit_amount += delegator_reward_amount;
            deposit_info.reward_debt = deposit_info.deposit_amount * vapi.acc_reward_per_share / 1_000_000_000_000;
            reviewer_info.deposit_vapis.insert(&vapi_id, &deposit_info);
            vapi.total_deposit_amount += delegator_reward_amount;
            self.vapis.insert(&vapi_id, &vapi);
        }

        reviewer_info.royalty_amount += total_royalty_amount;
        reviewer_info.acc_reward_per_share += total_delegator_reward_amount * 1_000_000_000_000 / reviewer_info.total_delegator_deposit_amount;
        self.reviewers.insert(&reviewer_id, &reviewer_info);
    }

    pub fn transfer_ownership(&mut self, vapi_id: String, new_coder_id: AccountId) {
        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");

        let account_id = env::predecessor_account_id();
        require!(vapi.coder_info.account_id == account_id, "Only coder can transfer ownership");

        vapi.coder_info.account_id = new_coder_id;
        self.vapis.insert(&vapi_id, &vapi);
    }

    pub fn delegator_request_refund(&mut self, reviewer_id: &AccountId, amount: U128) -> Promise {
        let amount = amount.into();
        self.compound(reviewer_id);

        let sender_id = env::predecessor_account_id();

        let mut reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        let mut delegator_info = reviewer_info.delegators.get(&sender_id).expect("Delegator not found");
        let reward = self.pending_reward(delegator_info.deposit_info.deposit_amount, delegator_info.deposit_info.reward_debt, reviewer_info.acc_reward_per_share);

        let delegator_balance = delegator_info.deposit_info.deposit_amount + reward;
        require!(delegator_balance >= amount, "Delegator balance is less than the amount to refund");

        if reviewer_info.pending_amount < amount {
            let pending_amount = reviewer_info.pending_amount;
            let total_deposit_amount = reviewer_info.total_delegator_deposit_amount - pending_amount;
            
            let origin_remain_amount = amount - pending_amount;
            let mut remain_amount = origin_remain_amount;
            for (vapi_id, mut deposit_info) in reviewer_info.deposit_vapis.to_vec() {
                let deposit_rate = (deposit_info.deposit_amount as f64) / (total_deposit_amount as f64);
                let decrease_amount = (origin_remain_amount as f64 * deposit_rate).floor() as Balance;

                deposit_info.deposit_amount -= decrease_amount;
                deposit_info.reward_debt = deposit_info.deposit_amount * reviewer_info.acc_reward_per_share / 1_000_000_000_000;
                reviewer_info.deposit_vapis.insert(&vapi_id, &deposit_info);

                let mut vapi = self.vapis.get(&vapi_id).unwrap();
                vapi.total_deposit_amount -= decrease_amount;
                self.vapis.insert(&vapi_id, &vapi);

                remain_amount -= decrease_amount;
            }

            if remain_amount > 0 {
                for (vapi_id, mut deposit_info) in reviewer_info.deposit_vapis.to_vec() {
                    if deposit_info.deposit_amount >= remain_amount {
                        deposit_info.deposit_amount -= remain_amount;
                        deposit_info.reward_debt = deposit_info.deposit_amount * reviewer_info.acc_reward_per_share / 1_000_000_000_000;
                        reviewer_info.deposit_vapis.insert(&vapi_id, &deposit_info);

                        let mut vapi = self.vapis.get(&vapi_id).unwrap();
                        vapi.total_deposit_amount -= remain_amount;
                        self.vapis.insert(&vapi_id, &vapi);
                        break;
                    }
                }
            }

            reviewer_info.total_delegator_deposit_amount -= amount - pending_amount;
            reviewer_info.pending_amount = 0;
        } else {
            reviewer_info.pending_amount -= amount;
        }

        delegator_info.deposit_info.deposit_amount = delegator_balance - amount;
        delegator_info.deposit_info.reward_debt = delegator_info.deposit_info.deposit_amount * reviewer_info.acc_reward_per_share / 1_000_000_000_000;
        
        delegator_info.refunding_amount += amount;
        delegator_info.refunding_start_timestamp = env::block_timestamp_ms();
        reviewer_info.delegators.insert(&sender_id, &delegator_info);
        
        reviewer_info.total_delegator_deposit_amount -= amount;
        self.reviewers.insert(&reviewer_id, &reviewer_info);

        // TODO: should emit success event
        return Promise::new(sender_id.clone());
    }

    pub fn delegator_claim_refund(&mut self, reviewer_id: &AccountId) -> Promise {
        let delegator_id = env::predecessor_account_id();

        let mut reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        let mut delegator_info = reviewer_info.delegators.get(&delegator_id).expect("Delegator not found");

        let refunding_amount = delegator_info.refunding_amount;
        if refunding_amount == 0 {
            return Promise::new(delegator_id);
        }

        require!(env::block_timestamp_ms() - delegator_info.refunding_start_timestamp >= 60 * 1_000, "Refunding period is less than 60 seconds");

        delegator_info.refunding_amount = 0;
        reviewer_info.delegators.insert(&delegator_id, &delegator_info);
        self.reviewers.insert(&reviewer_id, &reviewer_info);

        return ext_ft_core::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .ft_transfer(delegator_id.clone(), U128(refunding_amount), None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(20))
                    .callback_delegator_claim_refund(
                        &delegator_id, 
                        &reviewer_id, 
                        refunding_amount, 
                    )
            );
    }

    #[private]
    pub fn callback_delegator_claim_refund(&mut self, delegator_id: &AccountId, reviewer_id: &AccountId, refunding_amount: Balance) -> Promise {
        let mut reviewer_info = self.reviewers.get(&reviewer_id).unwrap();
        let mut delegator_info = reviewer_info.delegators.get(&delegator_id).unwrap();

        const REFUND_TRANSFER_PROMISE_INDEX: u64 = 0;
        match env::promise_result(REFUND_TRANSFER_PROMISE_INDEX) {
            PromiseResult::Failed => {
                delegator_info.refunding_amount += refunding_amount;
                reviewer_info.delegators.insert(&delegator_id, &delegator_info);
                self.reviewers.insert(&reviewer_id, &reviewer_info);
                return Promise::new(delegator_id.clone());
            }
            PromiseResult::Successful(_) => {
                if delegator_info.refunding_amount == 0 {
                    delegator_info.refunding_start_timestamp = 0;

                    if delegator_info.deposit_info.deposit_amount == 0 {
                        reviewer_info.delegators.remove(&delegator_id);
                    } else {
                        reviewer_info.delegators.insert(&delegator_id, &delegator_info);
                    }
                    self.reviewers.insert(&reviewer_id, &reviewer_info);
                }
                return Promise::new(delegator_id.clone());   
            }
        }
    }

    #[payable]
    pub fn withdraw_from_vapi(&mut self, vapi_id: String, amount: U128) -> Promise {
        log!("[withdraw_from_vapi] vapi_id: {}", vapi_id);
        let amount = amount.into();
        require!(amount > 0, "amount must be greater than 0");

        let reviewer_id = env::predecessor_account_id();
        self.compound(&reviewer_id);
    
        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        let mut reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        let mut deposit_info = reviewer_info.deposit_vapis.get(&vapi_id).expect("Deposit info not found");

        require!(deposit_info.deposit_amount >= amount, "deposit amount must be greater than amount");
        deposit_info.deposit_amount -= amount;

        if deposit_info.deposit_amount == 0 {
            reviewer_info.deposit_vapis.remove(&vapi_id);
        } else {
            deposit_info.reward_debt = deposit_info.deposit_amount * reviewer_info.acc_reward_per_share / 1_000_000_000_000;
            reviewer_info.deposit_vapis.insert(&vapi_id, &deposit_info);
        }

        reviewer_info.pending_amount += amount;
        self.reviewers.insert(&reviewer_id, &reviewer_info);

        vapi.total_deposit_amount -= amount;
        self.vapis.insert(&vapi_id, &vapi);

        return Promise::new(reviewer_id.clone());
    }
}

#[near]
impl TicleCore {
    fn internal_deposit_to_reviewer(&mut self, sender_id: &AccountId, reviewer_id: &AccountId, amount: Balance) -> Promise {
        log!("[internal_deposit] deposit to reviewer: {}, {}", sender_id, reviewer_id);

        let mut reviewer_info = self.reviewers.get(&reviewer_id).expect("Reviewer not found");
        let mut delegator_info = reviewer_info.delegators.get(&sender_id).unwrap_or(DelegatorInfo {
            deposit_info: DepositInfo {
                deposit_amount: 0,
                reward_debt: 0,
            },
            refunding_amount: 0,
            refunding_start_timestamp: 0,
        });

        let reward = self.pending_reward(
            delegator_info.deposit_info.deposit_amount,
            delegator_info.deposit_info.reward_debt,
            reviewer_info.acc_reward_per_share
        );
        delegator_info.deposit_info.deposit_amount += amount + reward;
        delegator_info.deposit_info.reward_debt = delegator_info.deposit_info.deposit_amount * reviewer_info.acc_reward_per_share / 1_000_000_000_000;
        reviewer_info.delegators.insert(&sender_id, &delegator_info);
        
        reviewer_info.pending_amount += amount;
        reviewer_info.total_delegator_deposit_amount += amount + reward;
        self.reviewers.insert(&reviewer_id, &reviewer_info);

        // TODO: should emit success event
        return Promise::new(reviewer_id.clone());
    }

    fn internal_settlement(&mut self, sender_id: &AccountId, vapi_ids: Vec<String>, amounts: Vec<U128>) -> Promise {
        log!("[internal_settlement]");
        require!(*sender_id == self.owner_id, "Only owner can settle");
        require!(vapi_ids.len() == amounts.len(), "vapi_ids and amounts must have the same length");

        let mut total_burn_amount: u128 = 0;
        let mut total_treasury: Balance = 0;
        for (vapi_id, amount) in vapi_ids.iter().zip(amounts.iter()) {
            let amount: Balance = amount.0;
            let reviewer_fee_amount = amount * 39 / 100;
            let burn_amount = amount * 1 / 100;

            let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
            
            vapi.coder_info.unclaimed_reward_amount += amount - reviewer_fee_amount - burn_amount;

            if vapi.total_deposit_amount == 0 {
                total_treasury += reviewer_fee_amount;
            } else {
                vapi.acc_reward_per_share += reviewer_fee_amount * 1_000_000_000_000 / vapi.total_deposit_amount;
            }
            
            self.vapis.insert(&vapi_id, &vapi);
            total_burn_amount += burn_amount;
        }

        self.treasury += total_treasury;

        return ext_ft_burn::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .burn(U128(total_burn_amount));
    }
}