use neo_primitives::contract_basic_method::ContractBasicMethod;

#[test]
fn contract_basic_method_verify() {
    assert_eq!(ContractBasicMethod::VERIFY, "verify");
}

#[test]
fn contract_basic_method_initialize() {
    assert_eq!(ContractBasicMethod::INITIALIZE, "_initialize");
}

#[test]
fn contract_basic_method_deploy() {
    assert_eq!(ContractBasicMethod::DEPLOY, "_deploy");
}

#[test]
fn contract_basic_method_update() {
    assert_eq!(ContractBasicMethod::UPDATE, "update");
}

#[test]
fn contract_basic_method_destroy() {
    assert_eq!(ContractBasicMethod::DESTROY, "destroy");
}

#[test]
fn contract_basic_method_verify_parameter_count() {
    assert_eq!(ContractBasicMethod::VERIFY_P_COUNT, -1);
}

#[test]
fn contract_basic_method_initialize_parameter_count() {
    assert_eq!(ContractBasicMethod::INITIALIZE_P_COUNT, 0);
}

#[test]
fn contract_basic_method_deploy_parameter_count() {
    assert_eq!(ContractBasicMethod::DEPLOY_P_COUNT, 2);
}

#[test]
fn contract_basic_method_update_parameter_count() {
    assert_eq!(ContractBasicMethod::UPDATE_P_COUNT, 3);
}

#[test]
fn contract_basic_method_destroy_parameter_count() {
    assert_eq!(ContractBasicMethod::DESTROY_P_COUNT, 0);
}

#[test]
fn contract_basic_method_names_are_unique() {
    let methods = [
        ContractBasicMethod::VERIFY,
        ContractBasicMethod::INITIALIZE,
        ContractBasicMethod::DEPLOY,
        ContractBasicMethod::UPDATE,
        ContractBasicMethod::DESTROY,
    ];

    let unique_count = methods
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, methods.len());
}

#[test]
fn contract_basic_method_parameter_counts_are_valid() {
    assert!(ContractBasicMethod::VERIFY_P_COUNT == -1 || ContractBasicMethod::VERIFY_P_COUNT >= 0);
    assert!(ContractBasicMethod::INITIALIZE_P_COUNT >= 0);
    assert!(ContractBasicMethod::DEPLOY_P_COUNT >= 0);
    assert!(ContractBasicMethod::UPDATE_P_COUNT >= 0);
    assert!(ContractBasicMethod::DESTROY_P_COUNT >= 0);
}

#[test]
fn contract_basic_method_names_follow_convention() {
    // Internal methods start with underscore
    assert!(ContractBasicMethod::INITIALIZE.starts_with('_'));
    assert!(ContractBasicMethod::DEPLOY.starts_with('_'));

    // Public methods don't start with underscore
    assert!(!ContractBasicMethod::VERIFY.starts_with('_'));
    assert!(!ContractBasicMethod::UPDATE.starts_with('_'));
    assert!(!ContractBasicMethod::DESTROY.starts_with('_'));
}

#[test]
fn contract_basic_method_names_are_lowercase() {
    assert!(
        ContractBasicMethod::VERIFY
            .chars()
            .all(|c| c.is_lowercase() || c == '_')
    );
    assert!(
        ContractBasicMethod::INITIALIZE
            .chars()
            .all(|c| c.is_lowercase() || c == '_')
    );
    assert!(
        ContractBasicMethod::DEPLOY
            .chars()
            .all(|c| c.is_lowercase() || c == '_')
    );
    assert!(
        ContractBasicMethod::UPDATE
            .chars()
            .all(|c| c.is_lowercase() || c == '_')
    );
    assert!(
        ContractBasicMethod::DESTROY
            .chars()
            .all(|c| c.is_lowercase() || c == '_')
    );
}
