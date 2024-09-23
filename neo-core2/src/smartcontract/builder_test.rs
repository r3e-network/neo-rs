use crate::smartcontract::Builder;
use crate::util::Uint160;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        let mut b = Builder::new();
        assert_eq!(0, b.len());

        b.invoke_method(&Uint160::from([1, 2, 3]), "method");
        assert_eq!(37, b.len());

        b.invoke_method(
            &Uint160::from([1, 2, 3]),
            "transfer",
            &[
                Uint160::from([3, 2, 1]).into(),
                Uint160::from([9, 8, 7]).into(),
                100500.into(),
            ],
        );
        assert_eq!(126, b.len());

        let s = b.script().expect("Failed to get script");
        assert!(!s.is_empty());
        assert_eq!(126, s.len());

        b.reset();
        assert_eq!(0, b.len());
    }
}
