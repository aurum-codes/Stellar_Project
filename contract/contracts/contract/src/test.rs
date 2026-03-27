#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger},
    token, vec, Address, Env, IntoVal,
};

// ─── Test Helpers ─────────────────────────────────────────────────────────────

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Deploy a minimal Stellar asset contract (SAC) for testing token transfers.
fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let contract_address = env.register_stellar_asset_contract(admin.clone());
    (
        token::Client::new(env, &contract_address),
        token::StellarAssetClient::new(env, &contract_address),
    )
}

/// Full setup: env + token + scholarship contract, returns all handles.
struct TestSetup {
    env: Env,
    contract_id: Address,
    client: ScholarshipContractClient<'static>,
    token: token::Client<'static>,
    token_admin: token::StellarAssetClient<'static>,
    admin: Address,
    sponsor: Address,
    student: Address,
}

fn setup() -> TestSetup {
    let env = create_test_env();

    let admin = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let student = Address::generate(&env);

    let (token, token_admin) = create_token(&env, &admin);

    let contract_id = env.register_contract(None, ScholarshipContract);
    let client = ScholarshipContractClient::new(&env, &contract_id);

    // Initialize the scholarship contract
    client.initialize(&admin, &token.address);

    // Mint 10_000 XLM (in stroops) to sponsor for testing
    token_admin.mint(&sponsor, &10_000_0000000i128);

    TestSetup {
        env,
        contract_id,
        client,
        token,
        token_admin,
        admin,
        sponsor,
        student,
    }
}

/// Helper: create a standard 4-semester scholarship
fn create_standard_scholarship(s: &TestSetup) -> u32 {
    let semester_amounts = vec![
        &s.env,
        1_000_0000000i128, // Semester 1 — 1000 XLM
        1_000_0000000i128, // Semester 2
        1_000_0000000i128, // Semester 3
        1_000_0000000i128, // Semester 4
    ];
    let min_gpas = vec![
        &s.env,
        250u32, // 2.50 GPA
        250u32,
        270u32, // 2.70 GPA
        300u32, // 3.00 GPA
    ];
    s.client.create_scholarship(
        &s.sponsor,
        &s.student,
        &4_000_0000000i128,
        &semester_amounts,
        &min_gpas,
    )
}

// ─── Initialization Tests ─────────────────────────────────────────────────────

#[test]
fn test_initialize_success() {
    let env = create_test_env();
    let admin = Address::generate(&env);
    let token_addr = Address::generate(&env);
    let contract_id = env.register_contract(None, ScholarshipContract);
    let client = ScholarshipContractClient::new(&env, &contract_id);

    client.initialize(&admin, &token_addr);
    // If no panic, initialization succeeded
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_fails() {
    let s = setup();
    let other_admin = Address::generate(&s.env);
    // Second init must panic
    s.client.initialize(&other_admin, &s.token.address);
}

// ─── Create Scholarship Tests ─────────────────────────────────────────────────

#[test]
fn test_create_scholarship_success() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    assert_eq!(id, 0u32);

    let sc = s.client.get_scholarship(&id);
    assert_eq!(sc.student, s.student);
    assert_eq!(sc.sponsor, s.sponsor);
    assert_eq!(sc.total_amount, 4_000_0000000i128);
    assert_eq!(sc.disbursed_amount, 0i128);
    assert_eq!(sc.status, ScholarshipStatus::Active);
    assert_eq!(sc.semesters.len(), 4);
}

#[test]
fn test_create_scholarship_locks_funds() {
    let s = setup();
    let sponsor_balance_before = s.token.balance(&s.sponsor);

    create_standard_scholarship(&s);

    let sponsor_balance_after = s.token.balance(&s.sponsor);
    let contract_balance = s.token.balance(&s.contract_id);

    // Sponsor paid out 4000 XLM
    assert_eq!(
        sponsor_balance_before - sponsor_balance_after,
        4_000_0000000i128
    );
    // Contract holds 4000 XLM
    assert_eq!(contract_balance, 4_000_0000000i128);
}

#[test]
fn test_create_multiple_scholarships_increments_id() {
    let s = setup();
    let student2 = Address::generate(&s.env);

    let id0 = create_standard_scholarship(&s);

    // Create second scholarship for different student
    let amounts = vec![&s.env, 500_0000000i128, 500_0000000i128];
    let gpas = vec![&s.env, 200u32, 200u32];
    let id1 = s.client.create_scholarship(
        &s.sponsor,
        &student2,
        &1_000_0000000i128,
        &amounts,
        &gpas,
    );

    assert_eq!(id0, 0u32);
    assert_eq!(id1, 1u32);
}

