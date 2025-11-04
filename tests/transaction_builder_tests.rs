use monero_rust::{TransactionPriority, TransactionConfig};

#[test]
fn test_priority_from_u8() {
    assert!(matches!(
        TransactionPriority::from_u8(0),
        Ok(TransactionPriority::Default)
    ));
    assert!(matches!(
        TransactionPriority::from_u8(1),
        Ok(TransactionPriority::Low)
    ));
    assert!(matches!(
        TransactionPriority::from_u8(2),
        Ok(TransactionPriority::Medium)
    ));
    assert!(matches!(
        TransactionPriority::from_u8(3),
        Ok(TransactionPriority::High)
    ));
    assert!(matches!(
        TransactionPriority::from_u8(4),
        Ok(TransactionPriority::Unimportant)
    ));
    assert!(TransactionPriority::from_u8(5).is_err());
    assert!(TransactionPriority::from_u8(255).is_err());
}

#[test]
fn test_priority_to_fee_priority() {
    use monero_wallet::rpc::FeePriority;

    assert!(matches!(
        TransactionPriority::Unimportant.to_fee_priority(),
        FeePriority::Unimportant
    ));
    assert!(matches!(
        TransactionPriority::Low.to_fee_priority(),
        FeePriority::Normal
    ));
    assert!(matches!(
        TransactionPriority::Default.to_fee_priority(),
        FeePriority::Normal
    ));
    assert!(matches!(
        TransactionPriority::Medium.to_fee_priority(),
        FeePriority::Normal
    ));
    assert!(matches!(
        TransactionPriority::High.to_fee_priority(),
        FeePriority::Elevated
    ));
}

#[test]
fn test_transaction_config_default() {
    let config = TransactionConfig::default();

    assert!(matches!(config.priority, TransactionPriority::Default));
    assert_eq!(config.account_index, 0);
    assert!(config.preferred_inputs.is_none());
    assert!(config.payment_id.is_none());
    assert!(!config.sweep_all);
}

#[test]
fn test_transaction_config_clone() {
    let config = TransactionConfig {
        priority: TransactionPriority::High,
        account_index: 1,
        preferred_inputs: Some(vec![[1u8; 32]]),
        payment_id: Some(vec![0x42; 8]),
        sweep_all: true,
    };

    let cloned = config.clone();
    assert!(matches!(cloned.priority, TransactionPriority::High));
    assert_eq!(cloned.account_index, 1);
    assert!(cloned.preferred_inputs.is_some());
    assert!(cloned.payment_id.is_some());
    assert!(cloned.sweep_all);
}
