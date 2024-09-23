use crate::smartcontract::manifest::{ABI, Method, Event, Parameter};
use crate::smartcontract::ParameterType;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_is_valid() {
        let mut a = ABI::default();
        assert!(a.is_valid().is_err()); // No methods.

        a.methods.push(Method { name: "qwe".to_string(), ..Default::default() });
        assert!(a.is_valid().is_ok());

        a.methods.push(Method { name: "qaz".to_string(), ..Default::default() });
        assert!(a.is_valid().is_ok());

        a.methods.push(Method { name: "qaz".to_string(), offset: -42, ..Default::default() });
        assert!(a.is_valid().is_err());

        a.methods.pop();
        a.methods.push(Method {
            name: "qwe".to_string(),
            parameters: vec![Parameter::new("param".to_string(), ParameterType::Boolean)],
            ..Default::default()
        });
        assert!(a.is_valid().is_ok());

        a.methods.push(Method { name: "qwe".to_string(), ..Default::default() });
        assert!(a.is_valid().is_err());
        a.methods.pop();

        a.events.push(Event { name: "wsx".to_string(), ..Default::default() });
        assert!(a.is_valid().is_ok());

        a.events.push(Event::default());
        assert!(a.is_valid().is_err());

        a.events.pop();
        a.events.push(Event { name: "edc".to_string(), ..Default::default() });
        assert!(a.is_valid().is_ok());

        a.events.push(Event { name: "wsx".to_string(), ..Default::default() });
        assert!(a.is_valid().is_err());
    }
}
