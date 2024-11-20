use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(untagged)]
enum TokenReceiverMessage {
    Settlement {
        vapi_ids: Vec<String>,
        amounts: Vec<U128>,
    },
    DepositToReviewer {
        reviewer_id: AccountId,
    },
}

#[near]
impl FungibleTokenReceiver for TicleCore {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        let token_id: AccountId = env::predecessor_account_id();
        require!(token_id == self.token_id, "Invalid token");

        if msg.is_empty() {
            return PromiseOrValue::Value(U128(0));
        }

        log!("[ft_on_transfer] sender_id: {}", sender_id);
        log!("[ft_on_transfer] msg: {}", msg);
        let message = serde_json::from_str::<TokenReceiverMessage>(&msg).expect("Invalid message format");
        log!("[ft_on_transfer] selected message");

        match message {
            TokenReceiverMessage::DepositToReviewer { reviewer_id } => {
                self.internal_deposit_to_reviewer(&sender_id, &reviewer_id, amount.into());
            }
            TokenReceiverMessage::Settlement { vapi_ids, amounts } => {
                self.internal_settlement(&sender_id, vapi_ids, amounts);
            }
        }

        return PromiseOrValue::Value(U128(0));
    }
}