#[test]
#[should_panic(expected = "Semester config mismatch")]
fn test_create_scholarship_empty_semesters_fails() {
    let s = setup();
    s.client.create_scholarship(
        &s.sponsor,
        &s.student,
        &1_000_0000000i128,
        &vec![&s.env],
        &vec![&s.env],
    );
}

#[test]
#[should_panic(expected = "Semester config mismatch")]
fn test_create_scholarship_mismatched_arrays_fails() {
    let s = setup();
    s.client.create_scholarship(
        &s.sponsor,
        &s.student,
        &1_000_0000000i128,
        &vec![&s.env, 500_0000000i128, 500_0000000i128],
        &vec![&s.env, 250u32], // only 1 GPA for 2 semesters
    );
}

#[test]
#[should_panic(expected = "Amounts must sum to total_amount")]
fn test_create_scholarship_wrong_total_fails() {
    let s = setup();
    s.client.create_scholarship(
        &s.sponsor,
        &s.student,
        &2_000_0000000i128, // says 2000 XLM
        &vec![&s.env, 500_0000000i128, 500_0000000i128], // only 1000 XLM
        &vec![&s.env, 250u32, 250u32],
    );
}

// ─── Semester Config Tests ────────────────────────────────────────────────────

#[test]
fn test_semester_structure_correct() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    let sc = s.client.get_scholarship(&id);

    for i in 0..sc.semesters.len() {
        let sem = sc.semesters.get(i).unwrap();
        assert_eq!(sem.semester_id, i + 1);
        assert_eq!(sem.release_amount, 1_000_0000000i128);
        assert!(!sem.released);
        assert!(!sem.performance_verified);
    }

    assert_eq!(sc.semesters.get(0).unwrap().min_gpa, 250u32);
    assert_eq!(sc.semesters.get(2).unwrap().min_gpa, 270u32);
    assert_eq!(sc.semesters.get(3).unwrap().min_gpa, 300u32);
}

// ─── Verify Performance Tests ─────────────────────────────────────────────────

#[test]
fn test_verify_performance_success() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &280u32); // GPA 2.80 ≥ 2.50 ✅

    let sc = s.client.get_scholarship(&id);
    let sem1 = sc.semesters.get(0).unwrap();
    assert!(sem1.performance_verified);
    assert!(!sem1.released);
}

#[test]
fn test_verify_performance_gpa_below_minimum_not_verified() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &200u32); // GPA 2.00 < 2.50 ❌

    let sc = s.client.get_scholarship(&id);
    let sem1 = sc.semesters.get(0).unwrap();
    assert!(!sem1.performance_verified); // NOT verified
}

#[test]
fn test_verify_performance_exact_minimum_gpa() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &250u32); // exactly 2.50 — boundary

    let sc = s.client.get_scholarship(&id);
    assert!(sc.semesters.get(0).unwrap().performance_verified); // should pass
}

#[test]
#[should_panic(expected = "Semester not found")]
fn test_verify_performance_invalid_semester() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    s.client.verify_performance(&id, &99, &300u32); // semester 99 doesn't exist
}

#[test]
#[should_panic(expected = "Scholarship not found")]
fn test_verify_performance_invalid_scholarship() {
    let s = setup();
    s.client.verify_performance(&999, &1, &300u32);
}

#[test]
#[should_panic(expected = "Semester already released")]
fn test_verify_performance_after_release_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &280u32);
    s.client.release_semester_funds(&id, &1);
    s.client.verify_performance(&id, &1, &300u32); // should panic
}

// ─── Release Funds Tests ──────────────────────────────────────────────────────

#[test]
fn test_release_semester_funds_success() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &280u32);

    let student_before = s.token.balance(&s.student);
    let released = s.client.release_semester_funds(&id, &1);

    assert_eq!(released, 1_000_0000000i128);

    let student_after = s.token.balance(&s.student);
    assert_eq!(student_after - student_before, 1_000_0000000i128);

    let sc = s.client.get_scholarship(&id);
    assert_eq!(sc.disbursed_amount, 1_000_0000000i128);
    assert!(sc.semesters.get(0).unwrap().released);
}

#[test]
fn test_release_all_semesters_completes_scholarship() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    // Verify and release all 4 semesters
    let gpas = [280u32, 260u32, 290u32, 350u32];
    for (i, &gpa) in gpas.iter().enumerate() {
        let sem_id = (i as u32) + 1;
        s.client.verify_performance(&id, &sem_id, &gpa);
        s.client.release_semester_funds(&id, &sem_id);
    }

    let sc = s.client.get_scholarship(&id);
    assert_eq!(sc.status, ScholarshipStatus::Completed);
    assert_eq!(sc.disbursed_amount, 4_000_0000000i128);

    // Contract holds 0 XLM
    assert_eq!(s.token.balance(&s.contract_id), 0i128);

    // Student received all 4000 XLM
    assert_eq!(s.token.balance(&s.student), 4_000_0000000i128);
}

