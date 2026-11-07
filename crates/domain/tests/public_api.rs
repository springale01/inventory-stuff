use domain::{
    AddCmd, CounterSkuBuilder, Id, InvalidSku, Item, MovementKind, SkuBuilder, SkuFamily, Stock,
    StockAmount,
};

struct Hardware;
impl SkuFamily for Hardware {}

#[test]
fn public_value_types_build_commands_and_sequential_skus_without_internal_imports() {
    let prefix = SkuBuilder::new().add_field("BOLT").finalize().unwrap();
    let supplier = CounterSkuBuilder::new(None, Some(7))
        .add(prefix.as_str())
        .build::<Hardware>(Some(3))
        .unwrap();
    assert_eq!(supplier.supply().unwrap().as_str(), "BOLT-007");
    assert_eq!(supplier.supply().unwrap().as_str(), "BOLT-008");
    let item = Id::<Item>::new();
    let mut stock = Stock::new(item, 0_u64, 2_u64).unwrap();
    let movement = AddCmd::new(item, 3_u64, "Delivery")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(movement.kind, MovementKind::Added);
    assert_eq!(stock.quantity(), StockAmount::Count(3));
    assert!(matches!(
        CounterSkuBuilder::default()
            .add(" ")
            .build::<Hardware>(None),
        Err(InvalidSku::EmptyField { index: 0 })
    ));
}

#[test]
fn public_counter_is_unique_across_threads_and_exhaustion_does_not_wrap() {
    let supplier = std::sync::Arc::new(
        CounterSkuBuilder::default()
            .add("SKU")
            .build::<Hardware>(None)
            .unwrap(),
    );
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let supplier = supplier.clone();
            std::thread::spawn(move || {
                (0..25)
                    .map(|_| supplier.supply().unwrap())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let codes: std::collections::HashSet<_> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    assert_eq!(codes.len(), 100);
    let last = CounterSkuBuilder::new(None, Some(usize::MAX - 1))
        .add("SKU")
        .build::<Hardware>(None)
        .unwrap();
    assert!(last.supply().is_ok());
    assert!(last.supply().is_err());
    assert!(last.supply().is_err());
}
