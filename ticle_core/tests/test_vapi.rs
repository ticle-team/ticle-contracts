use near_sdk::{json_types::U128, NearToken};
use serde_json::json;
use ticle_core::GetDepositInfoResponse;

use crate::common::utils::*;
pub mod common;

#[tokio::test]
async fn test_deposit() -> anyhow::Result<()> {
    let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, core_contract) = init(&worker, initial_balance).await?;

    register_user(&ft_contract, core_contract.id()).await?;

    let users = create_users(&worker, vec!["alice", "bob", "reviewer"], vec![10, 10, 10]).await?;
    for user in users.iter() {
        register_user(&ft_contract, user.id()).await?;

        let res = owner.transfer_near(user.id(), NearToken::from_near(1)).await?;
        assert!(res.is_success());

        let res = owner
            .call(ft_contract.id(), "ft_transfer")
            .args_json((user.id(), U128::from(NearToken::from_near(100).as_yoctonear()), "transfer to test account"))
            .max_gas()
            .deposit(ONE_YOCTO)
            .transact()
            .await?;
        assert!(res.is_success());
    }

    let alice = users.get(0).unwrap().clone();
    let bob = users.get(1).unwrap().clone();
    let reviewer = users.get(2).unwrap().clone();

    let vapi_id = "test-vapi";
    let res = owner
        .call(core_contract.id(), "create_vapi")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = owner
        .call(core_contract.id(), "create_reviewer")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let transfer_balance = U128::from(NearToken::from_near(10).as_yoctonear());

    // Alice deposits 10 tokens into the Reviewer
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "reviewer_id": reviewer.id() }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Bob deposits 10 tokens into the Reviewer
    let res = bob
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "reviewer_id": reviewer.id() }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Reviewer deposit 10 tokens into the VAPI
    let res = reviewer
        .call(core_contract.id(), "deposit_to_vapi")
        .args_json(json!({"vapi_id": vapi_id, "amount": transfer_balance}))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Settle 10 tokens
    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_ids": vec![vapi_id], "amounts": vec![transfer_balance] }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Compound reward
    let res = owner
        .call(core_contract.id(), "compound")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // reviewer royalty amount = 0.039
    // 10(Settlement) * 39%(Usage fee) * 1%(Royalty fee) = 0.039
    let reviewer_royalty_amount = core_contract
        .call("get_reviewer_royalty_amount")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(reviewer_royalty_amount, 39000000000000000000000);

    // Alice deposit amount = 10, reward = 1.9305
    // 10(Deposit) * 39%(Usage fee) * 99%(Usage fee - Royalty fee) * 50%(2 people) = 1.9305
    let alice_deposit_info = core_contract
        .call("get_delegator_deposit_info")
        .args_json(json!({"delegator_id": alice.id(), "reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(alice_deposit_info.deposit_amount, U128::from(NearToken::from_near(10).as_yoctonear()).into());
    assert_eq!(alice_deposit_info.reward, 1930500000000000000000000);

    // Bob deposit amount = 10, reward = 1.9305
    // 10(Deposit) * 39%(Usage fee) * 99%(Usage fee - Royalty fee) * 50%(2 people) = 1.9305
    let bob_deposit_info = core_contract
        .call("get_delegator_deposit_info")
        .args_json(json!({"delegator_id": bob.id(), "reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(bob_deposit_info.deposit_amount, U128::from(NearToken::from_near(10).as_yoctonear()).into());
    assert_eq!(bob_deposit_info.reward, 1930500000000000000000000);

    // Alice deposits additional 10 tokens into the Reviewer
    // During this process, unclaimed tokens (1.9305) are aggregate to Alice's balance
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "reviewer_id": reviewer.id() }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());
    
    // Alice deposit amount = 21.9305, reward = 0
    let alice_deposit_info = core_contract
        .call("get_delegator_deposit_info")
        .args_json(json!({"delegator_id": alice.id(), "reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(alice_deposit_info.deposit_amount, 21930500000000000000000000);
    assert_eq!(alice_deposit_info.reward, 0);

    // Settle 10 tokens
    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_ids": vec![vapi_id], "amounts": vec![transfer_balance] }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Compound reward
    let res = owner
        .call(core_contract.id(), "compound")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Alice deposit amount = 21.9305, reward = 2.651811...
    // 21.9305(Deposit) * 39%(Usage fee) * 99%(Usage fee - Royalty fee) * 21.9305 / 31.9305 = 2.651811...
    let alice_deposit_info = core_contract
        .call("get_delegator_deposit_info")
        .args_json(json!({"delegator_id": alice.id(), "reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(alice_deposit_info.deposit_amount, 21930500000000000000000000);
    assert_eq!(alice_deposit_info.reward, 2651811293250365500000000);

    // Bob deposit amount = 10, reward = 3.139688..
    // 1.9305(before reward) + (10(Deposit) * 39%(Usage fee) * 99%(Usage fee - Royalty fee) * 10 / 31.9305 = 3.139688..
    let bob_deposit_info = core_contract
        .call("get_delegator_deposit_info")
        .args_json(json!({"delegator_id": bob.id(), "reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(bob_deposit_info.deposit_amount, U128::from(NearToken::from_near(10).as_yoctonear()).into());
    assert_eq!(bob_deposit_info.reward, 3139688706710000000000000);

    return Ok(());
}

#[tokio::test]
async fn test_withdraw() -> anyhow::Result<()> {
let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, core_contract) = init(&worker, initial_balance).await?;

    register_user(&ft_contract, core_contract.id()).await?;

    let users = create_users(&worker, vec!["alice", "reviewer"], vec![10, 10]).await?;
    for user in users.iter() {
        register_user(&ft_contract, user.id()).await?;

        let res = owner.transfer_near(user.id(), NearToken::from_near(1)).await?;
        assert!(res.is_success());

        let res = owner
            .call(ft_contract.id(), "ft_transfer")
            .args_json((user.id(), U128::from(NearToken::from_near(100).as_yoctonear()), "transfer to test account"))
            .max_gas()
            .deposit(ONE_YOCTO)
            .transact()
            .await?;
        assert!(res.is_success());
    }

    let alice = users.get(0).unwrap().clone();
    let reviewer = users.get(1).unwrap().clone();

    let vapi_id = "test-vapi";
    let res = owner
        .call(core_contract.id(), "create_vapi")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = owner
        .call(core_contract.id(), "create_reviewer")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let transfer_balance = U128::from(NearToken::from_near(10).as_yoctonear());

    // Alice deposits 10 tokens into the Reviewer
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "reviewer_id": reviewer.id() }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    // Reviewer deposits 10 tokens into the VAPI
    let res = reviewer
        .call(core_contract.id(), "deposit_to_vapi")
        .args_json(json!({"vapi_id": vapi_id, "amount": transfer_balance}))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    // Reviewer withdraws 5 tokens from the VAPI
    let withdraw_amount = U128::from(NearToken::from_near(5).as_yoctonear());
    let res = reviewer
        .call(core_contract.id(), "withdraw_from_vapi")
        .args_json(json!({"vapi_id": vapi_id, "amount": withdraw_amount}))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Check Reviewer's deposit amount after withdrawal
    let reviewer_deposit_info = core_contract
        .call("get_reviewer_deposit_info")
        .args_json(json!({"reviewer_id": reviewer.id(), "vapi_id": vapi_id}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(reviewer_deposit_info.deposit_amount, U128::from(NearToken::from_near(5).as_yoctonear()).into());

    // Check Reviewer's pending amount after withdrawal
    let reviewer_pending_amount = core_contract
        .call("get_reviewer_pending_amount")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(reviewer_pending_amount, NearToken::from_near(5).as_yoctonear());

    // Check Reviewer's royalty amount after withdrawal
    let reviewer_royalty_amount = core_contract
        .call("get_reviewer_royalty_amount")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(reviewer_royalty_amount, 0);

    // Settlement 10 tokens
    let settlement_amount = U128::from(NearToken::from_near(10).as_yoctonear());
    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), settlement_amount, Option::<String>::None, serde_json::json!({ "vapi_ids": vec![vapi_id], "amounts": vec![settlement_amount] }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Compound
    let res = reviewer
        .call(core_contract.id(), "compound")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // Check Reviewer's royalty amount after compound
    let reviewer_royalty_amount = core_contract
        .call("get_reviewer_royalty_amount")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(reviewer_royalty_amount, 39000000000000000000000);

    return Ok(());
}

#[tokio::test]
#[ignore = "fast_forward is too slow, inquiring with the foundation."]
async fn test_refund() -> anyhow::Result<()> {
    let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, core_contract) = init(&worker, initial_balance).await?;

    register_user(&ft_contract, core_contract.id()).await?;

    let users = create_users(&worker, vec!["alice", "reviewer"], vec![10, 10]).await?;
    for user in users.iter() {
        register_user(&ft_contract, user.id()).await?;

        let res = owner.transfer_near(user.id(), NearToken::from_near(1)).await?;
        assert!(res.is_success());

        let res = owner
            .call(ft_contract.id(), "ft_transfer")
            .args_json((user.id(), U128::from(NearToken::from_near(100).as_yoctonear()), "transfer to test account"))
            .max_gas()
            .deposit(ONE_YOCTO)
            .transact()
            .await?;
        assert!(res.is_success());
    }

    let res = owner.transfer_near(core_contract.id(), NearToken::from_near(1)).await?;
    assert!(res.is_success());

    let alice = users.get(0).unwrap().clone();
    let reviewer = users.get(1).unwrap().clone();

    let vapi_ids = vec!["test-vapi-a", "test-vapi-b"];
    for vapi_id in vapi_ids.iter() {
        let res = owner
            .call(core_contract.id(), "create_vapi")
            .args_json(json!({"vapi_id": vapi_id}))
            .max_gas()
            .transact()
            .await?;
        assert!(res.is_success());
    }

    let res = owner
        .call(core_contract.id(), "create_reviewer")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let transfer_balance = U128::from(NearToken::from_near(10).as_yoctonear());

    // Alice deposits 10 tokens into the Reviewer
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "reviewer_id": reviewer.id() }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    // Reviewer deposits 3 tokens into the test-vapi-a
    let res = reviewer
        .call(core_contract.id(), "deposit_to_vapi")
        .args_json(json!({"vapi_id": vapi_ids[0], "amount": U128::from(NearToken::from_near(3).as_yoctonear())}))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    // Reviewer deposits 4 tokens into the test-vapi-b
    let res = reviewer
        .call(core_contract.id(), "deposit_to_vapi")
        .args_json(json!({"vapi_id": vapi_ids[1], "amount": U128::from(NearToken::from_near(4).as_yoctonear())}))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    // Alice requests a refund of 5 tokens from the Reviewer
    let refund_amount = U128::from(NearToken::from_near(5).as_yoctonear());
    let res = alice
        .call(core_contract.id(), "delegator_request_refund")
        .args_json(json!({"reviewer_id": reviewer.id(), "amount": refund_amount}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());
    
    // pending amount should be 0
    let pending_amount = core_contract
        .call("get_reviewer_pending_amount")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(pending_amount, 0);

    // remain deposit amount should be 5
    let test_vapi_a_deposit_info = core_contract
        .call("get_reviewer_deposit_info")
        .args_json(json!({"reviewer_id": reviewer.id(), "vapi_id": vapi_ids[0]}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    let test_vapi_b_deposit_info = core_contract
        .call("get_reviewer_deposit_info")
        .args_json(json!({"reviewer_id": reviewer.id(), "vapi_id": vapi_ids[1]}))
        .view()
        .await?
        .json::<GetDepositInfoResponse>()?;
    assert_eq!(test_vapi_a_deposit_info.deposit_amount + test_vapi_b_deposit_info.deposit_amount, NearToken::from_near(5).as_yoctonear());

    // Alice cannot claim rewards immediately.
    let res = alice
        .call(core_contract.id(), "delegator_claim_refund")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_failure());

    // Alice can claim rewards after one week.
    const ONE_WEEKS: u64 = 60 * 60 * 24 * 7 * 4;
    worker.fast_forward(ONE_WEEKS).await?;

    let res = alice
        .call(core_contract.id(), "delegator_claim_refund")
        .args_json(json!({"reviewer_id": reviewer.id()}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    return Ok(());
}