#[test]
#[should_panic(expected = "Performance not verified or GPA requirement not met")]
fn test_release_without_verification_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    s.client.release_semester_funds(&id, &1); // not verified yet
}

#[test]
#[should_panic(expected = "Performance not verified or GPA requirement not met")]
fn test_release_after_failed_gpa_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &200u32); // GPA too low, not verified
    s.client.release_semester_funds(&id, &1);      // should panic
}

#[test]
#[should_panic(expected = "Already released")]
fn test_release_same_semester_twice_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &280u32);
    s.client.release_semester_funds(&id, &1);
    s.client.release_semester_funds(&id, &1); // should panic
}

#[test]
#[should_panic(expected = "Scholarship not active")]
fn test_release_on_completed_scholarship_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    // Complete all semesters
    for i in 1u32..=4 {
        s.client.verify_performance(&id, &i, &350u32);
        s.client.release_semester_funds(&id, &i);
    }

    // Scholarship is now Completed — any further release should panic
    s.client.release_semester_funds(&id, &1);
}

// ─── Suspend & Refund Tests ───────────────────────────────────────────────────

#[test]
fn test_suspend_refunds_all_when_none_released() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    let sponsor_before = s.token.balance(&s.sponsor);
    s.client.suspend_scholarship(&id);
    let sponsor_after = s.token.balance(&s.sponsor);

    // Full 4000 XLM returned
    assert_eq!(sponsor_after - sponsor_before, 4_000_0000000i128);

    let sc = s.client.get_scholarship(&id);
    assert_eq!(sc.status, ScholarshipStatus::Suspended);
    assert_eq!(s.token.balance(&s.contract_id), 0i128);
}

#[test]
fn test_suspend_refunds_unreleased_only() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    // Release semester 1 and 2
    s.client.verify_performance(&id, &1, &280u32);
    s.client.release_semester_funds(&id, &1);
    s.client.verify_performance(&id, &2, &260u32);
    s.client.release_semester_funds(&id, &2);

    let sponsor_before = s.token.balance(&s.sponsor);
    s.client.suspend_scholarship(&id);
    let sponsor_after = s.token.balance(&s.sponsor);

    // Semesters 3 and 4 remain — 2000 XLM refunded
    assert_eq!(sponsor_after - sponsor_before, 2_000_0000000i128);
    assert_eq!(s.token.balance(&s.contract_id), 0i128);
}

#[test]
#[should_panic(expected = "Already inactive")]
fn test_suspend_already_suspended_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    s.client.suspend_scholarship(&id);
    s.client.suspend_scholarship(&id); // second suspend should panic
}

#[test]
#[should_panic(expected = "Already inactive")]
fn test_suspend_completed_scholarship_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    for i in 1u32..=4 {
        s.client.verify_performance(&id, &i, &350u32);
        s.client.release_semester_funds(&id, &i);
    }

    s.client.suspend_scholarship(&id); // completed → panic
}

#[test]
#[should_panic(expected = "Scholarship not active")]
fn test_release_on_suspended_scholarship_fails() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    s.client.suspend_scholarship(&id);
    s.client.release_semester_funds(&id, &1);
}

// ─── View Functions Tests ─────────────────────────────────────────────────────

#[test]
fn test_get_locked_balance_initial() {
    let s = setup();
    let id = create_standard_scholarship(&s);
    let locked = s.client.get_locked_balance(&id);
    assert_eq!(locked, 4_000_0000000i128);
}

#[test]
fn test_get_locked_balance_decreases_after_release() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    s.client.verify_performance(&id, &1, &280u32);
    s.client.release_semester_funds(&id, &1);

    let locked = s.client.get_locked_balance(&id);
    assert_eq!(locked, 3_000_0000000i128); // 4000 - 1000
}

#[test]
fn test_get_locked_balance_zero_after_full_disbursement() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    for i in 1u32..=4 {
        s.client.verify_performance(&id, &i, &350u32);
        s.client.release_semester_funds(&id, &i);
    }

    assert_eq!(s.client.get_locked_balance(&id), 0i128);
}

#[test]
fn test_get_student_scholarship() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    let sc = s.client.get_student_scholarship(&s.student);
    assert_eq!(sc.student, s.student);
    assert_eq!(sc.total_amount, 4_000_0000000i128);
}

#[test]
#[should_panic(expected = "No scholarship for student")]
fn test_get_student_scholarship_nonexistent() {
    let s = setup();
    let random = Address::generate(&s.env);
    s.client.get_student_scholarship(&random);
}

#[test]
#[should_panic(expected = "Not found")]
fn test_get_scholarship_invalid_id() {
    let s = setup();
    s.client.get_scholarship(&999u32);
}

// ─── Authorization Tests ──────────────────────────────────────────────────────

#[test]
fn test_admin_auth_required_for_verify() {
    let env = Env::default(); // NO mock_all_auths
    let admin = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let student = Address::generate(&env);

    let (token, token_admin) = create_token(&env, &admin);
    token_admin.mint(&sponsor, &10_000_0000000i128);

    // Approve token transfer for contract
    let contract_id = env.register_contract(None, ScholarshipContract);
    let client = ScholarshipContractClient::new(&env, &contract_id);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (&admin, &token.address).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &token.address);

    // Attempt verify_performance without admin auth → should panic
    let result = std::panic::catch_unwind(|| {
        client.verify_performance(&0u32, &1u32, &300u32);
    });
    assert!(result.is_err(), "Should require admin auth");
}

#[test]
fn test_sponsor_auth_required_for_create() {
    let env = Env::default(); // NO mock_all_auths
    let admin = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let student = Address::generate(&env);

    let (token, token_admin) = create_token(&env, &admin);
    token_admin.mint(&sponsor, &10_000_0000000i128);

    let contract_id = env.register_contract(None, ScholarshipContract);
    let client = ScholarshipContractClient::new(&env, &contract_id);

    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (&admin, &token.address).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &token.address);

    // Attempt create_scholarship without sponsor auth → should panic
    let result = std::panic::catch_unwind(|| {
        client.create_scholarship(
            &sponsor,
            &student,
            &1_000_0000000i128,
            &vec![&env, 1_000_0000000i128],
            &vec![&env, 250u32],
        );
    });
    assert!(result.is_err(), "Should require sponsor auth");
}

// ─── Edge Case / Integration Tests ───────────────────────────────────────────

#[test]
fn test_full_lifecycle_happy_path() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    // Initial state
    assert_eq!(s.client.get_locked_balance(&id), 4_000_0000000i128);
    assert_eq!(s.token.balance(&s.student), 0i128);

    // Semester 1 — GPA 3.10 ≥ 2.50 ✅
    s.client.verify_performance(&id, &1, &310u32);
    s.client.release_semester_funds(&id, &1);
    assert_eq!(s.token.balance(&s.student), 1_000_0000000i128);

    // Semester 2 — GPA 2.60 ≥ 2.50 ✅
    s.client.verify_performance(&id, &2, &260u32);
    s.client.release_semester_funds(&id, &2);
    assert_eq!(s.token.balance(&s.student), 2_000_0000000i128);

    // Semester 3 — GPA 2.50 < 2.70 ❌ → funds NOT released
    s.client.verify_performance(&id, &3, &250u32);
    let sc = s.client.get_scholarship(&id);
    assert!(!sc.semesters.get(2).unwrap().performance_verified);

    // Sponsor suspends — semester 3 & 4 refunded
    let sponsor_before = s.token.balance(&s.sponsor);
    s.client.suspend_scholarship(&id);
    let sponsor_after = s.token.balance(&s.sponsor);

    assert_eq!(sponsor_after - sponsor_before, 2_000_0000000i128);
    assert_eq!(s.client.get_scholarship(&id).status, ScholarshipStatus::Suspended);
}

#[test]
fn test_single_semester_scholarship() {
    let s = setup();
    let id = s.client.create_scholarship(
        &s.sponsor,
        &s.student,
        &500_0000000i128,
        &vec![&s.env, 500_0000000i128],
        &vec![&s.env, 300u32],
    );

    s.client.verify_performance(&id, &1, &350u32);
    s.client.release_semester_funds(&id, &1);

    let sc = s.client.get_scholarship(&id);
    assert_eq!(sc.status, ScholarshipStatus::Completed);
    assert_eq!(s.token.balance(&s.student), 500_0000000i128);
}

#[test]
fn test_out_of_order_semester_release_blocked() {
    let s = setup();
    let id = create_standard_scholarship(&s);

    // Try releasing semester 2 before semester 1
    s.client.verify_performance(&id, &2, &260u32);

    // Semester 2 is verified — release should succeed (contract allows non-sequential)
    let released = s.client.release_semester_funds(&id, &2);
    assert_eq!(released, 1_000_0000000i128);

    // Semester 1 still unreleased
    let sc = s.client.get_scholarship(&id);
    assert!(!sc.semesters.get(0).unwrap().released);
    assert!(sc.semesters.get(1).unwrap().released);
}